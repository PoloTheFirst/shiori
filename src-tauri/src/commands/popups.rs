use std::path::Path;

use tauri::{AppHandle, WebviewWindow};

use super::SharedState;
use crate::cutout;
use crate::mascot::{self, GalleryItem, MascotInfo};
use crate::mascot_pack;
use crate::ui_windows::{self, Toast};

#[tauri::command(async)]
pub fn mascot_info(window: WebviewWindow) -> Option<MascotInfo> {
    mascot::info(window.label())
}

#[tauri::command(async)]
pub fn mascot_alive(window: WebviewWindow) {
    mascot::alive(window.label());
}

#[tauri::command(async)]
pub fn mascot_poke(app: AppHandle, window: WebviewWindow) {
    mascot::poke(&app, window.label());
}

#[tauri::command(async)]
pub fn mascot_open_app(app: AppHandle) {
    ui_windows::show_main(&app);
}

#[tauri::command(async)]
pub fn mascot_menu(app: AppHandle, window: WebviewWindow) {
    mascot::show_menu(&app, window.label());
}

#[tauri::command(async)]
pub fn list_mascots(shared: SharedState) -> Vec<GalleryItem> {
    mascot::gallery(&shared)
}

#[tauri::command(async)]
pub fn add_mascot(app: AppHandle, shared: SharedState, path: String) -> Result<(), String> {
    let path = Path::new(&path);
    let id = if mascot_pack::is_pack(path) {
        mascot_pack::import(&shared, path)?
    } else {
        mascot::add_to_gallery(&shared, path)?
    };
    mascot::set_shown(&shared, &id, true);
    mascot::refresh(&app);
    Ok(())
}

#[tauri::command(async)]
pub fn export_mascot(shared: SharedState, id: String, path: String) -> Result<(), String> {
    mascot_pack::export(&shared, &id, Path::new(&path))
}

#[tauri::command]
pub async fn add_character_mascot(
    app: AppHandle,
    shared: SharedState<'_>,
    name: String,
    url: String,
) -> Result<bool, String> {
    let shared = std::sync::Arc::clone(&shared);
    super::blocking(move || {
        let (id, ai_failed) = mascot::add_character(&shared, &name, &url)?;
        mascot::set_shown(&shared, &id, true);
        mascot::refresh(&app);
        Ok(ai_failed)
    })
    .await
}

#[tauri::command]
pub fn cutout_ready(shared: SharedState) -> bool {
    cutout::ready(&shared)
}

#[tauri::command]
pub async fn download_cutout(shared: SharedState<'_>) -> Result<bool, String> {
    let shared = std::sync::Arc::clone(&shared);
    super::blocking(move || cutout::download(&shared)).await
}

#[tauri::command]
pub fn cancel_cutout_download() {
    cutout::cancel();
}

#[tauri::command(async)]
pub fn remove_cutout(shared: SharedState) -> Result<(), String> {
    cutout::remove(&shared)
}

#[tauri::command(async)]
pub fn remove_mascot(app: AppHandle, shared: SharedState, id: String) -> Result<(), String> {
    mascot::remove_from_gallery(&shared, &id)?;
    mascot::refresh(&app);
    Ok(())
}

#[tauri::command(async)]
pub fn rename_mascot(app: AppHandle, shared: SharedState, id: String, name: String) {
    mascot::set_name(&shared, &id, &name);
    mascot::refresh(&app);
}

#[tauri::command(async)]
pub fn ui_language(shared: SharedState) -> &'static str {
    crate::lang::resolve(&shared.config.lock().unwrap().str("language"))
}

#[tauri::command(async)]
pub fn languages() -> Vec<crate::lang::Language> {
    crate::lang::list()
}

#[tauri::command(async)]
pub fn language_strings(code: String) -> serde_json::Value {
    crate::lang::data(&code)["ui"].clone()
}

#[tauri::command(async)]
pub fn mascot_personalities(shared: SharedState) -> Vec<crate::personalities::Choice> {
    let language = crate::lang::resolve(&shared.config.lock().unwrap().str("language"));
    crate::personalities::choices(language)
}

#[tauri::command(async)]
pub fn set_mascot_personality(shared: SharedState, id: String, personality: String) {
    mascot::set_personality(&shared, &id, &personality);
}

#[tauri::command(async)]
pub fn personality_lines(shared: SharedState, personality: String) -> crate::personalities::Lines {
    let language = crate::lang::resolve(&shared.config.lock().unwrap().str("language"));
    crate::personalities::lines(&personality, language)
}

#[tauri::command(async)]
pub fn set_mascot_custom_lines(shared: SharedState, id: String, lines: serde_json::Value) {
    mascot::set_custom_lines(&shared, &id, lines);
}

#[tauri::command(async)]
pub fn set_mascot_view_only(shared: SharedState, id: String, view_only: bool) {
    mascot::set_view_only(&shared, &[id], view_only);
}

#[tauri::command(async)]
pub fn mascot_voices(shared: SharedState, id: String) -> serde_json::Map<String, serde_json::Value> {
    mascot::voices(&shared, &id)
}

#[tauri::command(async)]
pub fn add_mascot_voice(shared: SharedState, id: String, event: String, path: String) -> Result<(), String> {
    mascot::add_voice(&shared, &id, &event, Path::new(&path))
}

#[tauri::command(async)]
pub fn remove_mascot_voice(shared: SharedState, path: String) -> Result<(), String> {
    mascot::remove_voice(&shared, Path::new(&path))
}

#[tauri::command]
pub async fn set_mascot_size(app: AppHandle, id: String, height: u32) -> Result<(), String> {
    super::blocking(move || mascot::resize(&app, &id, height)).await
}

#[tauri::command(async)]
pub fn show_mascot(app: AppHandle, shared: SharedState, id: String, shown: bool) {
    mascot::set_shown(&shared, &id, shown);
    mascot::refresh(&app);
}

#[tauri::command(async)]
pub fn toast_action(app: AppHandle, action: String) {
    match action.as_str() {
        "check" => ui_windows::open_page(&app, "home"),
        "open" => ui_windows::show_main(&app),
        _ => {}
    }
}

#[tauri::command(async)]
pub fn balloon_text() -> String {
    ui_windows::balloon_text()
}

#[tauri::command(async)]
pub fn place_balloon(app: AppHandle, width: u32, height: u32) -> &'static str {
    ui_windows::place_balloon(&app, width, height)
}

#[tauri::command(async)]
pub fn hide_balloon(app: AppHandle) {
    ui_windows::hide_balloon(&app);
}

#[tauri::command(async)]
pub fn toast_content() -> Option<Toast> {
    ui_windows::current_toast()
}
