use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use image::imageops::FilterType;
use image::{DynamicImage, RgbaImage};
use serde::Serialize;
use serde_json::{json, Map, Value};
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::{
    AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, WebviewUrl, WebviewWindowBuilder,
};

use crate::config::safe_filename;
use crate::cutout;
use crate::lang;
use crate::personalities::{self, Lines};
use crate::state::{Shared, Snapshot};
use crate::ui_windows::{self, Toast};
use crate::winapi;

pub(crate) const BUILT_IN: &[u8] = include_bytes!("../assets/mascot.png");
pub(crate) const BUILT_IN_ID: &str = "default";
pub(crate) const IMAGE_EXTS: [&str; 3] = ["png", "webp", "gif"];
pub(crate) const AUDIO_EXTS: [&str; 6] = ["mp3", "wav", "ogg", "m4a", "opus", "flac"];
pub const VOICE_EVENTS: [&str; 8] =
    ["start", "stop", "idle", "back", "section", "screenshot", "poke", "chatter"];

pub const WINDOW_PREFIX: &str = "mascot-";
pub const HIDE_MENU_PREFIX: &str = "mascot-hide:";

struct Character {
    file: PathBuf,
    alpha: RgbaImage,
    view_only: bool,
    token: u64,
}

static CHARACTERS: Mutex<Option<HashMap<String, Character>>> = Mutex::new(None);
static SPEAKER: Mutex<Option<String>> = Mutex::new(None);

static GENERATION: AtomicU64 = AtomicU64::new(0);
static REQUESTS: AtomicU64 = AtomicU64::new(0);
static RESIZES: AtomicU64 = AtomicU64::new(0);
static APPLYING: Mutex<()> = Mutex::new(());
static LAST_SAID: AtomicU64 = AtomicU64::new(0);
static LAST_SEEN: Mutex<Option<HashMap<String, Instant>>> = Mutex::new(None);
static FORCE: Mutex<Vec<String>> = Mutex::new(Vec::new());
static EXITING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn exiting() {
    EXITING.store(true, Ordering::SeqCst);
}
const SILENT_LIMIT: Duration = Duration::from_secs(90);

const MIN_HEIGHT: u32 = 80;
const MAX_HEIGHT: u32 = 1500;
const BACKGROUND_TOLERANCE: u32 = 36;
const MIN_FLAT_BORDER: f64 = 0.6;
const MAX_CLEARED: f64 = 0.9;
const MAX_UPSCALE: u32 = 2;
const LEAK_RADIUS: u32 = 2;
const SMALL_POCKET: f64 = 0.006;
const EDGE_WIDTH: usize = 3;
const EDGE_CONTRAST: f64 = 170.0;

fn shared(app: &AppHandle) -> Arc<Shared> {
    Arc::clone(&app.state::<Arc<Shared>>())
}

fn with_characters<T>(f: impl FnOnce(&mut HashMap<String, Character>) -> T) -> T {
    let mut characters = CHARACTERS.lock().unwrap();
    f(characters.get_or_insert_with(HashMap::new))
}

fn id_of(label: &str) -> &str {
    label.strip_prefix(WINDOW_PREFIX).unwrap_or(label)
}

fn gallery_dir(shared: &Shared) -> PathBuf {
    shared.data_dir.join("mascots")
}

fn gallery_files(shared: &Shared) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(gallery_dir(shared))
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase());
            ext.is_some_and(|ext| IMAGE_EXTS.contains(&ext.as_str()))
        })
        .collect();
    files.sort();
    files
}

pub(crate) fn file_for(shared: &Shared, id: &str) -> Option<PathBuf> {
    gallery_files(shared)
        .into_iter()
        .find(|path| path.file_stem().is_some_and(|stem| stem.to_string_lossy() == id))
}

fn image_for(shared: &Shared, id: &str) -> Option<DynamicImage> {
    if id == BUILT_IN_ID {
        return image::load_from_memory(BUILT_IN).ok();
    }
    image::open(file_for(shared, id)?).ok()
}

fn fit(img: DynamicImage, height: u32) -> RgbaImage {
    let cropped = crop_transparent_margins(&img.to_rgba8());
    let height = height.clamp(MIN_HEIGHT, MAX_HEIGHT);
    let width =
        ((cropped.width() as f64 * height as f64 / cropped.height() as f64).round() as u32).max(1);
    image::imageops::resize(&cropped, width, height, FilterType::CatmullRom)
}

fn crop_transparent_margins(img: &RgbaImage) -> RgbaImage {
    let (mut left, mut top, mut right, mut bottom) = (img.width(), img.height(), 0, 0);
    for (x, y, px) in img.enumerate_pixels() {
        if px.0[3] > 0 {
            left = left.min(x);
            top = top.min(y);
            right = right.max(x);
            bottom = bottom.max(y);
        }
    }
    if right < left {
        return img.clone();
    }
    image::imageops::crop_imm(img, left, top, right - left + 1, bottom - top + 1).to_image()
}

fn shown_ids(shared: &Shared) -> Vec<String> {
    let ids: Vec<String> = shared
        .config
        .lock()
        .unwrap()
        .get("mascot_shown")
        .and_then(Value::as_array)
        .map(|list| list.iter().filter_map(Value::as_str).map(str::to_string).collect())
        .unwrap_or_default();
    if ids.is_empty() {
        vec![BUILT_IN_ID.to_string()]
    } else {
        ids
    }
}

fn save_settings(shared: &Shared, changes: Map<String, Value>) {
    let _ = shared.config.lock().unwrap().update(changes);
}

pub(crate) fn height_for(shared: &Shared, id: &str) -> u32 {
    let cfg = shared.config.lock().unwrap();
    let default = match cfg.num("mascot_height") as u32 {
        0 => 420,
        height => height,
    };
    cfg.get("mascot_sizes")
        .and_then(|sizes| sizes.get(id))
        .and_then(Value::as_u64)
        .map_or(default, |height| height as u32)
        .clamp(MIN_HEIGHT, MAX_HEIGHT)
}

#[derive(Serialize)]
pub struct GalleryItem {
    id: String,
    name: String,
    file: String,
    shown: bool,
    built_in: bool,
    height: u32,
    personality: String,
    custom: Value,
    view_only: bool,
}

