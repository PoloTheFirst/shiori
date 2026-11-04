use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{json, Map, Value};

pub type Entry = Map<String, Value>;

pub const STATUSES: [&str; 4] = ["playing", "finished", "stalled", "dropped"];

fn defaults() -> Map<String, Value> {
    let v = json!({
        "discord_client_id": "1466261523889393892",
        "detection_mode": "auto",
        "manual_target": {"exe": "", "title_contains": ""},
        "allow_nsfw_covers": false,
        "use_steam_names": true,
        "show_elapsed": true,
        "show_section": true,
        "show_total_read": true,
        "show_progress": false,
        "title_language": "romanized",
        "presence_language": "app",
        "library_view": "list",
        "library_sort": "most",
        "screenshots_view": "right",
        "screenshots_order": "game",
        "background_image": "",
        "background_blur": 8,
        "background_dim": 55,
        "background_panels": 80,
        "labs_compact_sidebar": false,
        "reco_hidden": [],
        "reco_view": "details",
        "split_by_title": false,
        "clear_on_close": true,
        "idle_when_unfocused": false,
        "idle_seconds": 60,
        "update_min_interval": 5,
        "start_minimized": false,
        "theme": "system",
        "custom_theme": {
            "mode": "dark", "BG": "#111214", "SURFACE": "#1B1C20", "ACCENT": "#5865F2", "TEXT": "#F2F3F5",
            "overrides": {}
        },
        "default_asset_key": "vn_cover",
        "show_vndb_button": true,
        "title_rules": [],
        "blacklist_exe": ["osu!.exe", "Medal.exe", "Riot Client.exe"],
        "check_updates": true,
        "last_seen_version": "",
        "mascot_enabled": false,
        "mascot_image": "",
        "mascot_shown": [],
        "mascot_positions": {},
        "mascot_sizes": {},
        "mascot_names": {},
        "mascot_personalities": {},
        "mascot_custom_lines": {},
        "mascot_view_only": {},
        "mascot_click_hotkey": "",
        "mascot_chatter_minutes": 0,
        "mascot_voice_volume": 80,
        "language": "auto",
        "mascot_height": 420,
        "mascot_talk": true,
        "mascot_topmost": false,
        "mascot_pos": null,
        "vndb_token": "",
        "vndb_sync": false,
        "vndb_import_finished": false,
        "vndb_mark_playing": true,
        "screenshot_hotkey": "PrintScreen",
        "screenshot_volume": 30,
        "screenshot_dir": "",
        "locale_emulator_path": "",
        "ntlea_path": "",
    });
    match v {
        Value::Object(m) => m,
        _ => unreachable!(),
    }
}

pub fn game_key(exe: &str) -> String {
    let name = exe
        .rsplit(['\\', '/'])
        .next()
        .unwrap_or("")
        .trim()
        .to_lowercase();
    name.strip_suffix(".exe")
        .map(str::to_string)
        .unwrap_or(name)
}

fn norm_path(path: &str) -> String {
    path.replace('/', "\\").to_lowercase()
}

fn folder_name(path: &str) -> String {
    Path::new(path)
        .parent()
        .and_then(|p| p.file_name())
        .map(|s| s.to_string_lossy().trim().to_lowercase())
        .unwrap_or_default()
}

fn is_generic_folder(name: &str) -> bool {
    const GENERIC: [&str; 12] = [
        "", "game", "games", "bin", "x64", "x86", "win64", "win32", "release", "data", "app",
        "program",
    ];
    GENERIC.contains(&name)
}

fn vn_of(entry: &Entry) -> String {
    ["vndb_id", "matched_vndb_id"]
        .iter()
        .filter_map(|field| entry.get(*field).and_then(Value::as_str))
        .find(|id| !id.is_empty())
        .unwrap_or("")
        .to_string()
}

