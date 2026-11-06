use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use serde::Serialize;
use tauri::{
    AppHandle, Emitter, Manager, PhysicalPosition, WebviewUrl, WebviewWindow,
    WebviewWindowBuilder,
};

use crate::capture::ScreenRect;
use crate::winapi;

pub const BROWSER_ARGS: &str =
    "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection --autoplay-policy=no-user-gesture-required --disable-background-timer-throttling --disable-renderer-backgrounding --disable-backgrounding-occluded-windows";

static CREATING: Mutex<()> = Mutex::new(());

fn focus_main(window: &WebviewWindow) {
    let _ = window.unminimize();
    let _ = window.show();
    let _ = window.set_focus();
}

pub fn show_main(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        focus_main(&window);
        return;
    }

    let app = app.clone();
    std::thread::spawn(move || {
        let _creating = CREATING.lock().unwrap();
        if let Some(window) = app.get_webview_window("main") {
            focus_main(&window);
            return;
        }
        let Some(conf) = app
            .config()
            .app
            .windows
            .iter()
            .find(|w| w.label == "main")
            .cloned()
        else {
            return;
        };
        match WebviewWindowBuilder::from_config(&app, &conf).and_then(|b| b.build()) {
            Ok(window) => {
                let _ = window.show();
                let _ = window.set_focus();
            }
            Err(e) => eprintln!("couldn't open the window: {e}"),
        }
    });
}

static PENDING_PAGE: Mutex<Option<String>> = Mutex::new(None);

pub fn open_page(app: &AppHandle, page: &str) {
    if app.get_webview_window("main").is_some() {
        let _ = app.emit_to("main", "navigate", page);
    } else {
        *PENDING_PAGE.lock().unwrap() = Some(page.to_string());
    }
    show_main(app);
}

pub fn take_pending_page() -> Option<String> {
    PENDING_PAGE.lock().unwrap().take()
}

fn popup(
    app: &AppHandle,
    label: &str,
    page: &str,
    width: f64,
    height: f64,
) -> Option<WebviewWindow> {
    let _creating = CREATING.lock().unwrap();
    if let Some(existing) = app.get_webview_window(label) {
        return Some(existing);
    }
    crate::crash::note(format!("create {label}"));

    let window = WebviewWindowBuilder::new(app, label, WebviewUrl::App(page.into()))
        .additional_browser_args(BROWSER_ARGS)
        .title("Shiori RPC")
        .inner_size(width, height)
        .decorations(false)
        .transparent(true)
        .shadow(false)
        .resizable(false)
        .skip_taskbar(true)
        .always_on_top(true)
        .focused(false)
        .focusable(false)
        .visible(false)
        .build()
        .ok()?;

    if let Ok(hwnd) = window.hwnd() {
        winapi::make_no_activate(hwnd.0 as isize);
    }
    Some(window)
}

fn close_later(app: &AppHandle, label: &'static str, generation: &'static AtomicU64, seconds: f64) {
    let mine = generation.fetch_add(1, Ordering::SeqCst) + 1;
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs_f64(seconds));
        if generation.load(Ordering::SeqCst) == mine {
            if let Some(window) = app.get_webview_window(label) {
                crate::crash::note(format!("hide {label}"));
                let _ = window.hide();
            }
        }
    });
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Toast {
    pub title: String,
    pub detail: String,
    pub image: Option<String>,
    pub ok: bool,
    pub action: String,
}

static TOAST: Mutex<Option<Toast>> = Mutex::new(None);
static TOAST_GENERATION: AtomicU64 = AtomicU64::new(0);

pub fn show_toast(app: &AppHandle, toast: Toast, area: Option<ScreenRect>) {
    *TOAST.lock().unwrap() = Some(toast.clone());

    let app = app.clone();
    std::thread::spawn(move || {
        let app = &app;
        let Some(window) = popup(app, "toast", "toast.html", 340.0, 76.0) else {
            return;
        };

        let (x, y) = match area {
            Some(area) => (area.left + 16, area.top + 16),
            None => {
                let (cx, cy) = winapi::cursor_pos().unwrap_or((0, 0));
                let (left, top, _, _) = winapi::work_area_at(cx, cy);
                (left + 16, top + 16)
            }
        };
        let _ = window.set_position(PhysicalPosition::new(x, y));
        let _ = window.emit("toast", &toast);
        let _ = window.show();

        let seconds = if toast.action.is_empty() { 2.8 } else { 7.0 };
        close_later(app, "toast", &TOAST_GENERATION, seconds);
    });
}