pub fn gallery(shared: &Shared) -> Vec<GalleryItem> {
    let enabled = shared.config.lock().unwrap().bool("mascot_enabled");
    let shown = shown_ids(shared);
    let is_shown = |id: &str| enabled && shown.iter().any(|s| s == id);

    let mut items = vec![GalleryItem {
        id: BUILT_IN_ID.into(),
        name: name_of(shared, BUILT_IN_ID),
        file: String::new(),
        shown: is_shown(BUILT_IN_ID),
        built_in: true,
        height: height_for(shared, BUILT_IN_ID),
        personality: personality_of(shared, BUILT_IN_ID),
        custom: custom_lines_of(shared, BUILT_IN_ID),
        view_only: view_only_of(shared, BUILT_IN_ID),
    }];
    for path in gallery_files(shared) {
        let id = path.file_stem().unwrap_or_default().to_string_lossy().to_string();
        items.push(GalleryItem {
            name: name_of(shared, &id),
            file: path.to_string_lossy().to_string(),
            shown: is_shown(&id),
            built_in: false,
            height: height_for(shared, &id),
            personality: personality_of(shared, &id),
            custom: custom_lines_of(shared, &id),
            view_only: view_only_of(shared, &id),
            id,
        });
    }
    items
}

pub fn add_to_gallery(shared: &Shared, source: &Path) -> Result<String, String> {
    image::open(source).map_err(|_| "That file isn't an image.".to_string())?;
    let ext = source
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .filter(|ext| IMAGE_EXTS.contains(&ext.as_str()))
        .ok_or("Use a PNG, WebP or GIF image.")?;

    let stem = source.file_stem().unwrap_or_default().to_string_lossy().to_lowercase();
    let base = safe_filename(&stem);
    let (mut id, mut n) = (base.clone(), 2);
    while id == BUILT_IN_ID || file_for(shared, &id).is_some() {
        id = format!("{base}_{n}");
        n += 1;
    }

    let dir = gallery_dir(shared);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    std::fs::copy(source, dir.join(format!("{id}.{ext}"))).map_err(|e| e.to_string())?;
    Ok(id)
}

fn close_colors(a: &image::Rgba<u8>, b: &image::Rgba<u8>) -> bool {
    a.0.iter()
        .take(3)
        .zip(b.0.iter())
        .map(|(x, y)| x.abs_diff(*y) as u32)
        .sum::<u32>()
        <= BACKGROUND_TOLERANCE
}

fn clear_flat_background(mut img: RgbaImage) -> RgbaImage {
    let (width, height) = img.dimensions();
    if width < 2 || height < 2 {
        return img;
    }
    let background = *img.get_pixel(0, 0);
    let border: Vec<(u32, u32)> = (0..width)
        .flat_map(|x| [(x, 0), (x, height - 1)])
        .chain((0..height).flat_map(|y| [(0, y), (width - 1, y)]))
        .collect();
    let flat = border
        .iter()
        .filter(|(x, y)| close_colors(img.get_pixel(*x, *y), &background))
        .count();
    if (flat as f64) < border.len() as f64 * MIN_FLAT_BORDER {
        return img;
    }

    let open_background = background_mask(&img, &background);
    let cleared = open_background.iter().filter(|is_background| **is_background).count();
    if cleared as f64 > (width as u64 * height as u64) as f64 * MAX_CLEARED {
        return img;
    }
    for (index, is_background) in open_background.iter().enumerate() {
        if *is_background {
            let (x, y) = (index as u32 % width, index as u32 / width);
            img.get_pixel_mut(x, y).0[3] = 0;
        }
    }
    if cleared > 0 {
        soften_edges(&mut img, &background);
        spread_colors_outward(&mut img);
    }
    img
}

fn grow_mask(mask: &[bool], width: u32, height: u32, keep: impl Fn(usize) -> bool) -> Vec<bool> {
    let mut out = mask.to_vec();
    let at = |x: u32, y: u32| (y * width + x) as usize;
    for y in 0..height {
        for x in 0..width {
            if !mask[at(x, y)] {
                continue;
            }
            for ny in y.saturating_sub(LEAK_RADIUS)..=(y + LEAK_RADIUS).min(height - 1) {
                for nx in x.saturating_sub(LEAK_RADIUS)..=(x + LEAK_RADIUS).min(width - 1) {
                    if keep(at(nx, ny)) {
                        out[at(nx, ny)] = true;
                    }
                }
            }
        }
    }
    out
}

fn background_mask(img: &RgbaImage, background: &image::Rgba<u8>) -> Vec<bool> {
    let (width, height) = img.dimensions();
    let at = |x: u32, y: u32| (y * width + x) as usize;
    let looks_like_it: Vec<bool> = img.pixels().map(|pixel| close_colors(pixel, background)).collect();

    let not_it: Vec<bool> = looks_like_it.iter().map(|is_it| !is_it).collect();
    let near_a_line = grow_mask(&not_it, width, height, |_| true);
    let open_area: Vec<bool> = near_a_line.iter().map(|near| !near).collect();

    let mut reached = vec![false; open_area.len()];
    let mut pending: Vec<(u32, u32)> = (0..width)
        .map(|x| (x, 0))
        .chain((0..height).flat_map(|y| [(0, y), (width - 1, y)]))
        .collect();
    while let Some((x, y)) = pending.pop() {
        let index = at(x, y);
        if reached[index] || !open_area[index] {
            continue;
        }
        reached[index] = true;
        pending.extend(neighbours(x, y, width, height));
    }

    let mut mask = grow_mask(&reached, width, height, |index| looks_like_it[index]);
    let small_pocket = (f64::from(width) * f64::from(height) * SMALL_POCKET) as usize;
    let mut checked = mask.clone();
    for start_y in 0..height {
        for start_x in 0..width {
            let start = at(start_x, start_y);
            if checked[start] || !looks_like_it[start] {
                continue;
            }
            let mut pocket = Vec::new();
            let mut touches_background = false;
            let mut pending = vec![(start_x, start_y)];
            checked[start] = true;
            while let Some((x, y)) = pending.pop() {
                pocket.push(at(x, y));
                for (nx, ny) in neighbours(x, y, width, height) {
                    let index = at(nx, ny);
                    touches_background |= mask[index];
                    if !checked[index] && looks_like_it[index] {
                        checked[index] = true;
                        pending.push((nx, ny));
                    }
                }
            }
            if touches_background && pocket.len() < small_pocket {
                for index in pocket {
                    mask[index] = true;
                }
            }
        }
    }
    mask
}

