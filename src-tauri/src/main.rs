#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
#![recursion_limit = "256"]

mod config;
mod discord;
mod engine;
mod emulators;
mod engines;
mod presence;
mod state;
mod steam;
mod title_parser;
mod vndb;
mod vndb_list;

mod capture;
mod clipboard;
mod hotkey;
mod shell;
mod sound;
mod winapi;

mod autostart;
mod backup;
mod images;
mod launcher;
mod crash;
mod lang;
mod mascot;
mod personalities;
mod reco;
mod screenshots;
mod updater;

mod commands;
mod tray;
mod ui_windows;

use std::path::PathBuf;
use std::sync::Arc;

use tauri::{App, Manager, RunEvent, WindowEvent};

use config::Config;
use engine::Engine;
use state::Shared;

fn play_arg(args: impl IntoIterator<Item = String>) -> Option<String> {
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        if arg == "--play" {
            return args.next();
        }
    }
    None
}

fn play_from_shortcut(app: &tauri::AppHandle, key: &str) {
    let shared = Arc::clone(&app.state::<Arc<Shared>>());
    if !commands::library::play_from_shortcut(&shared, key) {
        ui_windows::open_page(app, "library");
        shared.emit(
            "toast",
            serde_json::json!({ "title": "Couldn't start the game from its shortcut", "ok": false }),
        );
    }
}

fn data_dir_override() -> Option<PathBuf> {
    std::env::var_os("VNRPC_DATA_DIR").map(PathBuf::from)
}

fn setup(app: &mut App) -> Result<(), Box<dyn std::error::Error>> {
    let data_dir = match data_dir_override() {
        Some(dir) => dir,
        None => {
            let data_dir = app.path().app_data_dir()?;
            import_old_data(&data_dir);
            data_dir
        }
    };
    app.asset_protocol_scope()
        .allow_directory(&data_dir, true)?;
    crash::install(data_dir.clone(), app.package_info().version.to_string());
    crash::started();

    let cfg = Config::load(&data_dir);
    let client_id = cfg.str("discord_client_id");
    let interval = cfg.num("update_min_interval");
    let play = play_arg(std::env::args());
    let start_minimized = cfg.bool("start_minimized")
        || play.is_some()
        || std::env::args().any(|a| a == "--minimized");

    let (commands, command_queue) = std::sync::mpsc::channel();
    let shared = Arc::new(Shared::new(app.handle().clone(), data_dir, cfg, commands));
    app.manage(Arc::clone(&shared));
    commands::app::allow_screenshot_folder(&shared);

    let (discord_tx, discord_rx) = discord::channel();
    let for_status = Arc::clone(&shared);
    std::thread::Builder::new()
        .name("discord".into())
        .spawn(move || {
            let on_status = Box::new(move |ok: bool, message: String| {
                for_status.update_status(|st| {
                    st.discord_ok = ok;
                    st.discord_msg = message;
                });
            });
            discord::run(discord_rx, client_id, interval, on_status);
        })?;

    let engine = Engine::new(Arc::clone(&shared), discord_tx);
    std::thread::Builder::new()
        .name("engine".into())
        .spawn(move || engine.run(command_queue))?;

    let for_hotkey = Arc::clone(&shared);
    std::thread::Builder::new()
        .name("hotkey".into())
        .spawn(move || hotkey::run(for_hotkey))?;

    if shared.config.lock().unwrap().bool("vndb_import_finished") {
        let for_import = Arc::clone(&shared);
        std::thread::spawn(move || {
            let _ = vndb_list::import_finished(&for_import);
        });
    }

    if !cfg!(debug_assertions) {
        if data_dir_override().is_none() {
            autostart::refresh();
            updater::sync_installed_version();
        }
        updater::clean_leftovers();
    }
    tray::build(app.handle())?;
    mascot::apply_settings(app.handle());
    mascot::start_chatter(app.handle());
    mascot::start_watchdog(app.handle());

    if !start_minimized {
        ui_windows::show_main(app.handle());
    }
    if let Some(key) = play {
        play_from_shortcut(app.handle(), &key);
    }
    Ok(())
}

fn import_old_data(data_dir: &std::path::Path) {
    let Ok(appdata) = std::env::var("APPDATA") else {
        return;
    };
    let appdata = PathBuf::from(appdata);
    Config::migrate_from(data_dir, &appdata.join("moe.lenn.vnrpc-tauri"));
    let source = appdata.join("VisualNovelRPC");
    if source.is_dir() {
        Config::import_from(data_dir, &source);
    }
}

