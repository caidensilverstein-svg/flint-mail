# Flint Mail

A Spark-style mail and calendar app for macOS, written in Rust with eframe/egui.

- Mail over IMAP/SMTP (Gmail app password), local cache, drafts, undo, snooze, pins, Today folder
- Planner: day, 3-day, week and month calendar, tasks from Reminders, quick-add with dates
- Cmd+Shift+B anywhere: snapshot of the planner and tasks, saved to ~/Pictures/Flint Planner and copied
- Local AI through Ollama (qwen3.5:9b), optional Anthropic key

## Shortcuts

Press `?` in the app for the full list. Highlights: Cmd+Shift+B (planner snapshot, works from any app), Cmd+K (commands), Cmd+N (new email), Cmd+Z (undo done or delete), j/k/e/p/r/c/h/u in the inbox, t/n/d/w and arrows in the calendar.

## Mail search

`from:` `to:` `subject:` `label:` `is:unread|read|pinned|priority|done` `has:attachment|comment`, and a leading `-` excludes (for example `-label:work`).

## Build and run (macOS)

    ./package.sh

Builds the app and the Calendar helper, signs them with your Apple Development identity
(set `FLINT_SIGN_ID` to override) and installs `~/Applications/Flint Mail.app`.

Calendar and Reminders access goes through `Flint Calendar Helper.app`. Allow it in
System Settings > Privacy & Security.

## Layout

- `app/` the Rust crate (`src/app.rs` is most of the UI)
- `helper/` Swift EventKit helper and its Info.plists
- `package.sh` build, sign, relaunch

## Checking code off a Mac

`cd app && cargo check` and `cargo test` work on Linux. The GUI and calendar access need macOS.