pub(crate) fn neighbours(x: u32, y: u32, width: u32, height: u32) -> impl Iterator<Item = (u32, u32)> {
    [
        (x.wrapping_sub(1), y),
        (x + 1, y),
        (x, y.wrapping_sub(1)),
        (x, y + 1),
    ]
    .into_iter()
    .filter(move |(nx, ny)| *nx < width && *ny < height)
}

fn soften_edges(img: &mut RgbaImage, background: &image::Rgba<u8>) {
    let (width, height) = img.dimensions();
    let mut near_edge: Vec<(u32, u32)> = img
        .enumerate_pixels()
        .filter(|(_, _, pixel)| pixel.0[3] == 0)
        .map(|(x, y, _)| (x, y))
        .collect();
    let mut seen: std::collections::HashSet<(u32, u32)> = near_edge.iter().copied().collect();
    let mut outline: std::collections::HashSet<(u32, u32)> = std::collections::HashSet::new();

    for _ in 0..EDGE_WIDTH {
        let mut next = Vec::new();
        for (x, y) in near_edge {
            for neighbour in neighbours(x, y, width, height) {
                if seen.insert(neighbour) {
                    next.push(neighbour);
                }
            }
        }
        for (x, y) in &next {
            let pixel = img.get_pixel_mut(*x, *y);
            let contrast = pixel.0.iter().take(3).zip(background.0.iter()).map(|(a, b)| a.abs_diff(*b)).max().unwrap_or(0);
            let alpha = (f64::from(contrast) / EDGE_CONTRAST).min(1.0);
            if alpha >= 1.0 {
                outline.insert((*x, *y));
                continue;
            }
            for channel in 0..3 {
                let mixed = f64::from(pixel.0[channel]);
                let behind = f64::from(background.0[channel]);
                pixel.0[channel] = ((mixed - (1.0 - alpha) * behind) / alpha.max(0.05)).clamp(0.0, 255.0) as u8;
            }
            pixel.0[3] = (alpha * 255.0) as u8;
        }
        next.retain(|point| !outline.contains(point));
        near_edge = next;
    }
}

fn spread_colors_outward(img: &mut RgbaImage) {
    let (width, height) = img.dimensions();
    for _ in 0..EDGE_WIDTH + 1 {
        let before = img.clone();
        for (x, y, pixel) in img.enumerate_pixels_mut() {
            if pixel.0[3] > 0 {
                continue;
            }
            let colors: Vec<[u8; 4]> = neighbours(x, y, width, height)
                .map(|(nx, ny)| before.get_pixel(nx, ny).0)
                .filter(|color| color[3] > 0)
                .collect();
            if colors.is_empty() {
                continue;
            }
            for channel in 0..3 {
                let total: u32 = colors.iter().map(|color| u32::from(color[channel])).sum();
                pixel.0[channel] = (total / colors.len() as u32) as u8;
            }
            pixel.0[3] = 1;
        }
    }
    for pixel in img.pixels_mut() {
        if pixel.0[3] == 1 {
            pixel.0[3] = 0;
        }
    }
}

fn remove_background(shared: &Shared, picture: RgbaImage) -> (RgbaImage, bool) {
    if !cutout::ready(shared) {
        return (clear_flat_background(picture), false);
    }
    match cutout::cut(shared, &picture) {
        Ok(cut) => (cut, false),
        Err(error) => {
            eprintln!("the AI cutout failed: {error}");
            (clear_flat_background(picture), true)
        }
    }
}

pub fn add_character(shared: &Shared, name: &str, url: &str) -> Result<(String, bool), String> {
    if !url.starts_with("https://t.vndb.org/") {
        return Err("That image isn't from VNDB.".into());
    }
    let id = safe_filename(&name.to_lowercase());
    if file_for(shared, &id).is_some() {
        return Ok((id, false));
    }

    let bytes = shared.vndb.download(url)?;
    let picture = image::load_from_memory(&bytes).map_err(|_| "That file isn't an image.".to_string())?;
    let (picture, ai_failed) = remove_background(shared, picture.to_rgba8());
    let height = picture.height();

    let cache = shared.data_dir.join("cache");
    std::fs::create_dir_all(&cache).map_err(|e| e.to_string())?;
    let file = cache.join(format!("{id}.png"));
    picture.save(&file).map_err(|e| e.to_string())?;
    let added = add_to_gallery(shared, &file);
    let _ = std::fs::remove_file(&file);
    let id = added?;

    set_height(shared, &id, height_for(shared, &id).min(height * MAX_UPSCALE));
    Ok((id, ai_failed))
}

pub(crate) fn set_height(shared: &Shared, id: &str, height: u32) -> u32 {
    let height = height.clamp(MIN_HEIGHT, MAX_HEIGHT);
    let mut sizes = shared
        .config
        .lock()
        .unwrap()
        .get("mascot_sizes")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    sizes.insert(id.to_string(), json!(height));
    save_settings(shared, Map::from_iter([("mascot_sizes".into(), Value::Object(sizes))]));
    height
}

pub fn remove_from_gallery(shared: &Shared, id: &str) -> Result<(), String> {
    if id == BUILT_IN_ID {
        return Err("The default mascot can't be removed.".into());
    }
    set_shown(shared, id, false);
    if let Some(file) = file_for(shared, id) {
        std::fs::remove_file(file).map_err(|e| e.to_string())?;
    }
    let _ = std::fs::remove_dir_all(voices_dir(shared, id));
    set_name(shared, id, "");
    Ok(())
}

pub fn set_shown(shared: &Shared, id: &str, show: bool) {
    let enabled = shared.config.lock().unwrap().bool("mascot_enabled");
    let mut ids = if enabled { shown_ids(shared) } else { vec![] };
    ids.retain(|s| s != id);
    if show {
        ids.push(id.to_string());
    }
    let mut changes = Map::new();
    changes.insert("mascot_enabled".into(), json!(!ids.is_empty()));
    if !ids.is_empty() {
        changes.insert("mascot_shown".into(), json!(ids));
    }
    save_settings(shared, changes);
}

fn migrate_custom_image(shared: &Shared) {
    let (legacy, legacy_pos) = {
        let cfg = shared.config.lock().unwrap();
        (cfg.str("mascot_image"), cfg.get("mascot_pos").cloned())
    };
    let legacy = legacy.trim().to_string();
    let mut changes = Map::new();

    if !legacy.is_empty() {
        if let Ok(id) = add_to_gallery(shared, Path::new(&legacy)) {
            changes.insert("mascot_shown".into(), json!([id.clone()]));
            if let Some(pos) = legacy_pos.clone().filter(|p| !p.is_null()) {
                changes.insert("mascot_positions".into(), json!({ id: pos }));
            }
        }
        changes.insert("mascot_image".into(), json!(""));
    } else if let Some(pos) = legacy_pos.filter(|p| !p.is_null()) {
        changes.insert("mascot_positions".into(), json!({ BUILT_IN_ID: pos }));
    }
    if !changes.is_empty() {
        changes.insert("mascot_pos".into(), Value::Null);
        save_settings(shared, changes);
    }
}