pub fn safe_filename(key: &str) -> String {
    let cleaned: String = key
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || "-_.".contains(c) {
                c
            } else {
                '_'
            }
        })
        .collect();
    if cleaned.is_empty() {
        "game".into()
    } else {
        cleaned
    }
}

pub fn today() -> String {
    chrono::Local::now().format("%Y-%m-%d").to_string()
}

pub fn this_hour() -> String {
    chrono::Local::now().format("%Y-%H").to_string()
}

fn add_to_counter(entry: &mut Map<String, Value>, counter: &str, slot: String, seconds: i64) {
    if seconds <= 0 {
        return;
    }
    if let Value::Object(map) = entry
        .entry(counter)
        .or_insert_with(|| Value::Object(Map::new()))
    {
        let before = map.get(&slot).and_then(Value::as_i64).unwrap_or(0);
        map.insert(slot, json!(before + seconds));
    }
}

pub fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

fn read_yaml(path: &Path) -> Option<Map<String, Value>> {
    let text = fs::read_to_string(path).ok()?;
    match serde_yaml::from_str::<Value>(&text).ok()? {
        Value::Object(m) => Some(m),
        _ => None,
    }
}

fn write_yaml(path: &Path, data: &Map<String, Value>) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let text = serde_yaml::to_string(data).map_err(std::io::Error::other)?;
    let tmp = path.with_extension("yaml.tmp");
    fs::write(&tmp, text)?;
    fs::rename(&tmp, path)
}

fn copy_dir(from: &Path, to: &Path) -> std::io::Result<()> {
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)?.flatten() {
        let dest = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir(&entry.path(), &dest)?;
        } else if !dest.exists() {
            fs::copy(entry.path(), dest)?;
        }
    }
    Ok(())
}

fn rebase(value: &mut Value, old: &str, new: &str) {
    match value {
        Value::String(text) if text.starts_with(old) => *text = text.replacen(old, new, 1),
        Value::Array(items) => items.iter_mut().for_each(|item| rebase(item, old, new)),
        Value::Object(fields) => fields.values_mut().for_each(|item| rebase(item, old, new)),
        _ => {}
    }
}

pub struct Config {
    pub dir: PathBuf,
    data: Map<String, Value>,
    games: BTreeMap<String, Entry>,
    filenames: HashMap<String, String>,
    frac: HashMap<String, f64>,
}

impl Config {
    pub fn games_dir(&self) -> PathBuf {
        self.dir.join("games")
    }

    pub fn covers_dir(&self) -> PathBuf {
        self.dir.join("cache").join("covers")
    }

    pub fn import_from(dir: &Path, source: &Path) -> bool {
        if dir.join("config.yaml").exists() || !source.join("config.yaml").exists() {
            return false;
        }
        let _ = fs::create_dir_all(dir);
        let _ = fs::copy(source.join("config.yaml"), dir.join("config.yaml"));
        let _ = copy_dir(&source.join("games"), &dir.join("games"));
        let _ = copy_dir(
            &source.join("cache").join("covers"),
            &dir.join("cache").join("covers"),
        );
        let old = source.to_string_lossy().to_string();
        let new = dir.to_string_lossy().to_string();
        for entry in fs::read_dir(dir.join("games"))
            .into_iter()
            .flatten()
            .flatten()
        {
            if let Some(mut game) = read_yaml(&entry.path()) {
                if let Some(Value::String(v)) = game.get("cover_value") {
                    if v.starts_with(&old) {
                        let fixed = v.replacen(&old, &new, 1);
                        game.insert("cover_value".into(), Value::String(fixed));
                        let _ = write_yaml(&entry.path(), &game);
                    }
                }
            }
        }
        true
    }

