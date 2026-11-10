use std::path::PathBuf;
use std::sync::Arc;

use serde_json::{json, Map, Value};

use super::{blocking, SharedState};
use crate::config::{Config, Entry, STATUSES};
use crate::engines::{blacklist_set, is_windows_app, normalize_exe};
use crate::launcher::{self, LaunchError};
use crate::screenshots;
use crate::shell;
use crate::state::{Cmd, Shared};
use crate::vndb::VnResult;
use crate::vndb_list;

fn text(entry: &Entry, field: &str) -> String {
    entry
        .get(field)
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string()
}

pub fn visible_games(cfg: &Config) -> Vec<(String, Entry)> {
    let user: Vec<String> = cfg
        .get("blacklist_exe")
        .and_then(Value::as_array)
        .map(|list| {
            list.iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    let blacklist = blacklist_set(user.iter().map(String::as_str));

    cfg.all_games()
        .iter()
        .filter(|(key, entry)| {
            let path = text(entry, "path");
            let exe = if path.is_empty() {
                key.split('@').next().unwrap_or("").to_string()
            } else {
                path
            };
            !blacklist.contains(&normalize_exe(&exe)) && !is_windows_app(&exe)
        })
        .map(|(key, entry)| (key.clone(), entry.clone()))
        .collect()
}

fn cover_of(cfg: &Config, entry: &Entry) -> Option<String> {
    cfg.cached_cover(entry)
        .map(|p| p.to_string_lossy().to_string())
}

#[tauri::command(async)]
pub fn get_library(shared: SharedState) -> Vec<Value> {
    let cfg = shared.config.lock().unwrap();

    visible_games(&cfg)
        .into_iter()
        .map(|(key, entry)| {
            let vn_id = [text(&entry, "vndb_id"), text(&entry, "matched_vndb_id")]
                .into_iter()
                .find(|id| !id.is_empty())
                .unwrap_or_default();

            json!({
                "key": key,
                "name": screenshots::display_name(&key, &entry),
                "status": text(&entry, "status"),
                "playtime": entry.get("playtime_seconds").and_then(Value::as_i64).unwrap_or(0),
                "daily": entry.get("daily").cloned().unwrap_or(json!({})),
                "last_played": entry.get("last_played").and_then(Value::as_i64).unwrap_or(0),
                "finished_at": entry.get("finished_at").and_then(Value::as_i64).unwrap_or(0),
                "nsfw": entry.get("cover_nsfw").and_then(Value::as_bool).unwrap_or(false),
                "has_path": !launcher::launch_exe(&entry).is_empty(),
                "from_vndb": entry.get("vndb_list_only").and_then(Value::as_bool).unwrap_or(false)
                    && launcher::launch_exe(&entry).is_empty(),
                "released": if text(&entry, "released_vn") == vn_id { text(&entry, "released") } else { String::new() },
                "vndb_id": vn_id,
                "cover": cover_of(&cfg, &entry),
            })
        })
        .collect()
}

#[tauri::command]
pub async fn fill_release_dates(shared: SharedState<'_>) -> Result<usize, String> {
    let shared = Arc::clone(&shared);
    blocking(move || {
        let missing: Vec<(String, String)> = {
            let cfg = shared.config.lock().unwrap();
            visible_games(&cfg)
                .into_iter()
                .filter_map(|(key, entry)| {
                    let vn_id = [text(&entry, "vndb_id"), text(&entry, "matched_vndb_id")]
                        .into_iter()
                        .find(|id| !id.is_empty())?;
                    (text(&entry, "released_vn") != vn_id).then_some((key, vn_id))
                })
                .collect()
        };
        if missing.is_empty() {
            return Ok(0);
        }
        let mut ids: Vec<String> = missing.iter().map(|(_, id)| id.clone()).collect();
        ids.sort();
        ids.dedup();
        let dates: std::collections::HashMap<String, String> =
            shared.vndb.release_dates(&ids)?.into_iter().collect();
        {
            let mut cfg = shared.config.lock().unwrap();
            for (key, vn_id) in &missing {
                let date = dates.get(vn_id).cloned().unwrap_or_default();
                cfg.set_game(
                    key,
                    Map::from_iter([
                        ("released".into(), json!(date)),
                        ("released_vn".into(), json!(vn_id)),
                    ]),
                );
            }
        }
        shared.send(Cmd::Reload);
        shared.emit("library-changed", json!({}));
        Ok(missing.len())
    })
    .await
}

#[tauri::command(async)]
pub fn get_game(shared: SharedState, key: String) -> Value {
    let mut cfg = shared.config.lock().unwrap();
    let entry = cfg.game(&key);

    let folder = screenshots::folder_for(&mut cfg, &key, false);
    let shots = screenshots::list_shots(folder.as_deref());
    let launcher_name = text(&entry, "launcher");
    let tool = launcher::tool_path(&cfg, &launcher_name);
    let duplicates: Vec<Value> = cfg
        .duplicates_of(&key)
        .into_iter()
        .map(|other| {
            let other_entry = cfg.game(&other);
            json!({
                "key": other,
                "name": screenshots::display_name(&other, &other_entry),
                "playtime": other_entry.get("playtime_seconds").and_then(Value::as_i64).unwrap_or(0),
            })
        })
        .collect();

    json!({
        "key": key,
        "name": screenshots::display_name(&key, &entry),
        "entry": entry,
        "cover": cover_of(&cfg, &entry),
        "shots": shots.iter().take(5).map(|p| p.to_string_lossy().to_string()).collect::<Vec<_>>(),
        "shot_count": shots.len(),
        "tool": tool.map(|p| p.to_string_lossy().to_string()),
        "has_token": !cfg.str("vndb_token").trim().is_empty(),
        "vndb_sync": cfg.bool("vndb_sync"),
        "hotkey": cfg.str("screenshot_hotkey"),
        "duplicates": duplicates,
    })
}

fn changed(shared: &Shared, key: &str) {
    shared.send(Cmd::Reload);
    shared.emit("library-changed", json!({ "key": key }));
}

#[tauri::command(async)]
pub fn update_game(
    shared: SharedState,
    key: String,
    fields: Map<String, Value>,
) -> Result<(), String> {
    const EDITABLE: [&str; 5] = ["status", "privacy", "title", "launcher", "section"];

    for (field, value) in &fields {
        if !EDITABLE.contains(&field.as_str()) {
            return Err(format!("{field} can't be edited"));
        }
        if field == "status" && !(value == "" || STATUSES.iter().any(|s| value == *s)) {
            return Err(format!("unknown status {value}"));
        }
    }

    let status = fields
        .get("status")
        .and_then(Value::as_str)
        .map(str::to_string);
    let vn_id = {
        let mut cfg = shared.config.lock().unwrap();
        cfg.set_game(&key, fields);
        text(&cfg.game(&key), "vndb_id")
    };

    if let Some(status) = status {
        if !vn_id.is_empty() {
            vndb_list::sync_status(&shared, &key, &vn_id, &status, false);
        }
    }

    changed(&shared, &key);
    Ok(())
}

#[tauri::command(async)]
pub fn set_playtime(shared: SharedState, key: String, seconds: i64) {
    shared.send(Cmd::SetPlaytime(key.clone(), seconds));
    shared.emit("library-changed", json!({ "key": key }));
}

#[tauri::command(async)]
pub fn remove_game(shared: SharedState, key: String) -> Result<(), String> {
    let snap = shared.snapshot();
    if snap.detected && snap.key == key {
        return Err("Close the game before removing it.".into());
    }
    {
        let mut cfg = shared.config.lock().unwrap();
        if let Some(id) = key.strip_prefix(vndb_list::IMPORTED_PREFIX) {
            let mut ignored = cfg
                .get("vndb_import_ignored")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            ignored.push(json!(id));
            let _ = cfg.update(Map::from_iter([("vndb_import_ignored".into(), json!(ignored))]));
        }
        cfg.remove_game(&key);
    }
    changed(&shared, &key);
    Ok(())
}

#[tauri::command]
pub async fn recommendations(
    shared: SharedState<'_>,
    refresh: bool,
) -> Result<crate::reco::Recommendations, String> {
    let shared = Arc::clone(&shared);
    blocking(move || crate::reco::recommendations(&shared, refresh)).await
}

#[tauri::command]
pub async fn import_vndb_finished(shared: SharedState<'_>) -> Result<usize, String> {
    let shared = Arc::clone(&shared);
    blocking(move || vndb_list::import_finished(&shared)).await
}

#[tauri::command(async)]
pub fn merge_games(shared: SharedState, from: String, into: String) -> Result<(), String> {
    let snap = shared.snapshot();
    if snap.detected && (snap.key == from || snap.key == into) {
        return Err("Close the game before merging.".into());
    }
    let (from_shots, into_shots) = {
        let mut cfg = shared.config.lock().unwrap();
        let from_shots = screenshots::folder_for(&mut cfg, &from, false);
        let into_shots = screenshots::folder_for(&mut cfg, &into, false);
        cfg.merge_games(&from, &into);
        (from_shots, into_shots)
    };
    if let (Some(from_shots), Some(into_shots)) = (from_shots, into_shots) {
        screenshots::move_shots(&from_shots, &into_shots)?;
    }
    changed(&shared, &into);
    shared.emit("screenshots-changed", json!({ "key": into }));
    Ok(())
}

#[tauri::command(async)]
pub fn confirm_match(shared: SharedState, key: String) {
    {
        let mut cfg = shared.config.lock().unwrap();
        let entry = cfg.game(&key);
        let vn_id = text(&entry, "matched_vndb_id");
        if vn_id.is_empty() {
            return;
        }
        let mut fields = Map::from_iter([
            ("vndb_id".into(), json!(vn_id)),
            ("match_checked".into(), json!(true)),
        ]);
        let name = text(&entry, "name");
        if !name.is_empty() {
            fields.insert("title".into(), json!(name));
        }
        cfg.set_game(&key, fields);
    }
    changed(&shared, &key);
}

#[tauri::command(async)]
pub fn match_checked(shared: SharedState, key: String) {
    shared
        .config
        .lock()
        .unwrap()
        .set_game(&key, Map::from_iter([("match_checked".into(), json!(true))]));
    changed(&shared, &key);
}

#[tauri::command(async)]
pub fn set_game_path(shared: SharedState, key: String, path: String) {
    shared
        .config
        .lock()
        .unwrap()
        .set_game(&key, Map::from_iter([("launch_path".into(), json!(path))]));
    changed(&shared, &key);
}

#[tauri::command(async)]
pub fn set_tool_path(shared: SharedState, launcher: String, path: String) -> Result<(), String> {
    let setting = launcher::tool_setting(&launcher);
    if setting.is_empty() {
        return Err(format!("unknown launcher {launcher}"));
    }
    let settings = {
        let mut cfg = shared.config.lock().unwrap();
        cfg.update(Map::from_iter([(setting.into(), json!(path))]))
            .map_err(|e| e.to_string())?;
        cfg.data().clone()
    };
    shared.emit("settings-changed", settings);
    Ok(())
}

#[tauri::command(async)]
pub fn create_shortcut(shared: SharedState, key: String) -> Result<String, LaunchError> {
    let entry = shared.config.lock().unwrap().game(&key);
    let name = screenshots::display_name(&key, &entry);
    let app_exe = std::env::current_exe().map_err(|e| LaunchError::Failed(e.to_string()))?;
    let game_exe = PathBuf::from(launcher::launch_exe(&entry));
    if !game_exe.is_file() {
        return Err(LaunchError::GameMissing);
    }

    let file_name: String = name
        .chars()
        .map(|c| if "\\/:*?\"<>|".contains(c) { '_' } else { c })
        .collect();
    let dest = shell::desktop_folder().join(format!("{}.lnk", file_name.trim()));
    let shortcut = shell::Shortcut {
        target: &app_exe,
        arguments: &format!("--play \"{key}\""),
        icon: &game_exe,
        description: &format!("Play {name} with Shiori RPC"),
    };
    shell::create_shortcut(&dest, &shortcut).map_err(LaunchError::Failed)?;
    Ok(dest.to_string_lossy().to_string())
}

pub fn play_from_shortcut(shared: &Shared, key: &str) -> bool {
    let cfg = shared.config.lock().unwrap();
    let entry = cfg.game(key);
    !entry.is_empty() && launcher::launch(&cfg, &entry).is_ok()
}

#[tauri::command(async)]
pub fn play_game(shared: SharedState, key: String) -> Result<(), LaunchError> {
    let cfg = shared.config.lock().unwrap();
    let entry = cfg.game(&key);
    launcher::launch(&cfg, &entry)
}

#[tauri::command]
pub async fn vn_length(shared: SharedState<'_>, vn_id: String) -> Result<i64, String> {
    let shared = Arc::clone(&shared);
    blocking(move || Ok(shared.vndb.get(&vn_id)?.map_or(0, |vn| vn.length_minutes))).await
}

#[tauri::command]
pub async fn get_vote(shared: SharedState<'_>, key: String) -> Result<Option<i64>, String> {
    let shared = Arc::clone(&shared);
    blocking(move || vndb_list::vote(&shared, &key)).await
}

#[tauri::command]
pub async fn set_vote(
    shared: SharedState<'_>,
    key: String,
    vote: Option<i64>,
) -> Result<(), String> {
    let shared = Arc::clone(&shared);
    blocking(move || vndb_list::set_vote(&shared, &key, vote)).await
}

#[tauri::command]
pub async fn get_wishlist(shared: SharedState<'_>) -> Result<Value, String> {
    let shared = Arc::clone(&shared);
    blocking(move || {
        let (token, in_library) = {
            let cfg = shared.config.lock().unwrap();
            let ids: Vec<String> = cfg
                .all_games()
                .values()
                .flat_map(|e| [text(e, "vndb_id"), text(e, "matched_vndb_id")])
                .filter(|id| !id.is_empty())
                .collect();
            (cfg.str("vndb_token").trim().to_string(), ids)
        };
        if token.is_empty() {
            return Err("add your VNDB token in Settings first".into());
        }

        let wishlist: Vec<VnResult> = shared.vndb.wishlist(&token)?;
        Ok(json!({ "wishlist": wishlist, "in_library": in_library }))
    })
    .await
}