fn default_position(width: u32, height: u32, near: (i32, i32), offset: &mut i32) -> (i32, i32) {
    let (_, _, right, bottom) = winapi::work_area_at(near.0, near.1);
    let x = right - width as i32 - 40 - *offset;
    *offset += width as i32 + 20;
    (x, bottom - height as i32)
}

fn saved_position(value: Option<&Value>, width: u32, height: u32) -> Option<(i32, i32)> {
    let pos = value?.as_array()?;
    let x = pos.first()?.as_i64()? as i32;
    let y = pos.get(1)?.as_i64()? as i32;

    let (vx, vy, vw, vh) = winapi::virtual_screen();
    let center_x = x + width as i32 / 2;
    let center_y = y + height as i32 / 2;
    let on_screen = center_x >= vx && center_x <= vx + vw && center_y >= vy && y <= vy + vh - 40;
    on_screen.then_some((x, y))
}

#[derive(Serialize)]
pub struct MascotInfo {
    pub file: String,
    pub width: u32,
    pub height: u32,
}

pub fn info(label: &str) -> Option<MascotInfo> {
    with_characters(|characters| {
        let character = characters.get(label)?;
        Some(MascotInfo {
            file: character.file.to_string_lossy().to_string(),
            width: character.alpha.width(),
            height: character.alpha.height(),
        })
    })
}

pub fn apply_settings(app: &AppHandle) {
    let mine = REQUESTS.fetch_add(1, Ordering::SeqCst) + 1;
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(150));
        if REQUESTS.load(Ordering::SeqCst) != mine {
            return;
        }
        let _applying = APPLYING.lock().unwrap();
        apply_now(&app);
    });
}

fn token_of(label: &str) -> Option<u64> {
    with_characters(|characters| characters.get(label).map(|c| c.token))
}

pub fn force_rebuild(app: &AppHandle, label: &str) {
    FORCE.lock().unwrap().push(label.to_string());
    apply_settings(app);
}

fn apply_now(app: &AppHandle) {
    let shared = shared(app);
    migrate_custom_image(&shared);

    let (enabled, topmost, positions) = {
        let cfg = shared.config.lock().unwrap();
        (
            cfg.bool("mascot_enabled"),
            cfg.bool("mascot_topmost"),
            cfg.get("mascot_positions").cloned().unwrap_or(json!({})),
        )
    };
    let wanted: Vec<String> = if enabled { shown_ids(&shared) } else { vec![] };
    let wanted_labels: Vec<String> = wanted.iter().map(|id| format!("{WINDOW_PREFIX}{id}")).collect();
    let forced: Vec<String> = std::mem::take(&mut *FORCE.lock().unwrap());

    let leaving: Vec<(String, tauri::WebviewWindow)> = app
        .webview_windows()
        .into_iter()
        .filter(|(label, _)| label.starts_with(WINDOW_PREFIX))
        .filter(|(label, _)| !wanted_labels.contains(label) || forced.contains(label))
        .collect();
    if !leaving.is_empty() {
        ui_windows::hide_balloon(app);
        with_characters(|characters| {
            for (label, _) in &leaving {
                characters.remove(label);
            }
        });
        for (label, window) in &leaving {
            crate::crash::note(format!("hide {label} before destroying it"));
            let _ = window.hide();
        }
        std::thread::sleep(Duration::from_millis(600));
        for (label, window) in &leaving {
            crate::crash::note(format!("destroy {label}"));
            let _ = window.destroy();
        }
        std::thread::sleep(Duration::from_millis(300));
    }

    let cache = shared.data_dir.join("cache");
    let mut offset = 0;
    for (id, label) in wanted.iter().zip(&wanted_labels) {
        if let Some(window) = app.get_webview_window(label) {
            let _ = window.set_always_on_top(topmost);
            let _ = window.set_always_on_bottom(!topmost);
            if positions.get(id).is_none() {
                if let (Ok(pos), Ok(size)) = (window.outer_position(), window.outer_size()) {
                    let center = (pos.x + size.width as i32 / 2, pos.y + size.height as i32 / 2);
                    let (x, y) = default_position(size.width, size.height, center, &mut offset);
                    let _ = window.set_position(PhysicalPosition::new(x, y));
                }
            }
            continue;
        }

        let Some(img) = image_for(&shared, id) else {
            continue;
        };
        let character = fit(img, height_for(&shared, id));
        let (width, height) = (character.width(), character.height());
        let token = GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
        let file = cache.join(format!("mascot_{token}_{id}.png"));
        if character.save(&file).is_err() {
            continue;
        }
        let (x, y) = saved_position(positions.get(id), width, height)
            .unwrap_or_else(|| {
                let near = winapi::cursor_pos().unwrap_or((0, 0));
                default_position(width, height, near, &mut offset)
            });
        let view_only = view_only_of(&shared, id);
        with_characters(|characters| {
            characters.insert(label.clone(), Character { file, alpha: character, view_only, token })
        });
        build_window(app, label, x, y, width, height, topmost, token);
    }
    remove_old_pictures(&cache);
}

pub fn resize(app: &AppHandle, id: &str, height: u32) -> Result<(), String> {
    crate::crash::note(format!("resize {id} to {height}"));
    let shared = shared(app);
    let height = set_height(&shared, id, height);

    let _applying = APPLYING.lock().unwrap();
    let label = format!("{WINDOW_PREFIX}{id}");
    let Some(window) = app.get_webview_window(&label) else {
        return Ok(());
    };
    let img = image_for(&shared, id).ok_or("That mascot's image is gone.")?;
    let character = fit(img, height);
    let (width, new_height) = (character.width(), character.height());

    let n = RESIZES.fetch_add(1, Ordering::SeqCst);
    let file = shared.data_dir.join("cache").join(format!("mascot_r{n}_{id}.png"));
    character.save(&file).map_err(|e| e.to_string())?;
    let view_only = view_only_of(&shared, id);
    let token = token_of(&label).unwrap_or_else(|| GENERATION.fetch_add(1, Ordering::SeqCst) + 1);
    with_characters(|characters| {
        characters.insert(label.clone(), Character { file, alpha: character, view_only, token })
    });

    if let (Ok(pos), Ok(size)) = (window.outer_position(), window.outer_size()) {
        let bottom = pos.y + size.height as i32;
        let _ = window.set_size(PhysicalSize::new(width, new_height));
        let _ = window.set_position(PhysicalPosition::new(pos.x, bottom - new_height as i32));
    }
    let _ = app.emit_to(label.as_str(), "mascot-refresh", ());
    Ok(())
}

