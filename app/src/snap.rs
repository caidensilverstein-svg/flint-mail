// Global Cmd+Shift+B hotkey and planner snapshot helpers.
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use chrono::Local;
use eframe::egui;

pub struct Hotkey {
    _manager: global_hotkey::GlobalHotKeyManager,
    pub fired: Arc<AtomicBool>,
    pub shot: Arc<std::sync::Mutex<Option<Result<PathBuf, String>>>>,
}

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGPreflightScreenCaptureAccess() -> bool;
    fn CGRequestScreenCaptureAccess() -> bool;
}

pub fn capture_screen() -> Result<PathBuf, String> {
    if !unsafe { CGPreflightScreenCaptureAccess() } {
        unsafe { CGRequestScreenCaptureAccess() };
        return Err("Allow Flint Mail in System Settings > Privacy & Security > Screen Recording, then quit and reopen Flint and press Cmd+Shift+B again.".into());
    }
    let dir = snapshot_dir();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join(format!("screen-{}.png", Local::now().format("%Y-%m-%d_%H%M%S")));
    let ok = std::process::Command::new("screencapture").args(["-x", "-m"]).arg(&path).status().map(|s| s.success()).unwrap_or(false);
    if ok && path.exists() {
        Ok(path)
    } else {
        Err("The screen capture failed".into())
    }
}

pub fn encode_for_vision(path: &std::path::Path) -> Result<String, String> {
    let img = image::open(path).map_err(|e| e.to_string())?;
    let (w, h) = (img.width(), img.height());
    let long = w.max(h);
    let img = if long > 1568 { img.resize(1568, 1568, image::imageops::FilterType::Triangle) } else { img };
    let mut buf = std::io::Cursor::new(Vec::new());
    img.write_to(&mut buf, image::ImageFormat::Png).map_err(|e| e.to_string())?;
    Ok(base64(&buf.into_inner()))
}

fn base64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len() * 4 / 3 + 4);
    for c in data.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if c.len() > 1 { T[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if c.len() > 2 { T[n as usize & 63] as char } else { '=' });
    }
    out
}

pub fn register(ctx: &egui::Context) -> Result<Hotkey, String> {
    use global_hotkey::hotkey::{Code, HotKey, Modifiers};
    use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};
    let manager = GlobalHotKeyManager::new().map_err(|e| e.to_string())?;
    let hk = HotKey::new(Some(Modifiers::SUPER | Modifiers::SHIFT), Code::KeyB);
    manager.register(hk).map_err(|e| e.to_string())?;
    let fired = Arc::new(AtomicBool::new(false));
    let flag = fired.clone();
    let shot = Arc::new(std::sync::Mutex::new(None));
    let shot_w = shot.clone();
    let ctx = ctx.clone();
    std::thread::spawn(move || {
        let rx = GlobalHotKeyEvent::receiver();
        while let Ok(ev) = rx.recv() {
            if ev.id == hk.id() && ev.state == HotKeyState::Pressed {
                let res = capture_screen();
                *shot_w.lock().unwrap_or_else(|e| e.into_inner()) = Some(res);
                flag.store(true, Ordering::SeqCst);
                ctx.request_repaint();
            }
        }
    });
    Ok(Hotkey { _manager: manager, fired, shot })
}

pub fn snapshot_dir() -> PathBuf {
    let base = dirs::picture_dir().unwrap_or_else(|| PathBuf::from("."));
    base.join("Flint Planner")
}

pub fn crop_left(img: &egui::ColorImage, n: usize) -> egui::ColorImage {
    let [w, h] = img.size;
    let n = n.min(w.saturating_sub(1));
    let mut pixels = Vec::with_capacity((w - n) * h);
    for y in 0..h {
        pixels.extend_from_slice(&img.pixels[y * w + n..(y + 1) * w]);
    }
    egui::ColorImage { size: [w - n, h], pixels }
}

pub fn save(img: &egui::ColorImage) -> Result<PathBuf, String> {
    let [w, h] = img.size;
    let mut bytes = Vec::with_capacity(w * h * 4);
    for p in &img.pixels {
        bytes.extend_from_slice(&[p.r(), p.g(), p.b(), 255]);
    }
    let dir = snapshot_dir();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join(format!("planner-{}.png", Local::now().format("%Y-%m-%d_%H%M%S")));
    image::save_buffer(&path, &bytes, w as u32, h as u32, image::ColorType::Rgba8).map_err(|e| e.to_string())?;
    Ok(path)
}

pub fn copy_to_clipboard(path: &std::path::Path) {
    let script = format!("set the clipboard to (read (POSIX file \"{}\") as \u{ab}class PNGf\u{bb})", path.display());
    let _ = std::process::Command::new("osascript").args(["-e", &script]).output();
}

pub fn reveal(path: &std::path::Path) {
    let _ = std::process::Command::new("open").arg("-R").arg(path).spawn();
}
