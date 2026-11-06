mod ai;
mod app;
mod data;
mod groq;
mod home;
mod insights;
mod ics;
mod mac;
mod mechanics;
mod menumodel;
mod net;
mod snap;
mod templates;
mod theme;
#[cfg(target_os = "macos")]
mod tray;

use eframe::egui;
use std::path::PathBuf;

fn main() -> eframe::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let screenshot = args
        .iter()
        .position(|a| a == "--screenshot")
        .and_then(|i| args.get(i + 1))
        .map(PathBuf::from);
    let start_calendar = args.iter().any(|a| a == "--calendar");
    let with_tray = args.iter().any(|a| a == "--with-tray");

    let options = eframe::NativeOptions {
        persist_window: true,
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 800.0])
            .with_min_inner_size([960.0, 620.0])
            .with_title("Flint Mail"),
        ..Default::default()
    };
    eframe::run_native(
        "Flint Mail",
        options,
        Box::new(move |cc| Ok(Box::new(app::FlintApp::new(cc, screenshot, start_calendar, with_tray)))),
    )
}