    pub fn migrate_from(dir: &Path, source: &Path) -> bool {
        if dir.join("config.yaml").exists() || !source.join("config.yaml").exists() {
            return false;
        }
        let _ = fs::create_dir_all(dir);
        for item in fs::read_dir(source).into_iter().flatten().flatten() {
            let name = item.file_name();
            let to = dir.join(&name);
            if ["config.yaml", "crash.dmp", "crash.log", "events.log"]
                .iter()
                .any(|skip| name == *skip)
            {
                continue;
            }
            if item.path().is_dir() {
                let _ = copy_dir(&item.path(), &to);
            } else {
                let _ = fs::copy(item.path(), &to);
            }
        }

        let old = source.to_string_lossy().to_string();
        let new = dir.to_string_lossy().to_string();
        for entry in fs::read_dir(dir.join("games"))
            .into_iter()
            .flatten()
            .flatten()
        {
            if let Some(mut game) = read_yaml(&entry.path()) {
                game.values_mut().for_each(|value| rebase(value, &old, &new));
                let _ = write_yaml(&entry.path(), &game);
            }
        }
        let Some(mut settings) = read_yaml(&source.join("config.yaml")) else {
            return false;
        };
        settings.values_mut().for_each(|value| rebase(value, &old, &new));
        write_yaml(&dir.join("config.yaml"), &settings).is_ok()
    }

    pub fn load(dir: &Path) -> Config {
        let _ = fs::create_dir_all(dir.join("games"));
        let _ = fs::create_dir_all(dir.join("cache").join("covers"));
        let _ = fs::create_dir_all(dir.join("cache").join("vndb"));
        let mut data = defaults();
        for (k, v) in read_yaml(&dir.join("config.yaml")).unwrap_or_default() {
            match (data.get_mut(&k), v) {
                (Some(Value::Object(base)), Value::Object(over)) => base.extend(over),
                (_, v) => {
                    data.insert(k, v);
                }
            }
        }
        let mut games = BTreeMap::new();
        let mut filenames = HashMap::new();
        let mut paths: Vec<_> = fs::read_dir(dir.join("games"))
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
            .collect();
        paths.sort();
        for path in paths {
            if path.extension().and_then(|e| e.to_str()) != Some("yaml") {
                continue;
            }
            let Some(mut entry) = read_yaml(&path) else {
                continue;
            };
            let stem = path
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            let key = match entry.remove("_key") {
                Some(Value::String(k)) if !k.is_empty() => k,
                _ => stem.clone(),
            };
            games.insert(key.clone(), entry);
            filenames.insert(key, stem);
        }
        let cfg = Config {
            dir: dir.to_path_buf(),
            data,
            games,
            filenames,
            frac: HashMap::new(),
        };
        if !dir.join("config.yaml").exists() {
            let _ = cfg.save();
        }
        cfg
    }

    pub fn save(&self) -> std::io::Result<()> {
        write_yaml(&self.dir.join("config.yaml"), &self.data)
    }

    pub fn data(&self) -> &Map<String, Value> {
        &self.data
    }

    pub fn get(&self, key: &str) -> Option<&Value> {
        self.data.get(key)
    }

    pub fn str(&self, key: &str) -> String {
        self.data
            .get(key)
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string()
    }

    pub fn bool(&self, key: &str) -> bool {
        self.data.get(key).and_then(Value::as_bool).unwrap_or(false)
    }

    pub fn num(&self, key: &str) -> f64 {
        self.data.get(key).and_then(Value::as_f64).unwrap_or(0.0)
    }

    pub fn update(&mut self, changes: Map<String, Value>) -> std::io::Result<()> {
        self.data.extend(changes);
        self.save()
    }

    fn filename_for(&self, key: &str, entry: &Entry) -> String {
        let title = entry
            .get("title")
            .or_else(|| entry.get("name"))
            .and_then(Value::as_str)
            .map(str::trim)
            .unwrap_or("");
        let base = safe_filename(if title.is_empty() { key } else { title });
        let used: Vec<String> = self
            .filenames
            .iter()
            .filter(|(k, _)| k.as_str() != key)
            .map(|(_, v)| v.to_lowercase())
            .collect();
        let mut candidate = base.clone();
        let mut n = 2;
        while used.contains(&candidate.to_lowercase()) {
            candidate = format!("{base}_{n}");
            n += 1;
        }
        candidate
    }

