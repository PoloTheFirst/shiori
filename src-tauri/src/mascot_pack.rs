use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::Path;

use serde_json::{json, Map, Value};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use crate::config::safe_filename;
use crate::mascot::{self, AUDIO_EXTS, BUILT_IN, BUILT_IN_ID, IMAGE_EXTS, VOICE_EVENTS};
use crate::personalities;
use crate::state::Shared;

pub const EXTENSION: &str = "shiorimascot";
const FORMAT: i64 = 1;
const MANIFEST: &str = "mascot.json";
const NOT_A_PACK: &str = "That file isn't a Shiori mascot.";

const MAX_MANIFEST: u64 = 256 * 1024;
const MAX_IMAGE: u64 = 40 * 1024 * 1024;
const MAX_VOICE: u64 = 20 * 1024 * 1024;
const MAX_VOICES: usize = 100;
const MAX_LINE: usize = 300;
const MAX_LIST: usize = 20;

const TEXT_LINES: [&str; 6] = ["start", "stop", "idle", "back", "section", "screenshot"];
const LIST_LINES: [&str; 2] = ["pokes", "chatter"];

pub fn is_pack(path: &Path) -> bool {
    path.extension().is_some_and(|ext| ext.eq_ignore_ascii_case(EXTENSION))
}

pub fn export(shared: &Shared, id: &str, dest: &Path) -> Result<(), String> {
    let (image, ext) = if id == BUILT_IN_ID {
        (BUILT_IN.to_vec(), "png".to_string())
    } else {
        let file = mascot::file_for(shared, id).ok_or("That mascot's image is gone.")?;
        let ext = file.extension().unwrap_or_default().to_string_lossy().to_lowercase();
        (fs::read(&file).map_err(|e| e.to_string())?, ext)
    };
    let name = mascot::name_of(shared, id);
    let image_name = format!("image.{ext}");
    let manifest = json!({
        "format": FORMAT,
        "app_version": env!("CARGO_PKG_VERSION"),
        "name": name,
        "image": image_name,
        "height": mascot::height_for(shared, id),
        "personality": mascot::personality_of(shared, id),
        "lines": mascot::custom_lines_of(shared, id),
        "view_only": mascot::view_only_of(shared, id),
    });

    let part = dest.with_extension("part");
    let written = write_pack(shared, id, &part, &manifest, &image_name, &image);
    match written {
        Ok(()) => fs::rename(&part, dest).map_err(|e| e.to_string()),
        Err(error) => {
            let _ = fs::remove_file(&part);
            Err(error)
        }
    }
}

fn write_pack(
    shared: &Shared,
    id: &str,
    path: &Path,
    manifest: &Value,
    image_name: &str,
    image: &[u8],
) -> Result<(), String> {
    let mut zip = ZipWriter::new(File::create(path).map_err(|e| e.to_string())?);
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    let mut add = |name: &str, bytes: &[u8]| -> Result<(), String> {
        zip.start_file(name, options).map_err(|e| e.to_string())?;
        zip.write_all(bytes).map_err(|e| e.to_string())
    };

    add(MANIFEST, serde_json::to_string_pretty(manifest).unwrap().as_bytes())?;
    add(image_name, image)?;
    for (event, files) in mascot::voices(shared, id) {
        for file in files.as_array().into_iter().flatten().filter_map(Value::as_str) {
            let file = Path::new(file);
            let name = file.file_name().unwrap_or_default().to_string_lossy();
            add(&format!("voices/{event}/{name}"), &fs::read(file).map_err(|e| e.to_string())?)?;
        }
    }
    zip.finish().map_err(|e| e.to_string())?;
    Ok(())
}

pub fn import(shared: &Shared, source: &Path) -> Result<String, String> {
    let file = File::open(source).map_err(|e| e.to_string())?;
    let mut zip = ZipArchive::new(file).map_err(|_| NOT_A_PACK.to_string())?;
    let manifest: Value = serde_json::from_slice(&read_limited(&mut zip, MANIFEST, MAX_MANIFEST)?)
        .map_err(|_| NOT_A_PACK.to_string())?;
    match manifest["format"].as_i64() {
        Some(format) if format > FORMAT => {
            return Err("That mascot was made with a newer Shiori RPC. Update the app first.".into())
        }
        Some(_) => {}
        None => return Err(NOT_A_PACK.into()),
    }

    let image_name = manifest["image"].as_str().ok_or(NOT_A_PACK)?;
    let ext = image_name.rsplit_once('.').map(|(_, ext)| ext.to_lowercase()).unwrap_or_default();
    if !IMAGE_EXTS.contains(&ext.as_str()) {
        return Err(NOT_A_PACK.into());
    }
    let image = read_limited(&mut zip, image_name, MAX_IMAGE)?;
    image::load_from_memory(&image).map_err(|_| NOT_A_PACK.to_string())?;

    let name = clean_name(manifest["name"].as_str().unwrap_or_default());
    let staging = shared.data_dir.join("cache").join("mascot-pack");
    let _ = fs::remove_dir_all(&staging);
    fs::create_dir_all(&staging).map_err(|e| e.to_string())?;
    let imported = import_into(shared, &mut zip, &manifest, &name, &ext, &image, &staging);
    let _ = fs::remove_dir_all(&staging);
    imported
}