fn remove_old_pictures(cache: &Path) {
    let in_use: Vec<PathBuf> = with_characters(|characters| characters.values().map(|c| c.file.clone()).collect());
    for entry in std::fs::read_dir(cache).into_iter().flatten().flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with("mascot_") && name.ends_with(".png") && !in_use.contains(&entry.path()) {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn build_window(
    app: &AppHandle,
    label: &str,
    x: i32,
    y: i32,
    width: u32,
    height: u32,
    topmost: bool,
    token: u64,
) {
    crate::crash::event(format!("create {label}"));
    alive(label);
    let built = WebviewWindowBuilder::new(app, label, WebviewUrl::App("mascot.html".into()))
        .additional_browser_args(ui_windows::BROWSER_ARGS)
        .title("Shiori RPC mascot")
        .decorations(false)
        .transparent(true)
        .shadow(false)
        .resizable(false)
        .skip_taskbar(true)
        .always_on_top(topmost)
        .focused(false)
        .visible(false)
        .build();
    let window = match built {
        Ok(window) => window,
        Err(error) => {
            crate::crash::event(format!("couldn't create {label}: {error}"));
            eprintln!("couldn't create {label}: {error}");
            return;
        }
    };

    watch_webview(app, &window, label);
    let _ = window.set_size(PhysicalSize::new(width, height));
    let _ = window.set_position(PhysicalPosition::new(x, y));
    if !topmost {
        let _ = window.set_always_on_bottom(true);
    }
    let _ = window.show();

    let app = app.clone();
    let label = label.to_string();
    std::thread::spawn(move || click_through_loop(&app, &label, token));
}

fn watch_webview(app: &AppHandle, window: &tauri::WebviewWindow, label: &str) {
    use webview2_com::Microsoft::Web::WebView2::Win32::COREWEBVIEW2_PROCESS_FAILED_KIND;
    use webview2_com::ProcessFailedEventHandler;

    let app = app.clone();
    let label = label.to_string();
    let _ = window.with_webview(move |webview| unsafe {
        let Ok(core) = webview.controller().CoreWebView2() else {
            return;
        };
        let handler = ProcessFailedEventHandler::create(Box::new(move |_, args| {
            let mut kind = COREWEBVIEW2_PROCESS_FAILED_KIND::default();
            if let Some(args) = args {
                let _ = args.ProcessFailedKind(&mut kind);
            }
            if EXITING.load(Ordering::SeqCst) {
                return Ok(());
            }
            let fatal = matches!(kind.0, 0 | 1 | 2 | 6);
            crate::crash::event(format!("{label}: WebView2 process failed (kind {})", kind.0));
            if fatal {
                force_rebuild(&app, &label);
            }
            Ok(())
        }));
        let mut token = 0i64;
        let _ = core.add_ProcessFailed(&handler, &mut token);
    });
}

fn click_through_loop(app: &AppHandle, label: &str, token: u64) {
    let mut ignoring: Option<bool> = None;

    while token_of(label) == Some(token) {
        std::thread::sleep(Duration::from_millis(40));

        let Some(window) = app.get_webview_window(label) else {
            return;
        };
        let (Some((cx, cy)), Ok(pos)) = (winapi::cursor_pos(), window.outer_position()) else {
            continue;
        };

        let over_character = with_characters(|characters| {
            let character = characters.get(label)?;
            let (x, y) = (cx - pos.x, cy - pos.y);
            let inside = x >= 0
                && y >= 0
                && (x as u32) < character.alpha.width()
                && (y as u32) < character.alpha.height();
            let opaque = inside && character.alpha.get_pixel(x as u32, y as u32).0[3] > 10;
            Some(opaque && !character.view_only)
        });
        let Some(over_character) = over_character else {
            return;
        };

        let ignore = !over_character;
        if ignoring != Some(ignore) {
            let _ = window.set_ignore_cursor_events(ignore);
            ignoring = Some(ignore);
        }
    }
}

static MOVES: AtomicU64 = AtomicU64::new(0);

pub fn moved(app: &AppHandle) {
    let mine = MOVES.fetch_add(1, Ordering::SeqCst) + 1;
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(500));
        if MOVES.load(Ordering::SeqCst) == mine {
            save_positions(&app);
        }
    });
}

fn save_positions(app: &AppHandle) {
    let shared = shared(app);
    let mut positions = shared
        .config
        .lock()
        .unwrap()
        .get("mascot_positions")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();

    let labels: Vec<String> = with_characters(|characters| characters.keys().cloned().collect());
    for label in labels {
        let Some(window) = app.get_webview_window(&label) else {
            continue;
        };
        if let Ok(pos) = window.outer_position() {
            positions.insert(id_of(&label).to_string(), json!([pos.x, pos.y]));
        }
    }
    save_settings(&shared, Map::from_iter([("mascot_positions".into(), Value::Object(positions))]));
}

pub fn speaker() -> Option<String> {
    SPEAKER.lock().unwrap().clone()
}

pub(crate) const MAX_NAME: usize = 60;

pub(crate) fn name_of(shared: &Shared, id: &str) -> String {
    let saved = shared
        .config
        .lock()
        .unwrap()
        .get("mascot_names")
        .and_then(|all| all.get(id))
        .and_then(Value::as_str)
        .map(String::from);
    match saved {
        Some(name) => name,
        None if id == BUILT_IN_ID => "Default".into(),
        None => id.replace('_', " "),
    }
}

pub(crate) fn clean_name(name: &str) -> String {
    name.trim().chars().filter(|c| !c.is_control()).take(MAX_NAME).collect::<String>().trim().to_string()
}

pub fn set_name(shared: &Shared, id: &str, name: &str) {
    let name = clean_name(name);
    let mut all = shared
        .config
        .lock()
        .unwrap()
        .get("mascot_names")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    if name.is_empty() {
        all.remove(id);
    } else {
        all.insert(id.to_string(), json!(name));
    }
    save_settings(shared, Map::from_iter([("mascot_names".into(), Value::Object(all))]));
}

