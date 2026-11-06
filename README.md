# Flint Mail

A Spark-style mail and calendar app for macOS, written in Rust with eframe/egui.

- Opens on Needs You: a short ranked list of replies waiting, meetings, tasks due, events and deadlines found in mail, with one button each
- Mail over IMAP/SMTP (Gmail app password), local cache, drafts, undo, snooze (type "next monday 9am"), pins, Today folder
- Planner: day, 3-day, week and month calendar, tasks from Reminders, quick-add with dates ("call Mom friday 3:30pm", "in 2 days", "next week")
- Cmd+Shift+B anywhere: capture your screen into tasks, or snapshot the planner
- Menu bar item: join your next meeting, tick off tasks, quick task/event, make a task or event from the clipboard, pause notifications
- AI: Groq (fast, streaming) with a key, else Claude with a key, else local Ollama (qwen3.5:9b), else simple on-device versions
- Email analysis in the background: add-to-calendar banner, suggested tasks, suggested replies, key facts
- Templates: searchable picker, 20 starters, type / in a draft to insert one, save any draft as a template

## Shortcuts

Press `?` in the app for the full list. Highlights: Cmd+0 (Needs You), Cmd+Shift+B (capture screen to tasks, works from any app), Cmd+K (commands), Cmd+N (new email), Cmd+Z (undo done or delete), j/k/e/p/r/c/h/u in the inbox, t/n/d/w and arrows in the calendar.

## AI setup

Settings > AI. Paste a Groq key (kept in the Mac Keychain, never in a file) and press Test. Or set `GROQ_API_KEY`. With a cloud key, Flint also reads new mail in the background to find events, tasks and replies; turn that off in the same tab. Without a key everything still works with simple on-device versions.

## Mail search

`from:` `to:` `subject:` `label:` `is:unread|read|pinned|priority|done` `has:attachment|comment`, and a leading `-` excludes (for example `-label:work`).

## Build and run (macOS)

    ./morning.sh

`morning.sh` builds, keeps the previous app next to it, then runs `package.sh` (which signs, installs and opens the app).

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