    fn save_game(&mut self, key: &str) {
        let Some(entry) = self.games.get(key) else {
            return;
        };
        let stem = self.filename_for(key, entry);
        if let Some(old) = self.filenames.get(key) {
            if old.to_lowercase() != stem.to_lowercase() {
                let _ = fs::remove_file(self.games_dir().join(format!("{old}.yaml")));
            }
        }
        let mut to_write = entry.clone();
        to_write.insert("_key".into(), Value::String(key.to_string()));
        let _ = write_yaml(&self.games_dir().join(format!("{stem}.yaml")), &to_write);
        self.filenames.insert(key.to_string(), stem);
    }

    pub fn key_for(&self, exe: &str, exe_path: &str) -> String {
        let stem = game_key(if exe.is_empty() { exe_path } else { exe });
        if exe_path.is_empty() {
            return stem;
        }
        let wanted = norm_path(exe_path);
        let folder = folder_name(exe_path);
        let path_of = |e: &Entry| {
            e.get("path")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string()
        };
        if let Some((key, _)) = self
            .games
            .iter()
            .find(|(_, e)| !path_of(e).is_empty() && norm_path(&path_of(e)) == wanted)
        {
            return key.clone();
        }
        for (key, entry) in &self.games {
            if game_key(key.split('@').next().unwrap_or("")) != stem {
                continue;
            }
            let saved = path_of(entry);
            if saved.is_empty() {
                return key.clone();
            }
            if folder_name(&saved) == folder
                && (!is_generic_folder(&folder) || !Path::new(&saved).exists())
            {
                return key.clone();
            }
        }
        if !self.games.contains_key(&stem) {
            return stem;
        }
        let base = format!(
            "{stem}@{}",
            if folder.is_empty() {
                "game".into()
            } else {
                safe_filename(&folder)
            }
        );
        let (mut candidate, mut n) = (base.clone(), 2);
        while self.games.contains_key(&candidate) {
            candidate = format!("{base}_{n}");
            n += 1;
        }
        candidate
    }

    pub fn game(&self, key: &str) -> Entry {
        self.games.get(key).cloned().unwrap_or_default()
    }

    pub fn all_games(&self) -> &BTreeMap<String, Entry> {
        &self.games
    }

    pub fn known_game_paths(&self) -> Vec<String> {
        self.games
            .values()
            .filter_map(|e| e.get("path").and_then(Value::as_str))
            .filter(|p| !p.is_empty())
            .map(norm_path)
            .collect()
    }

    pub fn set_game(&mut self, key: &str, fields: Map<String, Value>) {
        let entry = self.games.entry(key.to_string()).or_default();
        if fields.get("status").and_then(Value::as_str) == Some("finished")
            && entry.get("status").and_then(Value::as_str) != Some("finished")
        {
            entry.insert("finished_at".into(), json!(now()));
        }
        for (k, v) in fields {
            if !v.is_null() {
                entry.insert(k, v);
            }
        }
        self.save_game(key);
    }

    pub fn replace_all(
        &mut self,
        data: Map<String, Value>,
        games: Vec<(String, Entry)>,
    ) -> std::io::Result<()> {
        for entry in fs::read_dir(self.games_dir())?.flatten() {
            if entry.path().extension().and_then(|e| e.to_str()) == Some("yaml") {
                fs::remove_file(entry.path())?;
            }
        }

        self.data = defaults();
        self.data.extend(data);
        self.save()?;

        self.games.clear();
        self.filenames.clear();
        self.frac.clear();
        for (key, entry) in games {
            self.games.insert(key.clone(), entry);
            self.save_game(&key);
        }
        Ok(())
    }

