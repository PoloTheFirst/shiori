use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use image::imageops::FilterType;
use image::{GrayImage, Luma, RgbaImage};
use ort::environment::Environment;
use ort::session::Session;
use ort::value::Tensor;
use serde_json::json;
use sha2::{Digest, Sha256};
use zip::ZipArchive;

use crate::mascot::neighbours;
use crate::state::Shared;

const USER_AGENT: &str = concat!("ShioriRPC/", env!("CARGO_PKG_VERSION"));
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(3600);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const CHUNK: usize = 64 * 1024;
const CANCELLED: &str = "cancelled";
const DAMAGED: &str = "The downloaded file is incomplete or corrupted.";
const COULD_NOT_DOWNLOAD: &str = "Couldn't download the AI model. Using the basic cutout.";
const NEEDS_LIBRARIES: &str = "The AI model needs the Microsoft Visual C++ Redistributable. Install it, then try again. For now, the basic cutout is used.";
const SYSTEM_LIBRARIES: [&str; 4] =
    ["vcruntime140.dll", "vcruntime140_1.dll", "msvcp140.dll", "msvcp140_1.dll"];

const MODEL_FILE: &str = "isnetis.onnx";
const RUNTIME_FILE: &str = "onnxruntime.dll";
const RUNTIME_ENTRY: &str = "onnxruntime-win-x64-1.31.1/lib/onnxruntime.dll";

struct Download {
    url: &'static str,
    sha256: &'static str,
    size: u64,
}

const RUNTIME: Download = Download {
    url: "https://github.com/microsoft/onnxruntime/releases/download/v1.31.1/onnxruntime-win-x64-1.31.1.zip",
    sha256: "5869e26bb25314375f2d5ef1877e3ea06b1543d6c710e6e4a1ec2f7177ab4b2c",
    size: 82_929_944,
};
const MODEL: Download = Download {
    url: "https://huggingface.co/skytnt/anime-seg/resolve/main/isnetis.onnx",
    sha256: "f15622d853e8260172812b657053460e20806f04b9e05147d49af7bed31a6e99",
    size: 176_069_933,
};

const SIZE: u32 = 1024;
const FAINT_ALPHA: u8 = 12;
const SOLID_ALPHA: u8 = 230;
const COLOR_ROUNDS: u32 = 6;
const MIN_KEPT: f64 = 0.02;

static ENGINE: Mutex<Option<Environment>> = Mutex::new(None);
static DOWNLOADING: AtomicBool = AtomicBool::new(false);
static STOP: AtomicBool = AtomicBool::new(false);

fn folder(shared: &Shared) -> PathBuf {
    shared.data_dir.join("ai")
}

pub fn ready(shared: &Shared) -> bool {
    let dir = folder(shared);
    dir.join(MODEL_FILE).is_file() && dir.join(RUNTIME_FILE).is_file()
}

pub fn cancel() {
    STOP.store(true, Ordering::SeqCst);
}

pub fn remove(shared: &Shared) -> Result<(), String> {
    let _engine = ENGINE.lock().unwrap();
    let dir = folder(shared);
    fs::remove_file(dir.join(MODEL_FILE)).map_err(|e| e.to_string())?;
    let _ = fs::remove_file(dir.join(RUNTIME_FILE));
    Ok(())
}

pub fn clean_unfinished(shared: &Shared) {
    for entry in fs::read_dir(folder(shared)).into_iter().flatten().flatten() {
        if entry.path().extension().is_some_and(|ext| ext == "part") {
            let _ = fs::remove_file(entry.path());
        }
    }
}

fn has_libraries(system: &Path) -> bool {
    SYSTEM_LIBRARIES.iter().all(|name| system.join(name).is_file())
}