pub(crate) fn personality_of(shared: &Shared, id: &str) -> String {
    shared
        .config
        .lock()
        .unwrap()
        .get("mascot_personalities")
        .and_then(|all| all.get(id))
        .and_then(Value::as_str)
        .unwrap_or(personalities::DEFAULT)
        .to_string()
}

pub fn set_personality(shared: &Shared, id: &str, personality: &str) {
    let mut all = shared
        .config
        .lock()
        .unwrap()
        .get("mascot_personalities")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    all.insert(id.to_string(), json!(personality));
    save_settings(shared, Map::from_iter([("mascot_personalities".into(), Value::Object(all))]));
}

pub(crate) fn custom_lines_of(shared: &Shared, id: &str) -> Value {
    shared
        .config
        .lock()
        .unwrap()
        .get("mascot_custom_lines")
        .and_then(|all| all.get(id))
        .cloned()
        .unwrap_or_else(|| json!({}))
}

pub fn set_custom_lines(shared: &Shared, id: &str, lines: Value) {
    let mut all = shared
        .config
        .lock()
        .unwrap()
        .get("mascot_custom_lines")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    all.insert(id.to_string(), lines);
    save_settings(shared, Map::from_iter([("mascot_custom_lines".into(), Value::Object(all))]));
}

fn lines_for(shared: &Shared, id: &str, language: &str) -> Lines {
    let personality = personality_of(shared, id);
    if personality == personalities::CUSTOM {
        let base = personalities::lines(personalities::DEFAULT, language);
        personalities::custom(base, &custom_lines_of(shared, id))
    } else {
        personalities::lines(&personality, language)
    }
}

fn say_as(
    app: &AppHandle,
    label: Option<&str>,
    event: &str,
    line: impl Fn(&Lines) -> Said,
    seconds: u64,
) {
    let labels: Vec<String> = with_characters(|characters| characters.keys().cloned().collect());
    let label = match label {
        Some(label) if labels.iter().any(|l| l == label) => label.to_string(),
        _ if labels.is_empty() => return,
        _ => labels[random(labels.len())].clone(),
    };

    LAST_SAID.store(now(), Ordering::SeqCst);
    crate::crash::note(format!("{label} says {event}"));
    let shared = shared(app);
    let (talks, language, volume) = {
        let cfg = shared.config.lock().unwrap();
        (
            cfg.get("mascot_talk").and_then(Value::as_bool).unwrap_or(true),
            lang::resolve(&cfg.str("language")),
            cfg.get("mascot_voice_volume").and_then(Value::as_u64).unwrap_or(80),
        )
    };
    let said = line(&lines_for(&shared, id_of(&label), language));
    if volume > 0 {
        if let Some(file) = voice_for(&shared, id_of(&label), event, said.index) {
            let _ = app.emit_to(label.as_str(), "voice", json!({ "file": file, "volume": volume }));
        }
    }
    if !talks {
        return;
    }
    *SPEAKER.lock().unwrap() = Some(label);
    ui_windows::show_balloon(app, &said.text, seconds as f64);
}

struct Said {
    text: String,
    index: Option<usize>,
}

fn just(text: String) -> Said {
    Said { text, index: None }
}

fn pick(lines: &[String]) -> Said {
    let index = random(lines.len());
    Said {
        text: lines.get(index).cloned().unwrap_or_default(),
        index: Some(index),
    }
}

fn now() -> u64 {
    chrono::Utc::now().timestamp().max(0) as u64
}

pub fn alive(label: &str) {
    LAST_SEEN
        .lock()
        .unwrap()
        .get_or_insert_with(HashMap::new)
        .insert(label.to_string(), Instant::now());
}

fn needs_rebuild(app: &AppHandle) -> Option<(String, String)> {
    let labels: Vec<String> = with_characters(|characters| characters.keys().cloned().collect());
    let seen = LAST_SEEN.lock().unwrap().clone().unwrap_or_default();
    for label in labels {
        let Some(window) = app.get_webview_window(&label) else {
            return Some((label.clone(), format!("{label} window is gone")));
        };
        if window.is_minimized().unwrap_or(false) {
            let _ = window.unminimize();
        }
        if !window.is_visible().unwrap_or(true) {
            let _ = window.show();
        }
        let silent = seen.get(&label).map_or(SILENT_LIMIT, Instant::elapsed);
        if silent >= SILENT_LIMIT {
            return Some((label.clone(), format!("{label} stopped answering")));
        }
    }
    None
}

pub fn start_watchdog(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_secs(15));
        if !shared(&app).config.lock().unwrap().bool("mascot_enabled") {
            continue;
        }
        let reason = match APPLYING.try_lock() {
            Ok(_guard) => needs_rebuild(&app),
            Err(_) => None,
        };
        if let Some((label, reason)) = reason {
            crate::crash::event(format!("rebuild mascot: {reason}"));
            force_rebuild(&app, &label);
        }
    });
}

pub fn start_chatter(app: &AppHandle) {
    LAST_SAID.store(now(), Ordering::SeqCst);
    let app = app.clone();
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_secs(10));
        let minutes = shared(&app)
            .config
            .lock()
            .unwrap()
            .get("mascot_chatter_minutes")
            .and_then(Value::as_u64)
            .unwrap_or(0);
        if minutes == 0 || now().saturating_sub(LAST_SAID.load(Ordering::SeqCst)) < minutes * 60 {
            continue;
        }
        say_as(
            &app,
            None,
            "chatter",
            |lines| pick(&lines.chatter),
            6,
        );
    });
}