fn main() {
    updater::wait_for_previous();
    if updater::heal_name() {
        return;
    }
    shell::set_app_id();
    let mut builder = tauri::Builder::default();

    if data_dir_override().is_none() {
        builder = builder.plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            match play_arg(args) {
                Some(key) => play_from_shortcut(app, &key),
                None => ui_windows::show_main(app),
            }
        }));
    }

    builder
        .plugin(tauri_plugin_dialog::init())
        .setup(setup)
        .on_menu_event(tray::handle_menu)
        .on_window_event(|window, event| {
            if window.label().starts_with(mascot::WINDOW_PREFIX) && matches!(event, WindowEvent::Moved(_)) {
                mascot::moved(window.app_handle());
            }
            if window.label() == "main" && matches!(event, WindowEvent::CloseRequested { .. }) {
                tray::closed_once(window.app_handle());
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::app::get_state,
            commands::app::update_settings,
            commands::app::set_paused,
            commands::app::set_autostart,
            commands::app::set_background,
            commands::labs::vn_description,
            commands::labs::vn_characters,
            commands::app::not_a_vn,
            commands::app::list_windows,
            commands::app::open_url,
            commands::app::test_discord,
            commands::app::check_vndb_token,
            commands::app::take_screenshot,
            commands::app::play_shutter,
            commands::app::quit,
            commands::library::get_library,
            commands::library::get_game,
            commands::library::merge_games,
            commands::library::vn_length,
            commands::library::update_game,
            commands::library::set_playtime,
            commands::library::remove_game,
            commands::library::confirm_match,
            commands::library::match_checked,
            commands::library::import_vndb_finished,
            commands::library::recommendations,
            commands::library::set_game_path,
            commands::library::set_tool_path,
            commands::library::play_game,
            commands::library::create_shortcut,
            commands::library::get_vote,
            commands::library::set_vote,
            commands::library::get_wishlist,
            commands::covers::search_vndb,
            commands::covers::release_covers,
            commands::covers::set_game_vn,
            commands::covers::set_release_cover,
            commands::covers::set_cover_url,
            commands::covers::prepare_image,
            commands::screenshots::list_screenshots,
            commands::screenshots::screenshot_info,
            commands::screenshots::thumbnail,
            commands::screenshots::delete_screenshot,
            commands::screenshots::copy_image_file,
            commands::screenshots::open_path,
            commands::screenshots::show_in_folder,
            commands::screenshots::open_screenshot_folder,
            commands::share::share_games,
            commands::share::learn_cover_nsfw,
            commands::share::image_data_url,
            commands::share::copy_png,
            commands::share::save_png,
            commands::share::year_recap,
            commands::app::preview_presence,
            commands::data::backup_default_name,
            commands::data::export_data,
            commands::data::import_data,
            commands::data::backup_folder,
            commands::data::check_update,
            commands::data::install_update,
            commands::data::release_notes,
            commands::popups::mascot_info,
            commands::popups::mascot_poke,
            commands::popups::mascot_open_app,
            commands::popups::mascot_menu,
            commands::popups::list_mascots,
            commands::popups::add_mascot,
            commands::popups::remove_mascot,
            commands::popups::show_mascot,
            commands::popups::set_mascot_size,
            commands::popups::mascot_personalities,
            commands::popups::ui_language,
            commands::popups::languages,
            commands::popups::language_strings,
            commands::popups::set_mascot_personality,
            commands::popups::personality_lines,
            commands::popups::set_mascot_custom_lines,
            commands::popups::set_mascot_view_only,
            commands::popups::mascot_alive,
            commands::popups::mascot_voices,
            commands::popups::add_mascot_voice,
            commands::popups::remove_mascot_voice,
            commands::popups::balloon_text,
            commands::popups::toast_action,
            commands::popups::place_balloon,
            commands::popups::hide_balloon,
            commands::popups::toast_content,
        ])
        .build(tauri::generate_context!())
        .expect("error while building the app")
        .run(|_app, event| {
            if let RunEvent::WindowEvent {
                label,
                event: tauri::WindowEvent::Destroyed,
                ..
            } = &event
            {
                crash::note(format!("destroyed {label}"));
            }
            if let RunEvent::ExitRequested { code: Some(_), .. } = &event {
                mascot::exiting();
            }
            if let RunEvent::Exit = event {
                mascot::exiting();
                crash::event("exit");
            }
            if let RunEvent::ExitRequested {
                api, code: None, ..
            } = event
            {
                api.prevent_exit();
            }
        });
}
