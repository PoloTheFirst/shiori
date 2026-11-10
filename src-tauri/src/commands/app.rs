use std::sync::Arc;

use serde_json::{json, Map, Value};
use tauri::{AppHandle, Manager};

use super::{blocking, SharedState};
use crate::engines::{normalize_exe, BUILTIN_BLACKLIST};
use crate::state::{Cmd, Shared};
use crate::{autostart, discord, hotkey, mascot, presence, screenshots, shell, ui_windows, updater, winapi};

pub fn allow_screenshot_folder(shared: &Shared) {
    let root = screenshots::root(&shared.config.lock().unwrap());
    let _ = std::fs::create_dir_all(&root);
    let _ = shared
        .app
        .asset_protocol_scope()
        .allow_directory(&root, true);
}

#[tauri::command(async)]
pub fn get_state(shared: SharedState) -> Value {
    let cfg = shared.config.lock().unwrap();
    json!({
        "version": updater::CURRENT,
        "snapshot": shared.snapshot(),
        "status": shared.status(),
        "settings": cfg.data(),
        "data_dir": cfg.dir.to_string_lossy(),
        "autostart": autostart::is_enabled(),
        "screenshot_root": screenshots::root(&cfg).to_string_lossy(),
        "default_screenshot_root": screenshots::default_root().to_string_lossy(),
        "pending_page": ui_windows::take_pending_page(),
        "language": crate::lang::resolve(&cfg.str("language")),
    })
}

#[tauri::command(async)]
pub fn update_settings(
    shared: SharedState,
    app: AppHandle,
    mut changes: Map<String, Value>,
) -> Result<(), String> {
    if let Some(Value::String(key)) = changes.get("screenshot_hotkey") {
        let normalized = hotkey::normalize(key);
        changes.insert("screenshot_hotkey".into(), json!(normalized));
    }
    const NOT_ON_SCREEN: [&str; 5] = [
        "mascot_chatter_minutes",
        "mascot_balloons",
        "mascot_voice_volume",
        "mascot_voice_volumes",
        "mascot_click_hotkey",
    ];
    crate::crash::note(format!(
        "settings: {}",
        changes.keys().cloned().collect::<Vec<_>>().join(", ")
    ));
    let mascot_changed = changes
        .keys()
        .any(|k| k.starts_with("mascot_") && !NOT_ON_SCREEN.contains(&k.as_str()));
    let language_changed = changes.contains_key("language");

    shared
        .config
        .lock()
        .unwrap()
        .update(changes)
        .map_err(|e| e.to_string())?;

    shared.send(Cmd::Reload);
    allow_screenshot_folder(&shared);
    if mascot_changed {
        mascot::apply_settings(&app);
    }
    if language_changed {
        crate::tray::refresh(&app);
    }
    let settings = shared.config.lock().unwrap().data().clone();
    shared.emit("settings-changed", settings);
    Ok(())
}

#[tauri::command(async)]
pub fn set_paused(shared: SharedState, paused: bool) {
    shared.send(Cmd::Pause(paused));
}

#[tauri::command(async)]
pub fn set_autostart(enabled: bool) -> Result<(), String> {
    autostart::set_enabled(enabled)
}

#[tauri::command(async)]
pub fn not_a_vn(shared: SharedState, exe: String) -> Result<(), String> {
    let mut cfg = shared.config.lock().unwrap();
    let mut list: Vec<Value> = cfg
        .get("blacklist_exe")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    let wanted = normalize_exe(&exe);
    let already = list
        .iter()
        .any(|v| v.as_str().map(normalize_exe).as_deref() == Some(wanted.as_str()));
    if !already {
        list.push(json!(exe));
    }

    cfg.update(Map::from_iter([(
        "blacklist_exe".into(),
        Value::Array(list),
    )]))
    .map_err(|e| e.to_string())?;
    let settings = cfg.data().clone();
    drop(cfg);

    shared.send(Cmd::Reload);
    shared.emit("settings-changed", settings);
    Ok(())
}

#[tauri::command(async)]
pub fn list_windows() -> Vec<winapi::WindowInfo> {
    winapi::list_top_level_windows()
        .into_iter()
        .filter(|w| !BUILTIN_BLACKLIST.contains(&w.exe().to_lowercase().as_str()))
        .collect()
}

#[tauri::command(async)]
pub fn open_url(url: String) -> Result<(), String> {
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return Err("only web links can be opened".into());
    }
    shell::open(&url)
}

#[tauri::command]
pub async fn test_discord(client_id: String) -> Result<(), String> {
    if client_id.trim().is_empty() || !client_id.trim().chars().all(|c| c.is_ascii_digit()) {
        return Err("An Application ID is a long number.".into());
    }
    blocking(move || discord::test_connection(client_id.trim())).await
}

#[tauri::command]
pub async fn check_vndb_token(shared: SharedState<'_>, token: String) -> Result<String, String> {
    let shared = Arc::clone(&shared);
    blocking(move || {
        let user = shared.vndb.list_user(token.trim())?;
        Ok(user["username"].as_str().unwrap_or("").to_string())
    })
    .await
}