    pub fn remove_game(&mut self, key: &str) {
        if self.games.remove(key).is_some() {
            self.frac.remove(key);
            let stem = self
                .filenames
                .remove(key)
                .unwrap_or_else(|| safe_filename(key));
            let _ = fs::remove_file(self.games_dir().join(format!("{stem}.yaml")));
        }
    }

    pub fn duplicates_of(&self, key: &str) -> Vec<String> {
        let Some(entry) = self.games.get(key) else {
            return vec![];
        };
        let path_of = |e: &Entry| e.get("path").and_then(Value::as_str).unwrap_or("").to_string();
        let stem_of = |k: &str| game_key(k.split('@').next().unwrap_or(""));
        let path = path_of(entry);
        let vn = vn_of(entry);
        let folder = folder_name(&path);
        let same_install = !path.is_empty() && crate::emulators::emulator_name(&path).is_none();

        self.games
            .iter()
            .filter(|(other, _)| other.as_str() != key)
            .filter(|(other, e)| {
                let other_path = path_of(e);
                let same_vn = !vn.is_empty() && vn_of(e) == vn;
                let same_path = same_install && norm_path(&other_path) == norm_path(&path);
                let same_folder = same_install
                    && !other_path.is_empty()
                    && stem_of(other) == stem_of(key)
                    && folder_name(&other_path) == folder
                    && !is_generic_folder(&folder);
                same_vn || same_path || same_folder
            })
            .map(|(other, _)| other.clone())
            .collect()
    }

    pub fn merge_games(&mut self, from: &str, into: &str) {
        let Some(source) = self.games.get(from).cloned() else {
            return;
        };
        let mut target = self.game(into);
        let num = |e: &Entry, field: &str| e.get(field).and_then(Value::as_i64).unwrap_or(0);
        let blank = |v: Option<&Value>| match v {
            None | Some(Value::Null) => true,
            Some(Value::String(s)) => s.is_empty(),
            _ => false,
        };

        for (field, value) in &source {
            if blank(target.get(field)) && field != "daily" && field != "hourly" {
                target.insert(field.clone(), value.clone());
            }
        }
        if num(&source, "last_played") > num(&target, "last_played") {
            if let Some(path) = source.get("path").filter(|p| !blank(Some(p))) {
                target.insert("path".into(), path.clone());
            }
        }
        for field in ["playtime_seconds", "sessions"] {
            target.insert(field.into(), json!(num(&target, field) + num(&source, field)));
        }
        target.insert(
            "last_played".into(),
            json!(num(&target, "last_played").max(num(&source, "last_played"))),
        );

        for counter in ["daily", "hourly"] {
            for (slot, seconds) in source.get(counter).and_then(Value::as_object).into_iter().flatten() {
                add_to_counter(&mut target, counter, slot.clone(), seconds.as_i64().unwrap_or(0));
            }
        }

        let frac = self.frac.get(from).copied().unwrap_or(0.0);
        *self.frac.entry(into.to_string()).or_insert(0.0) += frac;
        self.games.insert(into.to_string(), target);
        self.save_game(into);
        self.remove_game(from);
    }

    pub fn playtime(&self, key: &str) -> i64 {
        self.games
            .get(key)
            .and_then(|e| e.get("playtime_seconds"))
            .and_then(Value::as_i64)
            .unwrap_or(0)
    }

    pub fn add_playtime(&mut self, key: &str, seconds: f64) {
        if key.is_empty() || seconds <= 0.0 {
            return;
        }
        let total = self.frac.get(key).copied().unwrap_or(0.0) + seconds;
        let whole = total.floor();
        self.frac.insert(key.to_string(), total - whole);
        let whole = whole as i64;
        let entry = self.games.entry(key.to_string()).or_default();
        let current = entry
            .get("playtime_seconds")
            .and_then(Value::as_i64)
            .unwrap_or(0);
        entry.insert("playtime_seconds".into(), json!(current + whole));
        if whole > 0 {
            add_to_counter(entry, "daily", today(), whole);
            add_to_counter(entry, "hourly", this_hour(), whole);
        }
        entry.insert("last_played".into(), json!(now()));
        self.save_game(key);
    }

