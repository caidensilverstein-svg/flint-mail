// Global Cmd+Shift+B hotkey and planner snapshot helpers.
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use chrono::Local;
use eframe::egui;

pub struct Hotkey {
    _manager: global_hotkey::GlobalHotKeyManager,
    pub fired: Arc<AtomicBool>,
}

pub fn register(ctx: &egui::Context) -> Result<Hotkey, String> {
    use global_hotkey::hotkey::{Code, HotKey, Modifiers};
    use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};
    let manager = GlobalHotKeyManager::new().map_err(|e| e.to_string())?;
    let hk = HotKey::new(Some(Modifiers::SUPER | Modifiers::SHIFT), Code::KeyB);
    manager.register(hk).map_err(|e| e.to_string())?;
    let fired = Arc::new(AtomicBool::new(false));
    let flag = fired.clone();
    let ctx = ctx.clone();
    std::thread::spawn(move || {
        let rx = GlobalHotKeyEvent::receiver();
        while let Ok(ev) = rx.recv() {
            if ev.id == hk.id() && ev.state == HotKeyState::Pressed {
                flag.store(true, Ordering::SeqCst);
                ctx.request_repaint();
            }
        }
    });
    Ok(Hotkey { _manager: manager, fired })
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