pub(crate) fn view_only_of(shared: &Shared, id: &str) -> bool {
    shared
        .config
        .lock()
        .unwrap()
        .get("mascot_view_only")
        .and_then(|all| all.get(id))
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

pub fn set_view_only(shared: &Shared, ids: &[String], view_only: bool) {
    let mut all = shared
        .config
        .lock()
        .unwrap()
        .get("mascot_view_only")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    for id in ids {
        all.insert(id.clone(), json!(view_only));
    }
    save_settings(shared, Map::from_iter([("mascot_view_only".into(), Value::Object(all))]));
    with_characters(|characters| {
        for (label, character) in characters.iter_mut() {
            if ids.iter().any(|id| id == id_of(label)) {
                character.view_only = view_only;
            }
        }
    });
}

pub fn toggle_view_only(app: &AppHandle, key: &str) {
    crate::crash::note("view-only key");
    let shared = shared(app);
    let labels: Vec<String> = with_characters(|characters| characters.keys().cloned().collect());
    if labels.is_empty() {
        return;
    }
    let ids: Vec<String> = labels.iter().map(|l| id_of(l).to_string()).collect();
    let view_only = !with_characters(|characters| characters.values().all(|c| c.view_only));
    set_view_only(&shared, &ids, view_only);

    let language = lang::resolve(&shared.config.lock().unwrap().str("language"));
    let title = if view_only { "Mascots: clicks go through" } else { "Mascots: you can click them" };
    let toast = Toast {
        title: lang::tr(title, language).to_string(),
        detail: lang::tr("Press {} again to switch back.", language).replace("{}", key),
        image: None,
        ok: true,
        ..Default::default()
    };
    ui_windows::show_toast(app, toast, None);
    let settings = shared.config.lock().unwrap().data().clone();
    let _ = app.emit("settings-changed", settings);
}

fn voices_dir(shared: &Shared, id: &str) -> PathBuf {
    gallery_dir(shared).join("voices").join(id)
}

fn audio_files(dir: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .map(|e| e.to_string_lossy().to_lowercase())
                .is_some_and(|ext| AUDIO_EXTS.contains(&ext.as_str()))
        })
        .collect();
    files.sort_by_key(|path| natural_key(&path.file_name().unwrap_or_default().to_string_lossy()));
    files
}

fn natural_key(name: &str) -> String {
    let mut key = String::new();
    let mut digits = String::new();
    for c in name.to_lowercase().chars().chain(std::iter::once('\0')) {
        if c.is_ascii_digit() {
            digits.push(c);
            continue;
        }
        if !digits.is_empty() {
            key.push_str(&format!("{digits:0>20}"));
            digits.clear();
        }
        key.push(c);
    }
    key
}

fn voice_for(shared: &Shared, id: &str, event: &str, index: Option<usize>) -> Option<String> {
    let files = audio_files(&voices_dir(shared, id).join(event));
    if files.is_empty() {
        return None;
    }
    let n = index.unwrap_or_else(|| random(files.len())) % files.len();
    Some(files[n].to_string_lossy().to_string())
}

pub fn voices(shared: &Shared, id: &str) -> Map<String, Value> {
    VOICE_EVENTS
        .iter()
        .map(|event| {
            let files: Vec<String> = audio_files(&voices_dir(shared, id).join(event))
                .iter()
                .map(|f| f.to_string_lossy().to_string())
                .collect();
            (event.to_string(), json!(files))
        })
        .collect()
}

pub fn add_voice(shared: &Shared, id: &str, event: &str, source: &Path) -> Result<(), String> {
    if !VOICE_EVENTS.contains(&event) {
        return Err("Unknown moment.".into());
    }
    let ext = source
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .filter(|ext| AUDIO_EXTS.contains(&ext.as_str()))
        .ok_or("Use an MP3, WAV, OGG, M4A, OPUS or FLAC file.")?;
    let dir = voices_dir(shared, id).join(event);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;

    let stem = safe_filename(&source.file_stem().unwrap_or_default().to_string_lossy());
    let (mut target, mut n) = (dir.join(format!("{stem}.{ext}")), 2);
    while target.exists() {
        target = dir.join(format!("{stem}_{n}.{ext}"));
        n += 1;
    }
    std::fs::copy(source, target).map_err(|e| e.to_string())?;
    Ok(())
}

pub fn remove_voice(shared: &Shared, file: &Path) -> Result<(), String> {
    let root = gallery_dir(shared).join("voices");
    if !file.starts_with(&root) {
        return Err("That file isn't a mascot voice.".into());
    }
    std::fs::remove_file(file).map_err(|e| e.to_string())
}

fn random(len: usize) -> usize {
    use std::hash::{BuildHasher, RandomState};
    static DRAWS: AtomicU64 = AtomicU64::new(0);
    let draw = DRAWS.fetch_add(1, Ordering::Relaxed);
    (RandomState::new().hash_one(draw) % len.max(1) as u64) as usize
}

pub fn screenshot_saved(app: &AppHandle) {
    say_as(app, None, "screenshot", |lines| just(lines.screenshot.clone()), 3);
}

pub fn poke(app: &AppHandle, label: &str) {
    say_as(
        app,
        Some(label),
        "poke",
        |lines| pick(&lines.pokes),
        6,
    );
}

pub fn react(app: &AppHandle, old: &Snapshot, new: &Snapshot) {
    let name = new.game_name.clone();
    let section = new.section_label.clone();

    if new.detected && (!old.detected || old.key != new.key) {
        say_as(app, None, "start", |lines| just(lines.start.replace("{name}", &name)), 6);
    } else if old.detected && !new.detected {
        say_as(app, None, "stop", |lines| just(lines.stop.clone()), 6);
    } else if new.detected && new.idle && !old.idle {
        say_as(app, None, "idle", |lines| just(lines.idle.clone()), 6);
    } else if new.detected && old.idle && !new.idle {
        say_as(app, None, "back", |lines| just(lines.back.clone()), 3);
    } else if new.detected && !section.is_empty() && section != old.section_label {
        say_as(app, None, "section", |lines| just(lines.section.replace("{section}", &section)), 4);
    }
}

pub fn show_menu(app: &AppHandle, label: &str) {
    crate::crash::note(format!("menu on {label}"));
    let Some(window) = app.get_webview_window(label) else {
        return;
    };

    let language = lang::resolve(&shared(app).config.lock().unwrap().str("language"));
    let menu = (|| -> tauri::Result<Menu<tauri::Wry>> {
        let open = MenuItem::with_id(
            app,
            "mascot-open",
            lang::tr("Open Shiori RPC", language),
            true,
            None::<&str>,
        )?;
        let pick = MenuItem::with_id(
            app,
            "mascot-pick",
            lang::tr("Random VN from my wishlist", language),
            true,
            None::<&str>,
        )?;
        let separator = PredefinedMenuItem::separator(app)?;
        let hide = MenuItem::with_id(
            app,
            format!("{HIDE_MENU_PREFIX}{label}"),
            lang::tr("Hide this mascot", language),
            true,
            None::<&str>,
        )?;
        Menu::with_items(app, &[&open, &pick, &separator, &hide])
    })();

    if let Ok(menu) = menu {
        let _ = window.popup_menu(&menu);
    }
}