    pub fn start_session(&mut self, key: &str) {
        let entry = self.games.entry(key.to_string()).or_default();
        let sessions = entry.get("sessions").and_then(Value::as_i64).unwrap_or(0);
        entry.insert("sessions".into(), json!(sessions + 1));
        entry.insert("last_played".into(), json!(now()));
        self.save_game(key);
    }

    pub fn set_playtime(&mut self, key: &str, seconds: i64) {
        let entry = self.games.entry(key.to_string()).or_default();
        entry.insert("playtime_seconds".into(), json!(seconds.max(0)));
        self.frac.remove(key);
        self.save_game(key);
    }

    pub fn cached_cover(&self, entry: &Entry) -> Option<PathBuf> {
        let s = |k: &str| {
            entry
                .get(k)
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string()
        };
        let (source, value) = (s("cover_source"), s("cover_value"));
        if source == "local" && !value.is_empty() && Path::new(&value).is_file() {
            return Some(PathBuf::from(value));
        }
        let mut stems = Vec::new();
        if source == "url" && !value.is_empty() {
            stems.push(url_cache_key(&value));
        }
        let vn_id = if s("vndb_id").is_empty() {
            s("matched_vndb_id")
        } else {
            s("vndb_id")
        };
        if !vn_id.is_empty() {
            stems.push(vn_id);
        }
        let files: Vec<PathBuf> = fs::read_dir(self.covers_dir())
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
            .collect();
        stems.iter().find_map(|stem| {
            files
                .iter()
                .find(|p| {
                    p.file_stem()
                        .map(|f| f.to_string_lossy() == stem.as_str())
                        .unwrap_or(false)
                })
                .cloned()
        })
    }
}

