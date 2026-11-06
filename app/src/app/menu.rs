// Menu bar content from app state, and what its items do. Not macOS-gated so it
// is type-checked everywhere; only tray.rs itself is Mac-only.
use super::home::task_key;
use super::*;
use crate::insights;
use crate::menumodel::{self as mm, Do};

impl FlintApp {
    pub(super) fn menu_model(&self, now: DateTime<Local>) -> mm::Model {
        let needs = self.home_cards(now, 3).into_iter().map(|c| mm::NeedRow { id: c.id, title: c.title }).collect();
        let events = self
            .upcoming_events(now)
            .into_iter()
            .filter(|e| e.start.date_naive() == now.date_naive() || e.start - now < Duration::hours(12))
            .take(4)
            .map(|e| mm::EventRow { id: e.id, title: e.title.clone(), start: e.start, end: e.end, has_link: e.link.is_some() })
            .collect();
        let today = now.date_naive();
        let mut due: Vec<&mac::Task> = self.tasks.iter().filter(|t| t.due.map_or(false, |d| d.date_naive() <= today)).collect();
        due.sort_by_key(|t| t.due);
        let tasks_total = due.len();
        let tasks = due.into_iter().take(5).map(|t| mm::TaskRow { key: task_key(t), title: t.title.clone(), overdue: t.due.map_or(false, |d| d.date_naive() < today) }).collect();
        let mut unread: Vec<&Mail> = self.mails.iter().filter(|m| !m.read && self.in_folder(m, Folder::SmartInbox)).collect();
        unread.sort_by(|a, b| b.received.cmp(&a.received));
        let top_unread = unread.iter().take(3).map(|m| mm::MailRow { id: m.id, from: m.from_name.clone(), subject: m.subject.clone() }).collect();
        mm::build(&mm::Inputs {
            now,
            unread: self.unread_count(),
            needs,
            events,
            tasks,
            tasks_total,
            top_unread,
            quiet_until: mac::quiet_until(),
            sync_error: self.last_sync_error.clone(),
            last_sync: Some(now - Duration::seconds(self.last_mail_sync.elapsed().as_secs() as i64)),
            syncing: !self.syncing.is_empty() || self.cal_syncing,
            style: mm::TitleStyle::from_key(&self.settings.menu_title),
            sections: mm::Sections { needs: self.settings.menu_needs, today: self.settings.menu_today, unread: self.settings.menu_unread, quick: self.settings.menu_quick, clipboard: self.settings.menu_clipboard },
        })
    }