#[tauri::command(async)]
pub fn take_screenshot(shared: SharedState) {
    let shared = Arc::clone(&shared);
    std::thread::spawn(move || screenshots::take(&shared));
}

#[tauri::command(async)]
pub fn play_shutter(shared: SharedState, volume: i64) {
    crate::sound::play_shutter(&shared.data_dir.join("cache"), volume);
}

#[tauri::command]
pub fn quit(shared: SharedState, app: AppHandle) {
    quit_app(&app, &shared);
}

pub fn quit_app(app: &AppHandle, shared: &Shared) {
    crate::mascot::exiting();
    shared.send(Cmd::Quit);
    std::thread::sleep(std::time::Duration::from_millis(400));
    app.exit(0);
}

#[tauri::command(async)]
pub fn set_background(shared: SharedState, path: Option<String>) -> Result<(), String> {
    let dir = shared.data_dir.join("backgrounds");
    let stored = match path {
        Some(path) => {
            let source = std::path::Path::new(&path);
            let ext = source
                .extension()
                .map(|e| e.to_string_lossy().to_lowercase())
                .filter(|ext| ["png", "jpg", "jpeg", "webp", "gif", "bmp"].contains(&ext.as_str()))
                .ok_or("Use a PNG, JPG, WebP, GIF or BMP image.")?;
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
            let stamp = chrono::Utc::now().timestamp_millis();
            let target = dir.join(format!("background_{stamp}.{ext}"));
            std::fs::copy(source, &target).map_err(|e| e.to_string())?;
            target.to_string_lossy().to_string()
        }
        None => {
            let _ = std::fs::remove_dir_all(&dir);
            String::new()
        }
    };

    let settings = {
        let mut cfg = shared.config.lock().unwrap();
        cfg.update(Map::from_iter([("background_image".into(), json!(stored))]))
            .map_err(|e| e.to_string())?;
        cfg.data().clone()
    };
    shared.emit("settings-changed", settings);
    Ok(())
}

fn last_read(shared: &Shared) -> Option<crate::state::Snapshot> {
    let (key, entry, cover, playtime) = {
        let cfg = shared.config.lock().unwrap();
        let last = |e: &Map<String, Value>| e.get("last_played").and_then(Value::as_i64).unwrap_or(0);
        let (key, entry) = super::library::visible_games(&cfg)
            .into_iter()
            .filter(|(_, e)| last(e) > 0)
            .max_by_key(|(_, e)| last(e))?;
        let cover = cfg.cached_cover(&entry).map(|p| p.to_string_lossy().to_string());
        let playtime = cfg.playtime(&key);
        (key, entry, cover, playtime)
    };
    let text = |f: &str| entry.get(f).and_then(Value::as_str).unwrap_or("").to_string();
    let vn = [text("vndb_id"), text("matched_vndb_id")]
        .into_iter()
        .find(|id| !id.is_empty())
        .and_then(|id| shared.vndb.get(&id).ok().flatten());
    let section = Some(text("section"))
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "Chapter 2".into());

    Some(crate::state::Snapshot {
        detected: true,
        game_name: screenshots::display_name(&key, &entry),
        key,
        section_label: section,
        vn,
        playtime_seconds: playtime,
        cover: crate::state::Cover {
            local_path: cover,
            ..Default::default()
        },
        ..Default::default()
    })
}

fn preview(shared: &Shared, custom: &Value) -> Value {
    let now = crate::config::now();
    let current = shared.snapshot();
    let (source, snap) = if current.detected {
        ("live", Some(current))
    } else {
        match last_read(shared) {
            Some(snap) => ("last", Some(snap)),
            None => ("sample", None),
        }
    };

    let vars = {
        let cfg = shared.config.lock().unwrap();
        let language = presence::language(&cfg);
        match &snap {
            Some(snap) => presence::vars(snap, &cfg.game(&snap.key), cfg.str("title_language") == "original", language),
            None => presence::sample_vars(language),
        }
    };

    let title = vars[0].1.clone();
    let mut activity = discord::Activity {
        name: title.clone(),
        large_text: title,
        ..Default::default()
    };
    let (start, playtime) = match (source, &snap) {
        ("live", Some(snap)) => (snap.session_start, snap.playtime_seconds),
        (_, Some(snap)) => (now - 25 * 60, snap.playtime_seconds),
        _ => (now - 25 * 60, 35 * 3600 + 11 * 60),
    };
    presence::apply(&mut activity, custom, &vars, start, playtime, now);

    json!({
        "source": source,
        "activity": activity,
        "cover": snap.as_ref().and_then(|s| s.cover.local_path.clone()),
        "vars": vars.into_iter().map(|(k, v)| (k.to_string(), json!(v))).collect::<Map<String, Value>>(),
    })
}

#[tauri::command]
pub async fn preview_presence(shared: SharedState<'_>, custom: Value) -> Result<Value, String> {
    let shared = Arc::clone(&shared);
    blocking(move || Ok(preview(&shared, &custom))).await
}