pub fn download(shared: &Shared) -> Result<bool, String> {
    let system = std::env::var_os("SystemRoot").map(|root| Path::new(&root).join("System32"));
    if system.is_some_and(|system| !has_libraries(&system)) {
        return Err(NEEDS_LIBRARIES.into());
    }
    if DOWNLOADING.swap(true, Ordering::SeqCst) {
        return Err(COULD_NOT_DOWNLOAD.into());
    }
    STOP.store(false, Ordering::SeqCst);
    let result = download_missing(shared);
    DOWNLOADING.store(false, Ordering::SeqCst);
    match result {
        Err(error) if error == CANCELLED => Ok(false),
        Err(error) => {
            eprintln!("the AI download failed: {error}");
            Err(COULD_NOT_DOWNLOAD.into())
        }
        Ok(()) => Ok(true),
    }
}

fn download_missing(shared: &Shared) -> Result<(), String> {
    let dir = folder(shared);
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let client = reqwest::blocking::Client::builder()
        .user_agent(USER_AGENT)
        .connect_timeout(CONNECT_TIMEOUT)
        .timeout(DOWNLOAD_TIMEOUT)
        .build()
        .map_err(|e| e.to_string())?;

    let dll = dir.join(RUNTIME_FILE);
    let model = dir.join(MODEL_FILE);
    let need_runtime = !dll.is_file();
    let need_model = !model.is_file();
    let total = u64::from(need_runtime) * RUNTIME.size + u64::from(need_model) * MODEL.size;
    let mut done = 0;

    if need_runtime {
        let archive = dir.join("onnxruntime.zip.part");
        fetch(shared, &client, &RUNTIME, &archive, &mut done, total)?;
        let unpacked = unpack_runtime(&archive, &dll);
        let _ = fs::remove_file(&archive);
        unpacked?;
    }
    if need_model {
        let part = dir.join("isnetis.onnx.part");
        fetch(shared, &client, &MODEL, &part, &mut done, total)?;
        fs::rename(&part, &model).map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn fetch(
    shared: &Shared,
    client: &reqwest::blocking::Client,
    item: &Download,
    part: &Path,
    done: &mut u64,
    total: u64,
) -> Result<(), String> {
    let response = client
        .get(item.url)
        .send()
        .and_then(|r| r.error_for_status())
        .map_err(|e| e.to_string())?;
    let file = File::create(part).map_err(|e| e.to_string())?;
    let mut shown = u64::MAX;
    let copied = copy_checked(response, file, item.sha256, |chunk| {
        *done += chunk;
        let percent = *done * 100 / total.max(1);
        if percent != shown {
            shown = percent;
            shared.emit(
                "cutout-progress",
                json!({ "percent": percent, "done": *done / 1_000_000, "total": total / 1_000_000 }),
            );
        }
    });
    if copied.is_err() {
        let _ = fs::remove_file(part);
    }
    copied
}

fn copy_checked(
    mut from: impl Read,
    mut to: impl Write,
    sha256: &str,
    mut progress: impl FnMut(u64),
) -> Result<(), String> {
    let mut hash = Sha256::new();
    let mut buffer = vec![0; CHUNK];
    loop {
        if STOP.load(Ordering::SeqCst) {
            return Err(CANCELLED.into());
        }
        let read = from.read(&mut buffer).map_err(|e| e.to_string())?;
        if read == 0 {
            break;
        }
        hash.update(&buffer[..read]);
        to.write_all(&buffer[..read]).map_err(|e| e.to_string())?;
        progress(read as u64);
    }
    let found: String = hash.finalize().iter().map(|b| format!("{b:02x}")).collect();
    if found != sha256 {
        return Err(DAMAGED.into());
    }
    Ok(())
}

fn unpack_runtime(archive: &Path, dll: &Path) -> Result<(), String> {
    let file = File::open(archive).map_err(|e| e.to_string())?;
    let mut archive = ZipArchive::new(file).map_err(|e| e.to_string())?;
    let mut entry = archive.by_name(RUNTIME_ENTRY).map_err(|e| e.to_string())?;
    let part = dll.with_extension("dll.part");
    let mut out = File::create(&part).map_err(|e| e.to_string())?;
    io::copy(&mut entry, &mut out).map_err(|e| e.to_string())?;
    drop(out);
    fs::rename(&part, dll).map_err(|e| e.to_string())
}

pub fn cut(shared: &Shared, picture: &RgbaImage) -> Result<RgbaImage, String> {
    let dir = folder(shared);
    let mut engine = ENGINE.lock().unwrap();
    if engine.is_none() {
        let built = ort::environment::init_from(dir.join(RUNTIME_FILE))
            .map_err(|e| e.to_string())?
            .build()
            .map_err(|e| e.to_string())?;
        *engine = Some(built);
    }
    let environment = engine.as_ref().ok_or("The AI engine didn't start.")?;
    let mut session = Session::builder(environment)
        .map_err(|e| e.to_string())?
        .commit_from_file(dir.join(MODEL_FILE))
        .map_err(|e| e.to_string())?;

    let frame = Frame::of(picture.width(), picture.height());
    let input = Tensor::<f32>::from_array((
        [1_usize, 3, SIZE as usize, SIZE as usize],
        model_input(picture, &frame),
    ))
    .map_err(|e| e.to_string())?;
    let outputs = session.run(ort::inputs![input]).map_err(|e| e.to_string())?;
    let (_, mask) = outputs[0].try_extract_tensor::<f32>().map_err(|e| e.to_string())?;
    if mask.len() != (SIZE * SIZE) as usize {
        return Err("The AI model gave an unexpected result.".into());
    }
    let alpha = mask_to_alpha(mask, &frame, picture.width(), picture.height());
    apply_alpha(picture, &alpha)
}

struct Frame {
    x: u32,
    y: u32,
    width: u32,
    height: u32,
}

impl Frame {
    fn of(width: u32, height: u32) -> Self {
        let scale = f64::from(SIZE) / f64::from(width.max(height).max(1));
        let width = ((f64::from(width) * scale).round() as u32).clamp(1, SIZE);
        let height = ((f64::from(height) * scale).round() as u32).clamp(1, SIZE);
        Frame { x: (SIZE - width) / 2, y: (SIZE - height) / 2, width, height }
    }
}

fn model_input(picture: &RgbaImage, frame: &Frame) -> Vec<f32> {
    let resized = image::imageops::resize(picture, frame.width, frame.height, FilterType::Lanczos3);
    let side = SIZE as usize;
    let mut input = vec![0.0; 3 * side * side];
    for (x, y, pixel) in resized.enumerate_pixels() {
        let place = (y + frame.y) as usize * side + (x + frame.x) as usize;
        for channel in 0..3 {
            input[channel * side * side + place] = f32::from(pixel.0[channel]) / 255.0;
        }
    }
    input
}

fn mask_to_alpha(mask: &[f32], frame: &Frame, width: u32, height: u32) -> GrayImage {
    let crop = GrayImage::from_fn(frame.width, frame.height, |x, y| {
        let value = mask[((y + frame.y) * SIZE + x + frame.x) as usize];
        Luma([(value * 255.0).round().clamp(0.0, 255.0) as u8])
    });
    image::imageops::resize(&crop, width, height, FilterType::Lanczos3)
}

fn apply_alpha(picture: &RgbaImage, alpha: &GrayImage) -> Result<RgbaImage, String> {
    let mut out = picture.clone();
    let mut kept = 0_u64;
    for (x, y, pixel) in out.enumerate_pixels_mut() {
        let value = alpha.get_pixel(x, y).0[0];
        kept += u64::from(value >= SOLID_ALPHA);
        pixel.0[3] = if value <= FAINT_ALPHA { 0 } else { value };
    }
    let all = u64::from(out.width()) * u64::from(out.height());
    if (kept as f64) < all as f64 * MIN_KEPT {
        return Err("The AI model didn't find a character.".into());
    }
    take_colors_from_inside(&mut out);
    Ok(out)
}

fn take_colors_from_inside(img: &mut RgbaImage) {
    let (width, height) = img.dimensions();
    let at = |x: u32, y: u32| (y * width + x) as usize;
    let mut known: Vec<bool> = img.pixels().map(|pixel| pixel.0[3] >= SOLID_ALPHA).collect();
    for _ in 0..COLOR_ROUNDS {
        let before = img.clone();
        let known_before = known.clone();
        for (x, y, pixel) in img.enumerate_pixels_mut() {
            if known_before[at(x, y)] {
                continue;
            }
            let colors: Vec<[u8; 4]> = neighbours(x, y, width, height)
                .filter(|(nx, ny)| known_before[at(*nx, *ny)])
                .map(|(nx, ny)| before.get_pixel(nx, ny).0)
                .collect();
            if colors.is_empty() {
                continue;
            }
            for channel in 0..3 {
                let total: u32 = colors.iter().map(|color| u32::from(color[channel])).sum();
                pixel.0[channel] = (total / colors.len() as u32) as u8;
            }
            known[at(x, y)] = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    #[test]
    fn a_picture_is_centered_in_the_square() {
        let frame = Frame::of(512, 1024);
        assert_eq!((frame.width, frame.height), (512, 1024));
        assert_eq!((frame.x, frame.y), (256, 0));
    }

    #[test]
    fn small_pictures_fill_the_square() {
        let frame = Frame::of(256, 128);
        assert_eq!((frame.width, frame.height), (1024, 512));
        assert_eq!((frame.x, frame.y), (0, 256));
    }

    #[test]
    fn the_input_is_planar_and_padded_with_black() {
        let picture = RgbaImage::from_pixel(2, 4, Rgba([255, 0, 51, 255]));
        let frame = Frame::of(2, 4);
        let input = model_input(&picture, &frame);
        let side = SIZE as usize;
        let middle = 512 * side + 512;
        assert_eq!(input.len(), 3 * side * side);
        assert!((input[middle] - 1.0).abs() < 0.01);
        assert!(input[side * side + middle].abs() < 0.01);
        assert!((input[2 * side * side + middle] - 0.2).abs() < 0.01);
        assert_eq!(input[512 * side], 0.0);
    }

    #[test]
    fn the_mask_is_read_inside_the_frame_only() {
        let frame = Frame::of(512, 1024);
        let side = SIZE as usize;
        let mut mask = vec![0.0; side * side];
        for y in 0..side {
            for x in 256..768 {
                mask[y * side + x] = 1.0;
            }
        }
        let alpha = mask_to_alpha(&mask, &frame, 512, 1024);
        assert!(alpha.pixels().all(|pixel| pixel.0[0] == 255));
    }

    #[test]
    fn an_empty_mask_is_refused() {
        let picture = RgbaImage::from_pixel(10, 10, Rgba([200, 200, 200, 255]));
        let alpha = GrayImage::from_pixel(10, 10, Luma([0]));
        assert!(apply_alpha(&picture, &alpha).is_err());
    }

    #[test]
    fn soft_edges_lose_the_background_color() {
        let mut picture = RgbaImage::from_pixel(5, 1, Rgba([255, 255, 255, 255]));
        picture.put_pixel(0, 0, Rgba([10, 20, 30, 255]));
        let mut alpha = GrayImage::from_pixel(5, 1, Luma([0]));
        alpha.put_pixel(0, 0, Luma([255]));
        alpha.put_pixel(1, 0, Luma([128]));
        alpha.put_pixel(2, 0, Luma([5]));
        let out = apply_alpha(&picture, &alpha).unwrap();
        assert_eq!(out.get_pixel(1, 0).0, [10, 20, 30, 128]);
        assert_eq!(out.get_pixel(2, 0).0, [10, 20, 30, 0]);
    }

    #[test]
    fn missing_system_libraries_are_noticed() {
        assert!(!has_libraries(Path::new("C:\\no\\such\\folder")));
    }

    #[test]
    fn a_wrong_hash_is_refused() {
        let mut out = Vec::new();
        let result = copy_checked(&b"hello"[..], &mut out, "00", |_| {});
        assert_eq!(result, Err(DAMAGED.to_string()));
    }

    #[test]
    fn the_right_hash_is_accepted() {
        let mut out = Vec::new();
        let mut seen = 0;
        let hello = "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824";
        assert_eq!(copy_checked(&b"hello"[..], &mut out, hello, |n| seen += n), Ok(()));
        assert_eq!(out, b"hello");
        assert_eq!(seen, 5);
    }
}