fn import_into(
    shared: &Shared,
    zip: &mut ZipArchive<File>,
    manifest: &Value,
    name: &str,
    ext: &str,
    image: &[u8],
    staging: &Path,
) -> Result<String, String> {
    let staged = staging.join(format!("{}.{ext}", safe_filename(&name.to_lowercase())));
    fs::write(&staged, image).map_err(|e| e.to_string())?;
    let id = mascot::add_to_gallery(shared, &staged)?;
    mascot::set_name(shared, &id, name);

    if let Some(height) = manifest["height"].as_u64() {
        mascot::set_height(shared, &id, height.min(u64::from(u32::MAX)) as u32);
    }
    let personality = manifest["personality"].as_str().unwrap_or(personalities::DEFAULT);
    if personalities::exists(personality) {
        mascot::set_personality(shared, &id, personality);
    }
    mascot::set_custom_lines(shared, &id, clean_lines(&manifest["lines"]));
    if manifest["view_only"].as_bool() == Some(true) {
        mascot::set_view_only(shared, &[id.clone()], true);
    }
    if let Err(error) = add_voices(shared, &id, zip, staging) {
        let _ = mascot::remove_from_gallery(shared, &id);
        return Err(error);
    }
    Ok(id)
}

fn add_voices(shared: &Shared, id: &str, zip: &mut ZipArchive<File>, staging: &Path) -> Result<(), String> {
    let wanted: Vec<(String, String, String)> = zip
        .file_names()
        .filter_map(|entry| voice_target(entry).map(|(event, file)| (entry.to_string(), event, file)))
        .take(MAX_VOICES)
        .collect();
    for (entry, event, file) in wanted {
        let bytes = read_limited(zip, &entry, MAX_VOICE)?;
        let staged = staging.join(&file);
        fs::write(&staged, bytes).map_err(|e| e.to_string())?;
        let added = mascot::add_voice(shared, id, &event, &staged);
        let _ = fs::remove_file(&staged);
        added?;
    }
    Ok(())
}

fn voice_target(entry: &str) -> Option<(String, String)> {
    let mut parts = entry.split('/');
    let (Some("voices"), Some(event), Some(file), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return None;
    };
    if !VOICE_EVENTS.contains(&event) {
        return None;
    }
    let (stem, ext) = file.rsplit_once('.')?;
    let ext = ext.to_lowercase();
    if stem.is_empty() || !AUDIO_EXTS.contains(&ext.as_str()) {
        return None;
    }
    Some((event.to_string(), format!("{}.{ext}", safe_filename(stem))))
}

fn read_limited(zip: &mut ZipArchive<File>, name: &str, limit: u64) -> Result<Vec<u8>, String> {
    let entry = zip.by_name(name).map_err(|_| NOT_A_PACK.to_string())?;
    let mut bytes = Vec::new();
    entry.take(limit + 1).read_to_end(&mut bytes).map_err(|_| NOT_A_PACK.to_string())?;
    if bytes.len() as u64 > limit {
        return Err("That mascot file is too big.".into());
    }
    Ok(bytes)
}

fn clean_name(name: &str) -> String {
    let name = mascot::clean_name(name);
    if name.is_empty() {
        "mascot".into()
    } else {
        name
    }
}

fn short(text: &str) -> String {
    text.chars().take(MAX_LINE).collect()
}

fn clean_lines(lines: &Value) -> Value {
    let mut clean = Map::new();
    for key in TEXT_LINES {
        if let Some(text) = lines[key].as_str() {
            clean.insert(key.into(), json!(short(text)));
        }
    }
    for key in LIST_LINES {
        if let Some(list) = lines[key].as_array() {
            let list: Vec<String> = list.iter().filter_map(Value::as_str).take(MAX_LIST).map(short).collect();
            clean.insert(key.into(), json!(list));
        }
    }
    Value::Object(clean)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_known_voice_paths_are_taken() {
        assert_eq!(voice_target("voices/poke/Hi there.MP3"), Some(("poke".into(), "Hi_there.mp3".into())));
        assert_eq!(voice_target("voices/poke/../../evil.mp3"), None);
        assert_eq!(voice_target("voices/../x/a.mp3"), None);
        assert_eq!(voice_target("voices/dance/a.mp3"), None);
        assert_eq!(voice_target("voices/poke/a.exe"), None);
        assert_eq!(voice_target("voices/poke/.mp3"), None);
        assert_eq!(voice_target("image.png"), None);
    }

    #[test]
    fn lines_keep_only_known_text() {
        let long = "a".repeat(1000);
        let lines = json!({
            "start": "Hi {name}",
            "idle": 5,
            "evil": "x",
            "pokes": ["one", 2, long],
            "chatter": "not a list",
        });
        let clean = clean_lines(&lines);
        assert_eq!(clean["start"], "Hi {name}");
        assert!(clean.get("idle").is_none());
        assert!(clean.get("evil").is_none());
        assert!(clean.get("chatter").is_none());
        assert_eq!(clean["pokes"].as_array().unwrap().len(), 2);
        assert_eq!(clean["pokes"][1].as_str().unwrap().len(), MAX_LINE);
    }

    #[test]
    fn names_are_trimmed_and_never_empty() {
        assert_eq!(clean_name("  Kud\n "), "Kud");
        assert_eq!(clean_name(""), "mascot");
        assert_eq!(clean_name(&"x".repeat(200)).len(), mascot::MAX_NAME);
    }

    #[test]
    fn packs_are_known_by_their_extension() {
        assert!(is_pack(Path::new("C:\\a\\Kud.SHIORIMASCOT")));
        assert!(!is_pack(Path::new("C:\\a\\kud.png")));
    }
}