pub fn refresh(app: &AppHandle) {
    apply_settings(app);
    let settings = shared(app).config.lock().unwrap().data().clone();
    let _ = app.emit("settings-changed", settings);
}

pub fn hide(app: &AppHandle, label: &str) {
    set_shown(&shared(app), id_of(label), false);
    refresh(app);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn picture(width: u32, height: u32, color: impl Fn(u32, u32) -> [u8; 3]) -> RgbaImage {
        RgbaImage::from_fn(width, height, |x, y| {
            let [r, g, b] = color(x, y);
            image::Rgba([r, g, b, 255])
        })
    }

    #[test]
    fn a_flat_background_becomes_transparent() {
        let inside = |x: u32, y: u32| (6..14).contains(&x) && (6..14).contains(&y);
        let img = picture(20, 20, |x, y| if inside(x, y) { [200, 30, 30] } else { [250, 250, 250] });
        let cleared = clear_flat_background(img);
        assert_eq!(cleared.get_pixel(0, 0).0[3], 0);
        assert_eq!(cleared.get_pixel(5, 5).0[3], 0);
        assert_eq!(cleared.get_pixel(10, 10).0[3], 255);
    }

    #[test]
    fn a_blurry_white_edge_leaves_no_white_fringe() {
        let img = picture(40, 40, |x, y| {
            let edge = [x.abs_diff(20), y.abs_diff(20)].into_iter().max().unwrap();
            match edge {
                0..=10 => [20, 20, 20],
                11 => [200, 200, 200],
                12 => [232, 232, 232],
                _ => [255, 255, 255],
            }
        });
        let cleared = clear_flat_background(img);
        assert!(cleared.get_pixel(20, 8).0[3] < 100, "{:?}", cleared.get_pixel(20, 8));
        assert_eq!(cleared.get_pixel(20, 20).0[3], 255);

        let larger = image::imageops::resize(&cleared, 80, 80, FilterType::CatmullRom);
        let fringe = larger
            .pixels()
            .filter(|p| p.0[3] > 128 && p.0[..3].iter().all(|c| *c > 180))
            .count();
        assert_eq!(fringe, 0);
    }

    #[test]
    fn transparent_pixels_take_the_character_color_near_it() {
        let img = picture(20, 20, |x, y| {
            if (6..14).contains(&x) && (6..14).contains(&y) {
                [20, 20, 200]
            } else {
                [255, 255, 255]
            }
        });
        let cleared = clear_flat_background(img);
        let beside = cleared.get_pixel(10, 5).0;
        assert_eq!(beside[3], 0);
        assert!(beside[0] < 100, "{beside:?}");
    }

    #[test]
    fn white_clothes_survive_a_small_gap_in_their_outline() {
        let outline = |x: u32, y: u32| {
            let square = (10..30).contains(&x) && (10..30).contains(&y);
            let inside = (12..28).contains(&x) && (12..28).contains(&y);
            let gap = (19..21).contains(&x) && y < 12;
            square && !inside && !gap
        };
        let img = picture(40, 40, |x, y| if outline(x, y) { [15, 15, 15] } else { [255, 255, 255] });
        let cleared = clear_flat_background(img);
        assert_eq!(cleared.get_pixel(0, 0).0[3], 0);
        assert_eq!(cleared.get_pixel(20, 20).0[3], 255);
        assert_eq!(cleared.get_pixel(14, 14).0[3], 255);
    }

    #[test]
    fn small_gaps_between_strands_clear_but_closed_eyes_stay() {
        let img = picture(100, 100, |x, y| {
            let body = (20..80).contains(&x) && (20..80).contains(&y);
            let eye = (30..36).contains(&x) && (30..36).contains(&y);
            let sliver = (50..53).contains(&x) && (20..35).contains(&y);
            if body && !eye && !sliver {
                [15, 15, 15]
            } else {
                [255, 255, 255]
            }
        });
        let cleared = clear_flat_background(img);
        assert_eq!(cleared.get_pixel(51, 30).0[3], 0);
        assert_eq!(cleared.get_pixel(33, 33).0[3], 255);
    }

    #[test]
    fn white_socks_cut_by_the_bottom_edge_stay() {
        let leg = |x: u32, y: u32| (15..25).contains(&x) && y >= 20;
        let inside = |x: u32, y: u32| (17..23).contains(&x) && y >= 22;
        let img = picture(40, 40, |x, y| {
            if leg(x, y) && !inside(x, y) {
                [15, 15, 15]
            } else {
                [255, 255, 255]
            }
        });
        let cleared = clear_flat_background(img);
        assert_eq!(cleared.get_pixel(0, 39).0[3], 0);
        assert_eq!(cleared.get_pixel(20, 39).0[3], 255);
        assert_eq!(cleared.get_pixel(20, 30).0[3], 255);
    }

    #[test]
    fn white_inside_the_character_stays() {
        let ring = |x: u32, y: u32| (5..15).contains(&x) && (5..15).contains(&y) && !((8..12).contains(&x) && (8..12).contains(&y));
        let img = picture(20, 20, |x, y| if ring(x, y) { [20, 20, 20] } else { [255, 255, 255] });
        let cleared = clear_flat_background(img);
        assert_eq!(cleared.get_pixel(0, 0).0[3], 0);
        assert_eq!(cleared.get_pixel(10, 10).0[3], 255);
    }

    #[test]
    fn a_busy_background_is_left_alone() {
        let img = picture(20, 20, |x, y| [(x * 12) as u8, (y * 12) as u8, ((x + y) * 6) as u8]);
        let cleared = clear_flat_background(img);
        assert!(cleared.pixels().all(|p| p.0[3] == 255));
    }

    #[test]
    fn a_nearly_blank_picture_is_left_alone() {
        let img = picture(20, 20, |_, _| [255, 255, 255]);
        let cleared = clear_flat_background(img);
        assert!(cleared.pixels().all(|p| p.0[3] == 255));
    }

    #[test]
    fn voice_files_sort_like_people_count() {
        let mut names = vec!["quote10.mp3", "quote2.mp3", "Quote1.mp3", "a.wav"];
        names.sort_by_key(|name| natural_key(name));
        assert_eq!(names, ["a.wav", "Quote1.mp3", "quote2.mp3", "quote10.mp3"]);
    }

    #[test]
    fn random_reaches_every_choice() {
        let mut seen = [false; 12];
        for _ in 0..2000 {
            seen[random(12)] = true;
        }
        assert!(seen.iter().all(|s| *s));
        assert!((0..200).any(|_| random(2) == 1));
    }
}
