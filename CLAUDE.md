# Flint Mail

Rust + eframe/egui 0.29 macOS app. Crate in `app/` (`src/app.rs` holds most UI), Swift EventKit helper in `helper/`.

- Check code with `cd app && cargo check` and `cargo test`. Off a Mac, never run the GUI.
- macOS-only code is behind `cfg(target_os = "macos")` or uses `std::process::Command`.
- Keep UI text free of emojis. Comment only non-obvious why.
- Commit small, one change at a time.