pub fn current_toast() -> Option<Toast> {
    TOAST.lock().unwrap().clone()
}

static BALLOON_TEXT: Mutex<String> = Mutex::new(String::new());
static BALLOON_GENERATION: AtomicU64 = AtomicU64::new(0);
static BUBBLE: Mutex<Option<(i32, i32, i32, i32)>> = Mutex::new(None);
static BUBBLE_WATCH: AtomicBool = AtomicBool::new(false);

pub fn show_balloon(app: &AppHandle, text: &str, seconds: f64) {
    *BALLOON_TEXT.lock().unwrap() = text.to_string();

    let app = app.clone();
    let text = text.to_string();
    std::thread::spawn(move || {
        let app = &app;
        let Some(window) = popup(app, "balloon", "balloon.html", 300.0, 220.0) else {
            return;
        };
        let _ = window.emit("say", &text);

        let mine = BALLOON_GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
        std::thread::sleep(Duration::from_secs_f64(seconds));
        if BALLOON_GENERATION.load(Ordering::SeqCst) == mine {
            hide_balloon(app);
        }
    });
}

pub fn balloon_text() -> String {
    BALLOON_TEXT.lock().unwrap().clone()
}

pub fn place_balloon(app: &AppHandle, width: u32, height: u32) -> &'static str {
    let Some(speaker) = crate::mascot::speaker() else {
        return "left";
    };
    let (Some(balloon), Some(mascot)) = (
        app.get_webview_window("balloon"),
        app.get_webview_window(&speaker),
    ) else {
        return "left";
    };
    let (Ok(pos), Ok(size), Ok(frame)) = (
        mascot.outer_position(),
        mascot.outer_size(),
        balloon.outer_size(),
    ) else {
        return "left";
    };

    let (bw, bh) = (width as i32, height as i32);
    let (cw, ch) = (size.width as i32, size.height as i32);
    let (left, top, right, _) = winapi::work_area_at(pos.x + cw / 2, pos.y + ch / 2);

    let mut x = pos.x - bw + cw / 6;
    let before = x >= left;
    if !before {
        x = pos.x + cw - cw / 6;
    }
    let x = x.clamp(left, (right - bw).max(left));
    let y = top.max(pos.y + ch / 10);

    let target = if before {
        PhysicalPosition::new(x + bw - frame.width as i32, y)
    } else {
        PhysicalPosition::new(x, y)
    };
    if balloon.outer_position().ok() != Some(target) {
        let _ = balloon.set_position(target);
    }
    if !balloon.is_visible().unwrap_or(false) {
        let _ = balloon.set_ignore_cursor_events(true);
        let _ = balloon.show();
    }
    *BUBBLE.lock().unwrap() = Some((x, y, bw, bh));
    watch_bubble(app);
    if before {
        "right"
    } else {
        "left"
    }
}

fn watch_bubble(app: &AppHandle) {
    if BUBBLE_WATCH.swap(true, Ordering::SeqCst) {
        return;
    }
    let app = app.clone();
    std::thread::spawn(move || {
        let mut ignoring = true;
        while let Some(window) = app.get_webview_window("balloon") {
            std::thread::sleep(Duration::from_millis(60));
            let over = match (*BUBBLE.lock().unwrap(), winapi::cursor_pos()) {
                (Some((x, y, w, h)), Some((cx, cy))) => cx >= x && cx < x + w && cy >= y && cy < y + h,
                _ => false,
            };
            if ignoring == over {
                ignoring = !over;
                let _ = window.set_ignore_cursor_events(ignoring);
            }
        }
        BUBBLE_WATCH.store(false, Ordering::SeqCst);
    });
}

pub fn hide_balloon(app: &AppHandle) {
    BALLOON_GENERATION.fetch_add(1, Ordering::SeqCst);
    *BUBBLE.lock().unwrap() = None;
    if app.get_webview_window("balloon").is_some() {
        crate::crash::note("hide balloon");
        let _ = app.emit_to("balloon", "hush", ());
    }
}