    fn ask_async(&self, kind: &'static str, title: &'static str, prompt: &'static str, ctx: &egui::Context) {
        let tx = self.net_tx.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            if let Some(t) = mac::ask_text(title, prompt) {
                let _ = tx.send(NetResult::MenuText(kind, t));
                ctx.request_repaint();
            }
        });
    }

    fn clip_action(&mut self, kind: &'static str, ctx: &egui::Context) {
        let text = mac::clipboard_text();
        if text.is_empty() {
            mac::notify("Flint Mail", "The clipboard has no text.");
            return;
        }
        let prompt = insights::build_clip_prompt(&text, Local::now().date_naive());
        self.clip_text = text;
        self.run_ai(ctx, insights::SYSTEM, prompt, String::new(), Target::Clip(kind));
    }

    pub(super) fn add_task_text(&mut self, text: &str) {
        let (title, due) = parse_quick_task(text.trim(), Local::now());
        self.add_task(title, due);
    }

    pub(super) fn add_task(&mut self, title: String, due: Option<DateTime<Local>>) {
        self.tasks.push(mac::Task { id: String::new(), list: "Reminders".into(), title: title.clone(), due, flagged: false });
        let tx = self.net_tx.clone();
        std::thread::spawn(move || {
            if mac::add_reminder(&title, due) {
                let _ = tx.send(NetResult::Tasks(mac::planner().map(|d| d.tasks)));
            }
        });
        self.home_cache = None;
        self.last_tray_refresh = Local::now() - Duration::days(1);
    }

    pub(super) fn add_event_text(&mut self, text: &str) {
        let now = Local::now();
        let (title, when) = parse_quick_task(text.trim(), now);
        let start = when.unwrap_or_else(|| mech::round_up_half_hour(now));
        self.add_event_at(title, start, start + Duration::hours(1));
    }

    pub(super) fn add_event_at(&mut self, title: String, start: DateTime<Local>, end: DateTime<Local>) {
        let cal = self.calendars.first().map(|c| c.name.clone()).unwrap_or_default();
        let t = title.clone();
        std::thread::spawn(move || {
            mac::add_event(&cal, &t, start, end, "", "");
        });
        self.refresh_planner = true;
        self.toast(format!("Added: {}", title));
    }

    pub(super) fn finish_clip(&mut self, kind: &'static str, text: &str, used_model: bool) {
        let clip = std::mem::take(&mut self.clip_text);
        let first_line: String = clip.lines().next().unwrap_or("").chars().take(90).collect();
        let parsed = if used_model { insights::parse(text) } else { None };
        match kind {
            "task" => {
                let t = parsed.as_ref().and_then(|p| p.task.clone());
                match t {
                    Some(t) => self.add_task(t.title, insights::parse_when(&t.due)),
                    None => self.add_task_text(&first_line),
                }
                mac::notify("Flint Mail", "Task added from clipboard");
            }
            "event" => {
                match parsed.and_then(|p| p.event) {
                    Some(e) => {
                        if let Some(s) = insights::parse_when(&e.start) {
                            let end = insights::parse_when(&e.end).filter(|x| *x > s).unwrap_or(s + Duration::hours(1));
                            self.add_event_at(e.title, s, end);
                        }
                    }
                    None => self.add_event_text(&first_line),
                }
                mac::notify("Flint Mail", "Event added from clipboard");
            }
            _ => {
                let summary = parsed.map(|p| p.summary).filter(|s| !s.is_empty()).unwrap_or_else(|| ai::sentences(&clip).into_iter().take(2).collect::<Vec<_>>().join(" "));
                mac::notify("Clipboard summary", &summary.chars().take(220).collect::<String>());
                self.toast(summary);
            }
        }
    }

    pub(super) fn run_menu_action(&mut self, a: Do, ctx: &egui::Context) {
        let now = Local::now();
        match a {
            Do::OpenApp => self.mode = Mode::Home,
            Do::NewEmail => self.composer = Some(self.fresh_composer()),
            Do::Calendar => self.mode = Mode::Calendar,
            Do::Capture => self.begin_capture(ctx, crate::snap::capture_screen()),
            Do::AskAi => {
                self.mode = Mode::Mail;
                self.copilot_open = true;
            }
            Do::Settings => self.settings_open = true,
            Do::Quit => {
                self.quitting = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            Do::SyncNow => {
                for acc in self.settings.accounts.clone() {
                    self.sync_account(&acc, ctx);
                }
                self.refresh_planner = true;
                self.last_mail_sync = std::time::Instant::now();
            }
            Do::QuickTask => self.ask_async("task", "Quick Task", "What do you need to do? (try: call Mom tomorrow at 3pm)", ctx),
            Do::QuickEvent => self.ask_async("event", "Quick Event", "What is it and when? (try: lunch with Sam friday 1pm)", ctx),
            Do::ClipTask => self.clip_action("task", ctx),
            Do::ClipEvent => self.clip_action("event", ctx),
            Do::ClipSummary => self.clip_action("summary", ctx),
            Do::Quiet(m) => {
                let until = if m == 0 {
                    0
                } else if m < 0 {
                    mech::at(now.date_naive() + Duration::days(1), 8, 0).timestamp()
                } else {
                    (now + Duration::minutes(m)).timestamp()
                };
                mac::set_quiet_until(until);
                self.toast(if m == 0 { "Notifications resumed" } else { "Notifications paused" });
            }
            Do::Card(id) => {
                if let Some(c) = self.home_cards(now, 12).into_iter().find(|c| c.id == id) {
                    self.home_primary(&c, ctx);
                }
            }
            Do::Join(id) => {
                let link = self.events.iter().find(|e| e.id == id).and_then(|e| e.link.clone());
                match link {
                    Some(l) => {
                        let _ = mac::open_url(&l);
                    }
                    None => {
                        self.mode = Mode::Calendar;
                        self.selected_event = Some(id);
                    }
                }
            }
            Do::Mail(id) => self.open_mail(id, ctx),
            Do::TaskDone(key) => self.complete_task_key(&key),
            Do::Event(id) => {
                self.mode = Mode::Calendar;
                self.selected_event = Some(id);
                if let Some(e) = self.events.iter().find(|e| e.id == id) {
                    self.cal_anchor = e.start.date_naive();
                }
            }
        }
        self.last_tray_refresh = now - Duration::days(1);
    }
}