pub fn url_cache_key(url: &str) -> String {
    format!(
        "url_{}",
        &sha1_smol::Sha1::from(url).digest().to_string()[..12]
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn library(name: &str) -> Config {
        let dir = std::env::temp_dir().join(format!("vnrpc-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        Config::load(&dir)
    }

    fn add(cfg: &mut Config, key: &str, fields: Value) {
        cfg.set_game(key, fields.as_object().cloned().unwrap_or_default());
    }

    #[test]
    fn moves_everything_to_the_new_folder_and_fixes_its_paths() {
        let root = std::env::temp_dir().join(format!("vnrpc-test-move-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let (old, new) = (root.join("old"), root.join("new"));
        let inside = |path: &str| old.join(path).to_string_lossy().to_string();
        fs::create_dir_all(old.join("games")).unwrap();
        fs::create_dir_all(old.join("backgrounds")).unwrap();
        fs::write(old.join("backgrounds").join("bg.png"), "picture").unwrap();
        fs::write(old.join("crash.log"), "old crash").unwrap();
        let settings = json!({
            "background_image": inside("backgrounds/bg.png"),
            "screenshot_dir": r"D:\Shots",
            "mascots": [{ "image": inside("mascots/a.png") }],
        });
        write_yaml(&old.join("config.yaml"), settings.as_object().unwrap()).unwrap();
        let game = json!({ "title": "Eustia", "cover_value": inside("covers/a.png") });
        write_yaml(&old.join("games").join("Eustia.yaml"), game.as_object().unwrap()).unwrap();

        assert!(Config::migrate_from(&new, &old));

        let moved = |path: &str| new.join(path).to_string_lossy().to_string();
        let settings = read_yaml(&new.join("config.yaml")).unwrap();
        assert_eq!(settings["background_image"], json!(moved("backgrounds/bg.png")));
        assert_eq!(settings["screenshot_dir"], json!(r"D:\Shots"));
        assert_eq!(settings["mascots"][0]["image"], json!(moved("mascots/a.png")));
        let game = read_yaml(&new.join("games").join("Eustia.yaml")).unwrap();
        assert_eq!(game["cover_value"], json!(moved("covers/a.png")));
        assert!(new.join("backgrounds").join("bg.png").exists());
        assert!(!new.join("crash.log").exists());
        assert!(old.join("config.yaml").exists());
        assert!(!Config::migrate_from(&new, &old));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_second_copy_of_a_game_reuses_its_entry() {
        let mut cfg = library("copy");
        add(&mut cfg, "bgi", json!({ "path": r"C:\vn\Eustia Full Edited\BGI.exe" }));
        add(&mut cfg, "bgi@subahibi", json!({ "path": r"C:\vn\Subahibi\BGI.exe" }));

        assert_eq!(cfg.key_for("BGI.exe", r"D:\other\Eustia Full Edited\BGI.exe"), "bgi");
        assert_ne!(cfg.key_for("BGI.exe", r"D:\other\Sakura no Uta\BGI.exe"), "bgi");
    }

    #[test]
    fn finds_duplicates_but_not_lookalikes() {
        let mut cfg = library("dupes");
        add(&mut cfg, "bgi", json!({ "path": r"C:\a\Eustia\BGI.exe", "vndb_id": "v3770" }));
        add(&mut cfg, "bgi@eustia_2", json!({ "path": r"D:\b\Eustia\BGI.exe" }));
        add(&mut cfg, "bgi@moved", json!({ "path": r"E:\Renamed\BGI.exe", "matched_vndb_id": "v3770" }));
        add(&mut cfg, "bgi@subahibi", json!({ "path": r"C:\a\Subahibi\BGI.exe", "vndb_id": "v3144" }));
        add(&mut cfg, "ryujinx@01001", json!({ "path": r"C:\emu\Ryujinx.exe" }));
        add(&mut cfg, "ryujinx@01002", json!({ "path": r"C:\emu\Ryujinx.exe" }));

        let mut found = cfg.duplicates_of("bgi");
        found.sort();
        assert_eq!(found, vec!["bgi@eustia_2", "bgi@moved"]);
        assert!(cfg.duplicates_of("ryujinx@01001").is_empty());
    }

    #[test]
    fn merging_adds_up_time_and_keeps_choices() {
        let mut cfg = library("merge");
        add(&mut cfg, "keep", json!({
            "title": "Aiyoku no Eustia", "playtime_seconds": 100, "sessions": 2,
            "last_played": 10, "daily": { "2026-10-01": 100 }, "path": "old.exe",
        }));
        add(&mut cfg, "drop", json!({
            "playtime_seconds": 50, "sessions": 1, "last_played": 20, "status": "playing",
            "daily": { "2026-10-01": 20, "2026-10-02": 30 }, "path": "new.exe",
            "hourly": { "2026-23": 50 },
        }));

        cfg.merge_games("drop", "keep");
        let merged = cfg.game("keep");
        assert!(cfg.all_games().get("drop").is_none());
        assert_eq!(merged["playtime_seconds"], 150);
        assert_eq!(merged["sessions"], 3);
        assert_eq!(merged["last_played"], 20);
        assert_eq!(merged["daily"], json!({ "2026-10-01": 120, "2026-10-02": 30 }));
        assert_eq!(merged["hourly"], json!({ "2026-23": 50 }));
        assert_eq!(merged["title"], "Aiyoku no Eustia");
        assert_eq!(merged["status"], "playing");
        assert_eq!(merged["path"], "new.exe");
    }

    #[test]
    fn reading_time_is_counted_per_hour() {
        let mut cfg = library("hourly");
        add(&mut cfg, "vn", json!({ "title": "Amatsutsumi" }));
        cfg.add_playtime("vn", 90.0);
        cfg.add_playtime("vn", 30.0);
        let entry = cfg.game("vn");
        assert_eq!(entry["hourly"][this_hour()], 120);
        assert_eq!(entry["daily"][today()], 120);
    }
}
