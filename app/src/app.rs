use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;

use chrono::{DateTime, Datelike, Duration, Local, NaiveDate, Timelike};
use eframe::egui::{
    self, text::LayoutJob, Align, Align2, Color32, FontId, Galley, Key, Layout, RichText, Rounding, Sense, Stroke,
    TextFormat, Ui,
};

use crate::ai::{self, AiResult, Target, Tone};
use crate::ics;
use crate::data::{self, Account, CalendarInfo, Category, Comment, Event, Mail, Scheduled, SentMail, Settings};
use crate::mac::{self, Integration};
use crate::mechanics::{self as mech, Preset};
use crate::net::{self, NetResult};
use crate::theme::{self, Pal, ACCENT, GREEN, ORANGE, RED};

#[derive(Clone, Serialize, Deserialize, Default)]
struct MailState {
    pinned: bool,
    priority: bool,
    done: bool,
    trashed: bool,
    set_aside: bool,
    snoozed_until: Option<DateTime<Local>>,
    someday: bool,
    read: bool,
    labels: Vec<String>,
}

fn mail_key(account: &str, from_addr: &str, received: DateTime<Local>) -> String {
    format!("{}|{}|{}", account, from_addr, received.timestamp())
}

fn mail_state_path() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("Flint Mail").join("mail-state.json"))
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Mode {
    Mail,
    Calendar,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Folder {
    SmartInbox,
    Today,
    AllInboxes,
    Priority,
    Pinned,
    Gatekeeper,
    SetAside,
    Snoozed,
    FollowUps,
    Shared,
    Assigned,
    Scheduled,
    Sent,
    Done,
    Trash,
}

impl Folder {
    fn name(self) -> &'static str {
        match self {
            Folder::SmartInbox => "Smart Inbox",
            Folder::Today => "Today",
            Folder::AllInboxes => "All Inboxes",
            Folder::Priority => "Priority",
            Folder::Pinned => "Pinned",
            Folder::Gatekeeper => "New Senders",
            Folder::SetAside => "Set Aside",
            Folder::Snoozed => "Snoozed",
            Folder::FollowUps => "Follow-ups",
            Folder::Shared => "Shared with Team",
            Folder::Assigned => "Assigned to Me",
            Folder::Scheduled => "Send Later",
            Folder::Sent => "Sent",
            Folder::Done => "Done",
            Folder::Trash => "Trash",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Section {
    Priority,
    People,
    Notifications,
    Newsletters,
    Pins,
    Seen,
}

impl Section {
    // Order estimated; see MECHANICS.md.
    const ORDER: [Section; 6] = [
        Section::Priority,
        Section::People,
        Section::Notifications,
        Section::Newsletters,
        Section::Pins,
        Section::Seen,
    ];
    fn name(self) -> &'static str {
        match self {
            Section::Priority => "Priority",
            Section::People => "People",
            Section::Notifications => "Notifications",
            Section::Newsletters => "Newsletters",
            Section::Pins => "Pins",
            Section::Seen => "Seen",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum CalView {
    Day,
    ThreeDays,
    Week,
    Month,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum SettingsTab {
    General,
    Accounts,
    Ai,
    MenuBar,
    Integrations,
    Templates,
    Plan,
}

struct Composer {
    from: usize,
    to: String,
    to_name: String,
    subject: String,
    body: String,
    reply_to: Option<u64>,
    read_receipt: bool,
    follow_up_days: Option<i64>,
    ai_prompt: String,
    ai_busy: bool,
    ai_note: String,
    show_ai: bool,
    previous_body: Option<String>,
    attachments: Vec<PathBuf>,
    focus_top: bool,
}

impl Composer {
    fn blank() -> Self {
        Composer {
            from: 0,
            to: String::new(),
            to_name: String::new(),
            subject: String::new(),
            body: String::new(),
            reply_to: None,
            read_receipt: false,
            follow_up_days: None,
            ai_prompt: String::new(),
            ai_busy: false,
            ai_note: String::new(),
            show_ai: true,
            previous_body: None,
            attachments: Vec::new(),
            focus_top: false,
        }
    }
}

struct EventForm {
    title: String,
    date: String,
    time: String,
    minutes: i64,
    calendar: usize,
    location: String,
    link: String,
    attendees: String,
    editing: Option<u64>,
}

#[derive(Default)]
struct AccountForm {
    name: String,
    email: String,
    imap_host: String,
    imap_port: String,
    smtp_host: String,
    smtp_port: String,
    username: String,
    password: String,
    error: String,
}

struct PendingSend {
    item: Scheduled,
    deadline: DateTime<Local>,
}

struct Toast {
    text: String,
    until: DateTime<Local>,
}

enum Act {
    Select(u64),
    ToggleRead(u64),
    Pin(u64),
    Done(u64),
    Trash(u64),
    Priority(u64),
    SetAside(u64),
    Snooze(u64, Preset),
    Unsnooze(u64),
    Accept(String),
    Block(String),
    Reply(u64, bool),
    QuickReply(u64, &'static str),
    Forward(u64),
    Summarize(u64),
    MarkSectionRead(Section),
    DoneSection(Section),
    Unsubscribe(u64),
    Export(u64, Integration),
    Label(u64, String),
    Share(u64),
    Assign(u64, String),
    Comment(u64),
    CopyText(String),
    Expand(&'static str),
    Go(Folder),
}

pub struct FlintApp {
    mails: Vec<Mail>,
    sent: Vec<SentMail>,
    scheduled: Vec<Scheduled>,
    events: Vec<Event>,
    calendars: Vec<CalendarInfo>,
    settings: Settings,
    ai_key: String,
    ai_key_saved: bool,

    mode: Mode,
    folder: Folder,
    selected: Option<u64>,
    selected_event: Option<u64>,
    search: String,
    unread_only: bool,
    expanded: HashSet<&'static str>,
    composer: Option<Composer>,
    cmd_open: bool,
    cmd_query: String,
    settings_open: bool,
    settings_tab: SettingsTab,
    account_form: Option<AccountForm>,
    event_form: Option<EventForm>,
    copilot_open: bool,
    copilot_input: String,
    copilot_log: Vec<(bool, String)>,
    copilot_busy: bool,
    comment_input: String,
    cal_view: CalView,
    cal_anchor: NaiveDate,
    toasts: Vec<Toast>,
    pending: Option<PendingSend>,
    syncing: HashSet<String>,
    cal_syncing: bool,
    last_cal_sync: std::time::Instant,
    cal_warning: String,
    tasks: Vec<mac::Task>,
    cal_cache: (Vec<String>, Vec<mac::MacEvent>),
    tasks_syncing: bool,
    task_input: String,
    ical_text: String,
    next_id: u64,

    ai_tx: Sender<AiResult>,
    ai_rx: Receiver<AiResult>,
    net_tx: Sender<NetResult>,
    net_rx: Receiver<NetResult>,

    frame: u64,
    screenshot: Option<PathBuf>,
    shot_requested: bool,
    help_open: bool,
    confirm_delete: Option<u64>,
    last_sync_error: String,
    refresh_planner: bool,
    mail_state: HashMap<String, MailState>,
    last_mail_sync: std::time::Instant,
    draft: Option<Composer>,
    undo_mail: Option<(u64, bool, bool, bool)>,
    last_title_unread: usize,
    snap_stage: u8,
    snap_prev_view: Option<(Mode, CalView)>,
    snap_started: Option<std::time::Instant>,
    snap_wait: u8,
    snap_tex: Option<egui::TextureHandle>,
    snap_path: Option<PathBuf>,
    snap_open: bool,
    #[cfg(target_os = "macos")]
    hotkey: Option<crate::snap::Hotkey>,
    quitting: bool,
    hidden: bool,
    last_tick: DateTime<Local>,
    last_tray_refresh: DateTime<Local>,
    last_tray_unread: usize,
    #[cfg(target_os = "macos")]
    menubar: Option<crate::tray::MenuBar>,
}

fn settings_path() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("Flint Mail").join("settings.json"))
}

impl FlintApp {
    pub fn new(cc: &eframe::CreationContext<'_>, screenshot: Option<PathBuf>, start_calendar: bool, with_tray: bool) -> Self {
        let now = Local::now();
        let shot_mode = screenshot.is_some();
        let settings: Settings = if shot_mode {
            let mut d = Settings::default();
            d.dark = std::env::var_os("FLINT_DARK").is_some();
            d
        } else {
            settings_path()
                .and_then(|p| std::fs::read_to_string(p).ok())
                .and_then(|s| serde_json::from_str(&s).ok())
                .unwrap_or_default()
        };
        let mut settings = settings;
        let mut migrated_now = false;
        if !settings.hotkey_migrated && !shot_mode {
            settings.close_to_tray = true;
            settings.menu_bar = true;
            settings.hotkey_migrated = true;
            migrated_now = true;
        }
        if settings.signature == "Best,\nAlex" && !shot_mode {
            if let Some(first) = settings.accounts.first().and_then(|a| a.name.split_whitespace().next()) {
                settings.signature = format!("Best,\n{}", first);
            }
        }
        theme::apply(&cc.egui_ctx, settings.dark);
        cc.egui_ctx.set_zoom_factor(settings.zoom.clamp(0.8, 1.6));
        let (mails, sent, events) = data::sample(now);
        let (ai_tx, ai_rx) = channel();
        let (net_tx, net_rx) = channel();
        let ai_key = if shot_mode {
            String::new()
        } else {
            std::env::var("ANTHROPIC_API_KEY").ok().or_else(|| mac::keychain_get("ai-key")).unwrap_or_default()
        };
        let saved_cal_view = settings.cal_view.clone();
        let migrated_now = migrated_now;
        let mut app = FlintApp {
            mails,
            sent,
            scheduled: Vec::new(),
            events,
            calendars: data::default_calendars(),
            settings,
            ai_key_saved: !ai_key.is_empty(),
            ai_key,
            mode: Mode::Mail,
            folder: Folder::SmartInbox,
            selected: Some(1),
            selected_event: None,
            search: String::new(),
            unread_only: false,
            expanded: HashSet::new(),
            composer: None,
            cmd_open: false,
            cmd_query: String::new(),
            settings_open: false,
            settings_tab: SettingsTab::General,
            account_form: None,
            event_form: None,
            copilot_open: false,
            copilot_input: String::new(),
            copilot_log: Vec::new(),
            copilot_busy: false,
            comment_input: String::new(),
            cal_view: match saved_cal_view.as_str() {
                "day" => CalView::Day,
                "3days" => CalView::ThreeDays,
                "month" => CalView::Month,
                _ => CalView::Week,
            },
            cal_anchor: now.date_naive(),
            toasts: Vec::new(),
            pending: None,
            syncing: HashSet::new(),
            cal_syncing: false,
            last_cal_sync: std::time::Instant::now(),
            cal_warning: String::new(),
            tasks: Vec::new(),
            cal_cache: (Vec::new(), Vec::new()),
            tasks_syncing: false,
            task_input: String::new(),
            ical_text: if shot_mode { String::new() } else { mac::keychain_get("ical-urls").unwrap_or_default() },
            next_id: 10_000,
            ai_tx,
            ai_rx,
            net_tx,
            net_rx,
            frame: 0,
            screenshot,
            shot_requested: false,
            help_open: false,
            confirm_delete: None,
            last_sync_error: String::new(),
            refresh_planner: false,
            mail_state: if shot_mode { HashMap::new() } else { mail_state_path().and_then(|p| std::fs::read_to_string(p).ok()).and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default() },
            last_mail_sync: std::time::Instant::now(),
            draft: None,
            undo_mail: None,
            last_title_unread: usize::MAX,
            snap_stage: 0,
            snap_prev_view: None,
            snap_started: None,
            snap_wait: 0,
            snap_tex: None,
            snap_path: None,
            snap_open: false,
            #[cfg(target_os = "macos")]
            hotkey: None,
            quitting: false,
            hidden: false,
            last_tick: now - Duration::seconds(10),
            last_tray_refresh: now - Duration::days(1),
            last_tray_unread: usize::MAX,
            #[cfg(target_os = "macos")]
            menubar: None,
        };
        if shot_mode && std::env::var_os("FLINT_EVENT_DETAIL").is_some() {
            app.selected_event = app.events.iter().find(|e| e.link.is_some()).or(app.events.first()).map(|e| e.id);
        }
        if shot_mode && std::env::var_os("FLINT_EVENT_FORM").is_some() {
            app.event_form = Some(app.new_event_form(None));
        }
        if shot_mode && std::env::var_os("FLINT_REPLY").is_some() {
            let id = app.mails.first().map(|m| m.id).unwrap_or(1);
            app.apply(vec![Act::Reply(id, true)], &cc.egui_ctx);
        }
        if shot_mode && std::env::var_os("FLINT_COMPOSE").is_some() {
            app.composer = Some(Composer::blank());
        }
        if shot_mode && std::env::var_os("FLINT_SETTINGS").is_some() {
            app.settings_open = true;
            match std::env::var("FLINT_SETTINGS").as_deref() {
                Ok("ai") => app.settings_tab = SettingsTab::Ai,
                Ok("accounts") => app.settings_tab = SettingsTab::Accounts,
                Ok("menubar") => app.settings_tab = SettingsTab::MenuBar,
                _ => {}
            }
        }
        if shot_mode {
            let mk = |t: &str, d: Option<DateTime<Local>>| mac::Task { id: String::new(), list: "Reminders".into(), title: t.into(), due: d, flagged: false };
            app.tasks = vec![
                mk("Send the Q1 proposal", Some(now - Duration::days(1))),
                mk("Review launch plan", Some(now)),
                mk("Book dentist follow-up", Some(now + Duration::days(3))),
                mk("Renew domain", Some(now + Duration::days(20))),
                mk("Read the onboarding doc", None),
            ];
        }
        if migrated_now {
            app.save_settings();
        }
        if app.settings.ai_auto_labels {
            app.auto_label_all();
        }
        if start_calendar {
            app.mode = Mode::Calendar;
            match std::env::var("FLINT_CAL_VIEW").as_deref() {
                Ok("month") => app.cal_view = CalView::Month,
                Ok("day") => app.cal_view = CalView::Day,
                Ok("3days") => app.cal_view = CalView::ThreeDays,
                _ => {}
            }
        }
        #[cfg(target_os = "macos")]
        if shot_mode && with_tray {
            match crate::tray::MenuBar::new(&cc.egui_ctx) {
                Ok(mb) => {
                    app.menubar = Some(mb);
                    eprintln!("menu bar item created");
                }
                Err(e) => eprintln!("menu bar item failed: {}", e),
            }
        }
        let _ = with_tray;
        if !shot_mode {
            #[cfg(target_os = "macos")]
            {
                app.hotkey = crate::snap::register(&cc.egui_ctx).ok();
            }
            #[cfg(target_os = "macos")]
            if app.settings.menu_bar {
                app.menubar = crate::tray::MenuBar::new(&cc.egui_ctx).ok();
            }
            let accounts = app.settings.accounts.clone();
            if !accounts.is_empty() {
                app.drop_samples();
            }
            for acc in accounts {
                let cached = net::load_cache(&acc.email).mails;
                if !cached.is_empty() {
                    app.apply_remote(&acc.email, cached);
                }
                app.sync_account(&acc, &cc.egui_ctx);
            }
            let cache = mac::load_planner_cache();
            app.tasks = cache.tasks;
            if !cache.names.is_empty() {
                let _ = app.net_tx.send(NetResult::Calendar { names: cache.names, events: cache.events, warnings: Vec::new() });
            }
            app.sync_calendar(&cc.egui_ctx);
            if !mac::has_helper() {
                app.sync_tasks(&cc.egui_ctx);
            }
        }
        app
    }

    fn save_settings(&self) {
        if self.screenshot.is_some() {
            return;
        }
        if let Some(p) = settings_path() {
            if let Some(dir) = p.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            if let Ok(s) = serde_json::to_string_pretty(&self.settings) {
                let _ = std::fs::write(p, s);
            }
        }
    }

    fn toast(&mut self, text: impl Into<String>) {
        self.toasts.push(Toast { text: text.into(), until: Local::now() + Duration::seconds(4) });
    }

    fn id(&mut self) -> u64 {
        self.next_id += 1;
        self.next_id
    }

    fn accounts(&self) -> Vec<String> {
        let mut v: Vec<String> = if self.settings.accounts.is_empty() {
            data::SAMPLE_ACCOUNTS.iter().map(|s| s.to_string()).collect()
        } else {
            Vec::new()
        };
        for a in &self.settings.accounts {
            v.push(a.email.clone());
        }
        v
    }

    fn real_account(&self, email: &str) -> Option<Account> {
        self.settings.accounts.iter().find(|a| a.email == email).cloned()
    }

    fn mail(&self, id: u64) -> Option<&Mail> {
        self.mails.iter().find(|m| m.id == id)
    }

    fn mail_mut(&mut self, id: u64) -> Option<&mut Mail> {
        self.mails.iter_mut().find(|m| m.id == id)
    }

    fn blocked(&self, m: &Mail) -> bool {
        self.settings.blocked.iter().any(|b| b.eq_ignore_ascii_case(&m.from_addr))
    }

    fn active(&self, m: &Mail) -> bool {
        !m.trashed && !self.blocked(m) && m.snoozed_until.is_none() && !m.someday
    }

    fn in_folder(&self, m: &Mail, f: Folder) -> bool {
        let gate = self.settings.gatekeeper && m.new_sender;
        match f {
            Folder::SmartInbox | Folder::AllInboxes => self.active(m) && !m.done && !m.set_aside && !gate,
            Folder::Today => self.active(m) && !m.done && m.received.date_naive() == Local::now().date_naive(),
            Folder::Priority => self.active(m) && m.priority && !m.done,
            Folder::Pinned => !m.trashed && m.pinned,
            Folder::Gatekeeper => !m.trashed && m.new_sender && !self.blocked(m),
            Folder::SetAside => !m.trashed && m.set_aside && !m.done,
            Folder::Snoozed => !m.trashed && (m.snoozed_until.is_some() || m.someday),
            Folder::Shared => !m.trashed && (m.shared || !m.comments.is_empty()),
            Folder::Assigned => !m.trashed && m.assigned.as_deref() == Some("Me"),
            Folder::Done => !m.trashed && m.done,
            Folder::Trash => m.trashed,
            Folder::Scheduled | Folder::Sent | Folder::FollowUps => false,
        }
    }

    fn matches_search(&self, m: &Mail) -> bool {
        if self.unread_only && m.read {
            return false;
        }
        let q = self.search.trim().to_lowercase();
        if q.is_empty() {
            return true;
        }
        q.split_whitespace().all(|tok| {
            let (neg, tok) = match tok.strip_prefix('-') {
                Some(rest) if rest.contains(':') || rest.len() > 1 => (true, rest),
                _ => (false, tok),
            };
            let hit = if let Some(v) = tok.strip_prefix("from:") {
                m.from_name.to_lowercase().contains(v) || m.from_addr.to_lowercase().contains(v)
            } else if let Some(v) = tok.strip_prefix("to:") {
                m.to.to_lowercase().contains(v)
            } else if let Some(v) = tok.strip_prefix("subject:") {
                m.subject.to_lowercase().contains(v)
            } else if let Some(v) = tok.strip_prefix("label:") {
                m.labels.iter().any(|l| l.to_lowercase() == v)
            } else if tok == "is:unread" {
                !m.read
            } else if tok == "is:read" {
                m.read
            } else if tok == "is:pinned" {
                m.pinned
            } else if tok == "is:priority" {
                m.priority
            } else if tok == "is:done" {
                m.done
            } else if tok == "has:attachment" {
                !m.attachments.is_empty()
            } else if tok == "has:comment" {
                !m.comments.is_empty()
            } else {
                format!("{} {} {} {}", m.from_name, m.from_addr, m.subject, m.body).to_lowercase().contains(tok)
            };
            hit != neg
        })
    }

    fn section_of(&self, m: &Mail) -> Section {
        if m.priority {
            Section::Priority
        } else if m.pinned {
            Section::Pins
        } else if m.read {
            Section::Seen
        } else {
            match m.category {
                Category::People => Section::People,
                Category::Notification => Section::Notifications,
                Category::Newsletter => Section::Newsletters,
            }
        }
    }

    fn visible_mails(&self, f: Folder) -> Vec<&Mail> {
        let searching = !self.search.trim().is_empty();
        let mut v: Vec<&Mail> = self
            .mails
            .iter()
            .filter(|m| if searching { !m.trashed } else { self.in_folder(m, f) })
            .filter(|m| self.matches_search(m))
            .collect();
        v.sort_by(|a, b| b.received.cmp(&a.received));
        v
    }

    fn unread_count(&self) -> usize {
        self.mails.iter().filter(|m| !m.read && self.in_folder(m, Folder::SmartInbox)).count()
    }

    fn folder_count(&self, f: Folder) -> usize {
        match f {
            Folder::Scheduled => self.scheduled.len(),
            Folder::FollowUps => self.due_follow_ups().len(),
            Folder::SmartInbox | Folder::AllInboxes => self.unread_count(),
            Folder::Today => self.mails.iter().filter(|m| !m.read && self.in_folder(m, f)).count(),
            Folder::Pinned | Folder::SetAside => self.mails.iter().filter(|m| self.in_folder(m, f)).count(),
            Folder::Gatekeeper | Folder::Priority | Folder::Assigned => {
                self.mails.iter().filter(|m| self.in_folder(m, f)).count()
            }
            _ => 0,
        }
    }

    fn due_follow_ups(&self) -> Vec<&SentMail> {
        let now = Local::now();
        self.sent.iter().filter(|s| !s.replied && s.follow_up.map_or(false, |t| t <= now)).collect()
    }

    fn upcoming_events(&self, now: DateTime<Local>) -> Vec<&Event> {
        let mut v: Vec<&Event> = self
            .events
            .iter()
            .filter(|e| self.calendars.get(e.calendar).map_or(true, |c| c.visible))
            .filter(|e| e.end > now && !e.all_day)
            .collect();
        v.sort_by_key(|e| e.start);
        v
    }

    fn auto_label_all(&mut self) {
        for m in self.mails.iter_mut() {
            if let Some(l) = ai::suggest_label(m) {
                if !m.labels.iter().any(|x| x == l) {
                    m.labels.push(l.to_string());
                }
            }
        }
    }

    // ---------- background work ----------

    fn drop_samples_mail(&mut self) {
        self.mails.retain(|m| !data::SAMPLE_ACCOUNTS.contains(&m.account.as_str()));
        self.sent.retain(|m| !data::SAMPLE_ACCOUNTS.contains(&m.account.as_str()));
    }

    fn drop_samples(&mut self) {
        self.mails.retain(|m| !data::SAMPLE_ACCOUNTS.contains(&m.account.as_str()));
        self.sent.retain(|m| !data::SAMPLE_ACCOUNTS.contains(&m.account.as_str()));
        self.events.clear();
    }

    fn save_planner(&self) {
        mac::save_planner_cache(&mac::PlannerCache { names: self.cal_cache.0.clone(), events: self.cal_cache.1.clone(), tasks: self.tasks.clone() });
    }

    fn sync_tasks(&mut self, ctx: &egui::Context) {
        if self.tasks_syncing || self.screenshot.is_some() {
            return;
        }
        self.tasks_syncing = true;
        let tx = self.net_tx.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let res = if mac::has_helper() { mac::planner().map(|d| d.tasks) } else { mac::reminders() };
            let _ = tx.send(NetResult::Tasks(res));
            ctx.request_repaint();
        });
    }

    fn sync_calendar(&mut self, ctx: &egui::Context) {
        if self.cal_syncing || self.screenshot.is_some() {
            return;
        }
        self.cal_syncing = true;
        self.last_cal_sync = std::time::Instant::now();
        let tx = self.net_tx.clone();
        let ctx = ctx.clone();
        let urls: Vec<String> = self.ical_text.lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect();
        std::thread::spawn(move || {
            let mut names: Vec<String> = Vec::new();
            let mut events = Vec::new();
            let mut warnings = Vec::new();
            if mac::has_helper() {
                match mac::planner() {
                    Ok(d) => {
                        names.extend(d.names);
                        events.extend(d.events);
                        if names.is_empty() && d.errors.iter().any(|e| e.starts_with("Calendar")) {
                            match mac::calendar_events() {
                                Ok((n, e)) => {
                                    names.extend(n);
                                    events.extend(e);
                                }
                                Err(e) => warnings.push(format!("Mac Calendar: {}", e)),
                            }
                        } else {
                            warnings.extend(d.errors.iter().filter(|e| e.starts_with("Calendar")).cloned());
                        }
                        warnings.extend(d.errors.iter().filter(|e| !e.starts_with("Calendar")).cloned());
                        let _ = tx.send(NetResult::Tasks(Ok(d.tasks)));
                    }
                    Err(e) => warnings.push(format!("Mac Calendar: {}", e)),
                }
            } else {
                match mac::calendar_events() {
                    Ok((n, e)) => {
                        names.extend(n);
                        events.extend(e);
                    }
                    Err(e) => warnings.push(format!("Mac Calendar: {}", e)),
                }
            }
            for url in urls {
                match ics::fetch(&url) {
                    Ok((name, e)) => {
                        if !names.contains(&name) {
                            names.push(name);
                        }
                        events.extend(e);
                    }
                    Err(e) => warnings.push(e),
                }
            }
            let mut seen = HashSet::new();
            events.retain(|e| seen.insert((e.title.clone(), e.start.timestamp(), e.end.timestamp())));
            let _ = tx.send(NetResult::Calendar { names, events, warnings });
            ctx.request_repaint();
        });
    }

    fn apply_remote(&mut self, account: &str, mails: Vec<net::RemoteMail>) {
        let old_ids: HashMap<String, u64> = self.mails.iter().filter(|m| m.account == account).map(|m| (mail_key(&m.account, &m.from_addr, m.received), m.id)).collect();
        self.mails.retain(|m| m.account != account);
                    for rm in mails {
                        let id = match old_ids.get(&mail_key(&rm.account, &rm.from_addr, rm.received)) {
                            Some(id) => *id,
                            None => self.id(),
                        };
                        let key = mail_key(&rm.account, &rm.from_addr, rm.received);
                        let st = self.mail_state.get(&key).cloned();
                        let mut m = Mail {
                            id,
                            account: rm.account,
                            from_name: rm.from_name,
                            from_addr: rm.from_addr,
                            to: rm.to,
                            subject: rm.subject,
                            body: rm.body,
                            received: rm.received,
                            category: rm.category,
                            read: rm.read || st.as_ref().map(|x| x.read).unwrap_or(false),
                            pinned: st.as_ref().map(|x| x.pinned).unwrap_or(false),
                            priority: st.as_ref().map(|x| x.priority).unwrap_or(false),
                            done: st.as_ref().map(|x| x.done).unwrap_or(false),
                            trashed: st.as_ref().map(|x| x.trashed).unwrap_or(false),
                            set_aside: st.as_ref().map(|x| x.set_aside).unwrap_or(false),
                            snoozed_until: st.as_ref().and_then(|x| x.snoozed_until),
                            someday: st.as_ref().map(|x| x.someday).unwrap_or(false),
                            new_sender: false,
                            labels: st.as_ref().map(|x| x.labels.clone()).unwrap_or_default(),
                            attachments: rm.attachments,
                            comments: Vec::new(),
                            assigned: None,
                            shared: false,
                            summary: None,
                            can_unsubscribe: rm.can_unsubscribe,
                            uid: rm.uid,
                        };
                        if self.settings.ai_auto_labels && m.labels.is_empty() {
                            if let Some(l) = ai::suggest_label(&m) {
                                m.labels.push(l.into());
                            }
                        }
                        self.mails.push(m);
                    }
    }

    fn persist_mail_state(&mut self) {
        if self.screenshot.is_some() {
            return;
        }
        for m in self.mails.iter().filter(|m| !data::SAMPLE_ACCOUNTS.contains(&m.account.as_str())) {
            self.mail_state.insert(
                mail_key(&m.account, &m.from_addr, m.received),
                MailState { pinned: m.pinned, priority: m.priority, done: m.done, trashed: m.trashed, set_aside: m.set_aside, snoozed_until: m.snoozed_until, someday: m.someday, read: m.read, labels: m.labels.clone() },
            );
        }
        if self.mail_state.len() > 5000 {
            let live: HashSet<String> = self.mails.iter().map(|m| mail_key(&m.account, &m.from_addr, m.received)).collect();
            self.mail_state.retain(|k, _| live.contains(k));
        }
        if let Some(p) = mail_state_path() {
            if let Ok(t) = serde_json::to_string(&self.mail_state) {
                let _ = std::fs::write(p, t);
            }
        }
    }

    fn sync_account(&mut self, acc: &Account, ctx: &egui::Context) {
        let Some(password) = mac::keychain_get(&acc.email) else {
            self.toast(format!("No saved password for {}", acc.email));
            return;
        };
        self.syncing.insert(acc.email.clone());
        let acc = acc.clone();
        let tx = self.net_tx.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let res = match net::fetch_inbox(&acc, &password) {
                Ok(mails) => NetResult::Synced { account: acc.email.clone(), mails },
                Err(error) => NetResult::SyncFailed { account: acc.email.clone(), error },
            };
            let _ = tx.send(res);
            ctx.request_repaint();
        });
    }

    fn run_ai(&mut self, ctx: &egui::Context, system: &str, prompt: String, fallback: String, target: Target) {
        let key = (!self.ai_key.trim().is_empty()).then(|| self.ai_key.clone());
        ai::spawn(
            key,
            self.settings.ai_model.clone(),
            system.to_string(),
            prompt,
            fallback,
            target,
            self.ai_tx.clone(),
            ctx.clone(),
        );
    }

    fn poll_background(&mut self) {
        while let Ok(r) = self.ai_rx.try_recv() {
            if let Some(e) = &r.error {
                self.toast(format!("{} - used the on-device version instead", e));
            }
            match r.target {
                Target::Summary(id) => {
                    if let Some(m) = self.mail_mut(id) {
                        m.summary = Some(r.text);
                    }
                }
                Target::Composer => {
                    if let Some(c) = self.composer.as_mut() {
                        c.previous_body = Some(c.body.clone());
                        c.body = r.text;
                        c.ai_busy = false;
                        c.ai_note = if r.used_model { "Written with AI".into() } else { "Written on this Mac (add an AI key for better results)".into() };
                    }
                }
                Target::Copilot => {
                    self.copilot_log.push((false, r.text));
                    self.copilot_busy = false;
                }
                Target::EventNotes(id) => {
                    if let Some(e) = self.events.iter_mut().find(|e| e.id == id) {
                        e.notes = format!("{}\n\nSummary:\n{}", e.notes.trim(), r.text);
                    }
                }
                Target::Labels => {}
            }
        }
        while let Ok(r) = self.net_rx.try_recv() {
            match r {
                NetResult::Attached(p) => {
                    if let Some(c) = self.composer.as_mut() {
                        if !c.attachments.contains(&p) {
                            c.attachments.push(p);
                        }
                    }
                }
                NetResult::RefreshPlanner => {
                    self.cal_syncing = false;
                    self.tasks_syncing = false;
                    self.refresh_planner = true;
                }
                NetResult::Tasks(res) => {
                    self.tasks_syncing = false;
                    match res {
                        Ok(mut t) => {
                            t.sort_by(|a, b| match (a.due, b.due) {
                                (Some(x), Some(y)) => x.cmp(&y),
                                (Some(_), None) => std::cmp::Ordering::Less,
                                (None, Some(_)) => std::cmp::Ordering::Greater,
                                _ => a.title.cmp(&b.title),
                            });
                            self.tasks = t;
                            self.save_planner();
                        }
                        Err(e) => self.toast(format!("Reminders: {}", e)),
                    }
                }
                NetResult::Calendar { names, events, warnings } => {
                    self.cal_syncing = false;
                    self.cal_warning = warnings.iter().map(|w| format!("{}.", w.trim_end_matches('.'))).collect::<Vec<_>>().join(" ");
                    if names.is_empty() {
                        self.toast(format!("Couldn't load your calendars. {}", warnings.join(" | ")));
                        continue;
                    }
                    self.cal_cache = (names.clone(), events.clone());
                    self.save_planner();
                    let now = Local::now();
                    self.calendars = names.iter().map(|n| CalendarInfo { name: n.clone(), visible: true }).collect();
                    let was_empty = self.events.is_empty();
                    let notified_before: HashSet<(String, i64)> = self.events.iter().filter(|e| e.notified).map(|e| (e.title.clone(), e.start.timestamp())).collect();
                    self.events.clear();
                    let n = events.len();
                    for e in events {
                        let id = self.id();
                        let calendar = names.iter().position(|c| *c == e.calendar).unwrap_or(0);
                        let link = meeting_link(&e.location).or_else(|| meeting_link(&e.title));
                        let already = notified_before.contains(&(e.title.clone(), e.start.timestamp()));
                        self.events.push(Event {
                            id,
                            title: e.title,
                            start: e.start,
                            end: e.end,
                            calendar,
                            link,
                            location: e.location,
                            attendees: Vec::new(),
                            notes: String::new(),
                            notified: already || e.start < now || e.all_day,
                            all_day: e.all_day,
                        });
                    }
                    if warnings.is_empty() {
                        if was_empty {
                            self.toast(format!("Calendar: {} events loaded", n));
                        }
                    } else {
                        self.toast(format!("Calendar: {} events loaded. {}", n, warnings.join(" | ")));
                    }
                }
                NetResult::Synced { account, mails } => {
                    self.syncing.remove(&account);
                    self.last_sync_error.clear();
                    self.drop_samples_mail();
                    let n = mails.len();
                    let known: HashSet<String> = self.mails.iter().filter(|m| m.account == account).map(|m| mail_key(&m.account, &m.from_addr, m.received)).collect();
                    let first_load = known.is_empty();
                    let fresh: Vec<(String, String)> = mails
                        .iter()
                        .filter(|rm| !rm.read && !known.contains(&mail_key(&rm.account, &rm.from_addr, rm.received)))
                        .map(|rm| (rm.from_name.clone(), rm.subject.clone()))
                        .collect();
                    self.apply_remote(&account, mails);
                    if first_load {
                        self.toast(format!("{}: {} emails synced", account, n));
                    } else if self.settings.notifications && !fresh.is_empty() {
                        for (from, subject) in fresh.iter().take(3) {
                            mac::notify(&format!("New mail from {}", from), subject);
                        }
                    }
                }
                NetResult::SyncFailed { account, error } => {
                    self.syncing.remove(&account);
                    let msg = format!("{}: {}", account, error);
                    if self.last_sync_error != msg {
                        self.last_sync_error = msg.clone();
                        self.toast(msg);
                    }
                }
                NetResult::Sent { to } => self.toast(format!("Sent to {}", to)),
                NetResult::SendFailed { to, error } => self.toast(format!("Couldn't send to {}: {}", to, error)),
            }
        }
    }

    fn tick(&mut self, ctx: &egui::Context) {
        let now = Local::now();
        if (now - self.last_tick).num_seconds() < 1 {
            return;
        }
        self.last_tick = now;
        let notify = self.settings.notifications && self.screenshot.is_none();

        // snoozed emails come back
        let mut back = Vec::new();
        for m in self.mails.iter_mut() {
            if let Some(t) = m.snoozed_until {
                if t <= now {
                    m.snoozed_until = None;
                    m.read = false;
                    back.push(format!("{}: {}", m.from_name, m.subject));
                }
            }
        }
        for b in back {
            if notify {
                mac::notify("Snoozed email is back", &b);
            }
            self.toast(format!("Back from snooze: {}", b));
        }

        // send later
        let due: Vec<Scheduled> = self.scheduled.iter().filter(|s| s.send_at <= now).cloned().collect();
        self.scheduled.retain(|s| s.send_at > now);
        for s in due {
            self.deliver(s, ctx);
        }

        // undo-send window closes
        if let Some(p) = &self.pending {
            if p.deadline <= now {
                let p = self.pending.take().unwrap();
                self.deliver(p.item, ctx);
            }
        }

        // follow-up reminders
        let mut fired = Vec::new();
        for s in self.sent.iter_mut() {
            if !s.replied && !s.follow_up_fired && s.follow_up.map_or(false, |t| t <= now) {
                s.follow_up_fired = true;
                fired.push(format!("No reply yet: {}", s.subject));
            }
        }
        for f in fired {
            if notify {
                mac::notify("Follow up", &f);
            }
        }

        // meeting reminders, one minute ahead
        let mut starting = Vec::new();
        for e in self.events.iter_mut() {
            let lead = (e.start - now).num_seconds();
            let lead_secs = mech::MEETING_LEAD_MS / 1000;
            if !e.notified && lead <= lead_secs && lead > -lead_secs {
                e.notified = true;
                starting.push(e.title.clone());
            }
        }
        for t in starting {
            if notify {
                mac::notify("Starting now", &t);
            }
            self.toast(format!("Starting now: {}", t));
        }

        self.toasts.retain(|t| t.until > now);
        self.update_menubar(now);
    }

    fn deliver(&mut self, s: Scheduled, ctx: &egui::Context) {
        if let Some(acc) = self.real_account(&s.account) {
            if let Some(pw) = mac::keychain_get(&acc.email) {
                let tx = self.net_tx.clone();
                let ctx = ctx.clone();
                let (to, subject, body, files) = (s.to.clone(), s.subject.clone(), s.body.clone(), s.attachments.clone());
                std::thread::spawn(move || {
                    let r = match net::send(&acc, &pw, &to, &subject, &body, &files) {
                        Ok(()) => NetResult::Sent { to },
                        Err(error) => NetResult::SendFailed { to, error },
                    };
                    let _ = tx.send(r);
                    ctx.request_repaint();
                });
            }
        } else {
            self.toast(format!("Sent to {} (sample account)", s.to));
        }
        self.sent.insert(
            0,
            SentMail {
                id: s.id,
                account: s.account,
                to: s.to,
                subject: s.subject,
                body: s.body,
                at: Local::now(),
                read_receipt: s.read_receipt,
                opened: Vec::new(),
                follow_up: s.follow_up,
                follow_up_fired: false,
                replied: false,
            },
        );
    }

    #[allow(unused_variables)]
    fn update_menubar(&mut self, now: DateTime<Local>) {
        #[cfg(target_os = "macos")]
        {
            if self.menubar.is_none() {
                return;
            }
            let unread = self.unread_count();
            // The original refreshes the menu bar agenda once a minute; refresh sooner when the unread count moves.
            let due = (now - self.last_tray_refresh).num_milliseconds() >= mech::TRAY_REFRESH_MS;
            if !due && unread == self.last_tray_unread {
                return;
            }
            self.last_tray_refresh = now;
            self.last_tray_unread = unread;
            let upcoming: Vec<(String, DateTime<Local>, DateTime<Local>)> = self
                .upcoming_events(now)
                .iter()
                .filter(|e| e.start.date_naive() == now.date_naive() || e.start - now < Duration::hours(12))
                .take(3)
                .map(|e| (e.title.clone(), e.start, e.end))
                .collect();
            let mut title = String::new();
            let mut state = crate::tray::IconState::Default;
            if let Some((t, start, end)) = upcoming.first() {
                let in_progress = start.timestamp_millis() - mech::MEETING_LEAD_MS <= now.timestamp_millis() && *end > now;
                if in_progress {
                    state = crate::tray::IconState::MeetingInProgress;
                }
                if self.settings.menu_bar_countdown && (*start - now) < Duration::hours(1) {
                    let short: String = t.chars().take(18).collect();
                    title = format!("{} {}", short, mech::countdown(*start, now));
                }
            }
            if self.settings.menu_bar_unread && unread > 0 {
                title = if title.is_empty() { format!("{}", unread) } else { format!("{}  |  {}", unread, title) };
            }
            let status = if unread == 0 { "Inbox zero".to_string() } else { format!("{} unread in Smart Inbox", unread) };
            let agenda: Vec<String> = upcoming
                .iter()
                .map(|(t, s, _)| format!("{}  {}  ({})", s.format("%-I:%M %p"), t, mech::countdown(*s, now)))
                .collect();
            if let Some(mb) = self.menubar.as_mut() {
                mb.update(&title, &status, &agenda, state);
            }
        }
    }

    #[allow(unused_variables)]
    fn handle_tray(&mut self, ctx: &egui::Context) {
        #[cfg(target_os = "macos")]
        {
            use crate::tray::TrayAction;
            let actions = match &self.menubar {
                Some(mb) => mb.poll(),
                None => return,
            };
            let now = Local::now();
            for a in actions {
                let show = !matches!(a, TrayAction::Quit);
                if show {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                    self.hidden = false;
                }
                match a {
                    TrayAction::Open => {}
                    TrayAction::Agenda => {
                        self.mode = Mode::Calendar;
                        self.cal_view = CalView::Day;
                        self.cal_anchor = now.date_naive();
                        self.selected_event = self.upcoming_events(now).first().map(|e| e.id);
                    }
                    TrayAction::NewEmail => self.composer = Some(self.fresh_composer()),
                    TrayAction::Calendar => self.mode = Mode::Calendar,
                    TrayAction::Snapshot => self.start_snapshot(ctx),
                    TrayAction::AskAi => {
                        self.mode = Mode::Mail;
                        self.copilot_open = true;
                    }
                    TrayAction::Settings => self.settings_open = true,
                    TrayAction::Quit => {
                        self.quitting = true;
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                    TrayAction::Event(i) => {
                        let id = self.upcoming_events(now).get(i).map(|e| e.id);
                        if let Some(id) = id {
                            self.mode = Mode::Calendar;
                            self.selected_event = Some(id);
                            if let Some(e) = self.events.iter().find(|e| e.id == id) {
                                self.cal_anchor = e.start.date_naive();
                            }
                        }
                    }
                }
            }
        }
    }

    // ---------- actions ----------

    fn apply(&mut self, acts: Vec<Act>, ctx: &egui::Context) {
        let had_acts = !acts.is_empty();
        self.apply_inner(acts, ctx);
        if had_acts {
            self.persist_mail_state();
        }
    }

    fn apply_inner(&mut self, acts: Vec<Act>, ctx: &egui::Context) {
        let now = Local::now();
        for a in acts {
            match a {
                Act::Select(id) => {
                    self.selected = Some(id);
                    self.comment_input.clear();
                    let auto = self.settings.ai_auto_summary;
                    let was_unread = self.mail(id).filter(|m| !m.read && m.uid > 0).map(|m| (m.account.clone(), m.uid));
                    if let Some(m) = self.mail_mut(id) {
                        m.read = true;
                    }
                    if self.settings.sync_read_to_gmail {
                        if let Some((account, uid)) = was_unread {
                            if let Some(acc) = self.real_account(&account) {
                                if let Some(pw) = mac::keychain_get(&acc.email) {
                                    std::thread::spawn(move || {
                                        let _ = net::mark_seen(&acc, &pw, uid);
                                    });
                                }
                            }
                        }
                    }
                    if auto && self.mail(id).map_or(false, |m| m.summary.is_none()) {
                        self.apply(vec![Act::Summarize(id)], ctx);
                    }
                }
                Act::ToggleRead(id) => {
                    if let Some(m) = self.mail_mut(id) {
                        m.read = !m.read;
                    }
                }
                Act::Pin(id) => {
                    if let Some(m) = self.mail_mut(id) {
                        m.pinned = !m.pinned;
                    }
                }
                Act::Priority(id) => {
                    if let Some(m) = self.mail_mut(id) {
                        m.priority = !m.priority;
                    }
                }
                Act::Done(id) => {
                    if let Some(m) = self.mail_mut(id) {
                        let snap = (m.id, m.done, m.read, m.trashed);
                        m.done = !m.done;
                        m.read = true;
                        self.undo_mail = Some(snap);
                    }
                    self.select_next(id);
                    self.toast("Marked as done  (Cmd+Z to undo)");
                }
                Act::Trash(id) => {
                    if let Some(m) = self.mail_mut(id) {
                        let snap = (m.id, m.done, m.read, m.trashed);
                        m.trashed = true;
                        self.undo_mail = Some(snap);
                    }
                    self.select_next(id);
                    self.toast("Moved to Trash  (Cmd+Z to undo)");
                }
                Act::SetAside(id) => {
                    if let Some(m) = self.mail_mut(id) {
                        m.set_aside = !m.set_aside;
                    }
                    self.toast("Set aside for later");
                }
                Act::Snooze(id, p) => {
                    let t = p.resolve(now, false);
                    if let Some(m) = self.mail_mut(id) {
                        match t {
                            Some(t) => m.snoozed_until = Some(t),
                            None => m.someday = true,
                        }
                    }
                    self.select_next(id);
                    self.toast(match t {
                        Some(t) => format!("Snoozed until {}", mech::short_when(t, now)),
                        None => "Snoozed to Someday".into(),
                    });
                }
                Act::Unsnooze(id) => {
                    if let Some(m) = self.mail_mut(id) {
                        m.snoozed_until = None;
                        m.someday = false;
                    }
                }
                Act::Accept(addr) => {
                    for m in self.mails.iter_mut().filter(|m| m.from_addr == addr) {
                        m.new_sender = false;
                    }
                    self.toast(format!("{} can now reach your inbox", addr));
                }
                Act::Block(addr) => {
                    if !self.settings.blocked.contains(&addr) {
                        self.settings.blocked.push(addr.clone());
                    }
                    for m in self.mails.iter_mut().filter(|m| m.from_addr == addr) {
                        m.new_sender = false;
                    }
                    self.save_settings();
                    self.toast(format!("Blocked {}", addr));
                }
                Act::Reply(id, all) => {
                    if let Some(m) = self.mail(id).cloned() {
                        let mut c = Composer::blank();
                        c.from = self.accounts().iter().position(|a| *a == m.account).unwrap_or(0);
                        c.to = m.from_addr.clone();
                        if all {
                            let me = m.account.to_lowercase();
                            for part in m.to.split(',') {
                                let addr = match (part.find('<'), part.rfind('>')) {
                                    (Some(a), Some(b)) if b > a => part[a + 1..b].trim().to_string(),
                                    _ => part.trim().to_string(),
                                };
                                if !addr.is_empty() && addr.to_lowercase() != me && !c.to.to_lowercase().contains(&addr.to_lowercase()) {
                                    c.to.push_str(", ");
                                    c.to.push_str(&addr);
                                }
                            }
                        }
                        c.to_name = m.from_name.clone();
                        c.subject = if m.subject.starts_with("Re:") { m.subject.clone() } else { format!("Re: {}", m.subject) };
                        c.reply_to = Some(id);
                        c.focus_top = true;
                        let quoted: String = m.body.lines().take(40).map(|l| format!("> {}", l)).collect::<Vec<_>>().join("\n");
                        c.body = format!("\n\n{}\n\nOn {}, {} <{}> wrote:\n{}", self.settings.signature, m.received.format("%a, %b %-d at %-I:%M %p"), m.from_name, m.from_addr, quoted);
                        self.composer = Some(c);
                    }
                }
                Act::QuickReply(id, intent) => {
                    if let Some(m) = self.mail(id).cloned() {
                        self.apply(vec![Act::Reply(id, false)], ctx);
                        let fallback = format!("{}\n\n{}", ai::quick_reply(&m, intent), self.settings.signature);
                        let what = match intent {
                            "interested" => "positive, interested",
                            "not_interested" => "polite decline",
                            _ => "short thank-you",
                        };
                        let prompt = format!(
                            "Write a {} reply to this email. Reply body only, no subject line. Sign off with:\n{}\n\nFrom: {}\nSubject: {}\n\n{}",
                            what, self.settings.signature, m.from_name, m.subject, m.body
                        );
                        if let Some(c) = self.composer.as_mut() {
                            c.ai_busy = true;
                        }
                        self.run_ai(ctx, "You write concise, natural email replies.", prompt, fallback, Target::Composer);
                    }
                }
                Act::Forward(id) => {
                    if let Some(m) = self.mail(id).cloned() {
                        let mut c = Composer::blank();
                        c.subject = format!("Fwd: {}", m.subject);
                        c.body = format!(
                            "\n\n---------- Forwarded message ----------\nFrom: {} <{}>\nSubject: {}\n\n{}",
                            m.from_name, m.from_addr, m.subject, m.body
                        );
                        self.composer = Some(c);
                    }
                }
                Act::Summarize(id) => {
                    if let Some(m) = self.mail(id).cloned() {
                        if let Some(mm) = self.mail_mut(id) {
                            mm.summary = Some("Summarizing...".into());
                        }
                        let prompt = format!(
                            "Summarize this email in 2 short sentences, then list any requests or deadlines for me as bullets.\n\nFrom: {}\nSubject: {}\n\n{}",
                            m.from_name, m.subject, m.body
                        );
                        self.run_ai(ctx, "You summarize emails clearly and briefly.", prompt, ai::summarize(&m), Target::Summary(id));
                    }
                }
                Act::MarkSectionRead(sec) => {
                    let ids: Vec<u64> = self.visible_mails(Folder::SmartInbox).iter().filter(|m| self.section_of(m) == sec).map(|m| m.id).collect();
                    for id in ids {
                        if let Some(m) = self.mail_mut(id) {
                            m.read = true;
                        }
                    }
                }
                Act::DoneSection(sec) => {
                    let ids: Vec<u64> = self.visible_mails(Folder::SmartInbox).iter().filter(|m| self.section_of(m) == sec).map(|m| m.id).collect();
                    let n = ids.len();
                    for id in ids {
                        if let Some(m) = self.mail_mut(id) {
                            m.done = true;
                            m.read = true;
                        }
                    }
                    self.toast(format!("{} emails marked as done", n));
                }
                Act::Unsubscribe(id) => {
                    let addr = self.mail(id).map(|m| m.from_addr.clone());
                    if let Some(addr) = addr {
                        for m in self.mails.iter_mut().filter(|m| m.from_addr == addr) {
                            m.can_unsubscribe = false;
                            m.done = true;
                        }
                        if !self.settings.blocked.contains(&addr) {
                            self.settings.blocked.push(addr.clone());
                        }
                        self.save_settings();
                        self.toast(format!("Unsubscribed from {}", addr));
                    }
                }
                Act::Export(id, integ) => {
                    if let Some(m) = self.mail(id).cloned() {
                        let text = format!("From: {} <{}>\n{}\n\n{}", m.from_name, m.from_addr, m.received.format("%b %-d, %Y"), m.body);
                        match integ.export(&m.subject, &text) {
                            Ok((msg, copy)) => {
                                if copy {
                                    ctx.output_mut(|o| o.copied_text = format!("{}\n\n{}", m.subject, text));
                                }
                                self.toast(msg);
                            }
                            Err(e) => self.toast(format!("{}: {}", integ.name(), e)),
                        }
                    }
                }
                Act::Label(id, label) => {
                    if let Some(m) = self.mail_mut(id) {
                        if let Some(pos) = m.labels.iter().position(|l| *l == label) {
                            m.labels.remove(pos);
                        } else {
                            m.labels.push(label);
                        }
                    }
                }
                Act::Share(id) => {
                    if let Some(m) = self.mail_mut(id) {
                        m.shared = true;
                    }
                    ctx.output_mut(|o| o.copied_text = format!("flint://thread/{}", id));
                    self.toast("Shared with your team - link copied");
                }
                Act::Assign(id, who) => {
                    if let Some(m) = self.mail_mut(id) {
                        m.assigned = Some(who.clone());
                        m.shared = true;
                    }
                    self.toast(format!("Assigned to {}", who));
                }
                Act::Comment(id) => {
                    let text = self.comment_input.trim().to_string();
                    if !text.is_empty() {
                        if let Some(m) = self.mail_mut(id) {
                            m.comments.push(Comment { author: "Me".into(), text, at: now });
                            m.shared = true;
                        }
                        self.comment_input.clear();
                    }
                }
                Act::CopyText(t) => {
                    ctx.output_mut(|o| o.copied_text = t);
                    self.toast("Copied");
                }
                Act::Expand(key) => {
                    self.expanded.insert(key);
                }
                Act::Go(f) => {
                    self.folder = f;
                    self.search.clear();
                    self.selected = self.visible_mails(f).first().map(|m| m.id);
                }
            }
        }
    }

    fn select_next(&mut self, id: u64) {
        if self.selected != Some(id) {
            return;
        }
        let list: Vec<u64> = self.visible_mails(self.folder).iter().map(|m| m.id).collect();
        self.selected = list.into_iter().find(|x| *x != id);
    }

    fn send_composer(&mut self, at: Option<DateTime<Local>>) {
        let Some(c) = self.composer.take() else { return };
        if c.to.trim().is_empty() {
            self.toast("Add at least one recipient");
            self.composer = Some(c);
            return;
        }
        let now = Local::now();
        let accounts = self.accounts();
        let account = accounts.get(c.from).cloned().unwrap_or_else(|| accounts[0].clone());
        let follow_up = c.follow_up_days.map(|d| at.unwrap_or(now) + Duration::days(d));
        let item = Scheduled {
            id: self.id(),
            account,
            to: c.to.clone(),
            subject: if c.subject.trim().is_empty() { "(no subject)".into() } else { c.subject.clone() },
            body: c.body.clone(),
            send_at: at.unwrap_or(now),
            read_receipt: c.read_receipt,
            follow_up,
            attachments: c.attachments.clone(),
        };
        if let Some(rid) = c.reply_to {
            if let Some(m) = self.mail_mut(rid) {
                m.done = true;
            }
        }
        match at {
            Some(t) => {
                self.scheduled.push(item);
                self.toast(format!("Will send {}", mech::short_when(t, now)));
            }
            None => {
                if let Some(p) = self.pending.take() {
                    self.sent.insert(
                        0,
                        SentMail {
                            id: p.item.id,
                            account: p.item.account,
                            to: p.item.to,
                            subject: p.item.subject,
                            body: p.item.body,
                            at: now,
                            read_receipt: p.item.read_receipt,
                            opened: Vec::new(),
                            follow_up: p.item.follow_up,
                            follow_up_fired: false,
                            replied: false,
                        },
                    );
                }
                self.pending = Some(PendingSend { item, deadline: now + Duration::seconds(self.settings.undo_send_secs.max(0)) });
            }
        }
    }

    fn undo_send(&mut self) {
        if let Some(p) = self.pending.take() {
            let mut c = Composer::blank();
            c.from = self.accounts().iter().position(|a| *a == p.item.account).unwrap_or(0);
            c.to = p.item.to;
            c.subject = p.item.subject;
            c.body = p.item.body;
            c.read_receipt = p.item.read_receipt;
            self.composer = Some(c);
            self.toast("Sending undone");
        }
    }

    // ---------- keyboard ----------

    fn shortcuts(&mut self, ctx: &egui::Context) {
        let (cmd_k, esc) = ctx.input(|i| (i.modifiers.command && i.key_pressed(Key::K), i.key_pressed(Key::Escape)));
        if cmd_k {
            self.cmd_open = !self.cmd_open;
            self.cmd_query.clear();
        }
        if esc {
            self.cmd_open = false;
        }
        if esc {
            self.help_open = false;
            self.snap_open = false;
            if !ctx.wants_keyboard_input() {
                self.settings_open = false;
            }
        }
        if !ctx.wants_keyboard_input() && ctx.input(|i| i.modifiers.command && !i.modifiers.shift && i.key_pressed(Key::Z)) {
            if let Some((id, done, read, trashed)) = self.undo_mail.take() {
                if let Some(m) = self.mail_mut(id) {
                    m.done = done;
                    m.read = read;
                    m.trashed = trashed;
                }
                self.selected = Some(id);
                self.toast("Undone");
            }
        }
        if !ctx.wants_keyboard_input() && ctx.input(|i| i.events.iter().any(|e| matches!(e, egui::Event::Text(t) if t == "?"))) {
            self.help_open = !self.help_open;
        }
        if !ctx.wants_keyboard_input() && self.mode == Mode::Calendar && self.screenshot.is_none() {
            let (t, left, right, m_key) = ctx.input(|i| {
                let k = |key| i.key_pressed(key) && !i.modifiers.command;
                (k(Key::T), k(Key::ArrowLeft), k(Key::ArrowRight), k(Key::M))
            });
            let step = match self.cal_view {
                CalView::Day => 1,
                CalView::ThreeDays => 3,
                CalView::Week => 7,
                CalView::Month => 30,
            };
            if t {
                self.cal_anchor = Local::now().date_naive();
            }
            if left {
                self.cal_anchor -= Duration::days(step);
            }
            if right {
                self.cal_anchor += Duration::days(step);
            }
            if m_key {
                self.mode = Mode::Mail;
            }
        }
        if ctx.wants_keyboard_input() || self.mode != Mode::Mail || self.screenshot.is_some() {
            return;
        }
        let keys = ctx.input(|i| {
            let k = |key| i.key_pressed(key) && !i.modifiers.command;
            (k(Key::J), k(Key::K), k(Key::E), k(Key::P), k(Key::R), k(Key::C), k(Key::H), k(Key::U), k(Key::Delete) || k(Key::Backspace), k(Key::Slash))
        });
        let (down, up, done, pin, reply, compose, snooze, unread, del, search) = keys;
        let list: Vec<u64> = self.visible_mails(self.folder).iter().map(|m| m.id).collect();
        let pos = self.selected.and_then(|s| list.iter().position(|x| *x == s));
        let mut acts = Vec::new();
        if down {
            let next = pos.map_or(0, |p| (p + 1).min(list.len().saturating_sub(1)));
            if let Some(id) = list.get(next) {
                acts.push(Act::Select(*id));
            }
        }
        if up {
            let prev = pos.map_or(0, |p| p.saturating_sub(1));
            if let Some(id) = list.get(prev) {
                acts.push(Act::Select(*id));
            }
        }
        if let Some(id) = self.selected {
            if done {
                acts.push(Act::Done(id));
            }
            if pin {
                acts.push(Act::Pin(id));
            }
            if reply {
                acts.push(Act::Reply(id, false));
            }
            if snooze {
                acts.push(Act::Snooze(id, Preset::Tomorrow));
            }
            if unread {
                acts.push(Act::ToggleRead(id));
            }
            if del {
                acts.push(Act::Trash(id));
            }
        }
        if compose {
            self.composer = Some(self.fresh_composer());
        }
        if search {
            ctx.memory_mut(|m| m.request_focus(egui::Id::new("search")));
        }
        self.apply(acts, ctx);
    }
}

// ---------- drawing helpers ----------

fn parse_quick_task(input: &str, now: DateTime<Local>) -> (String, Option<DateTime<Local>>) {
    let words: Vec<&str> = input.split_whitespace().collect();
    let mut keep: Vec<&str> = Vec::new();
    let mut day: Option<NaiveDate> = None;
    let mut time: Option<(u32, u32)> = None;
    let mut i = 0;
    while i < words.len() {
        let w = words[i].to_lowercase();
        let w = w.trim_matches(|c: char| c == ',' || c == '.');
        let weekdays = ["monday", "tuesday", "wednesday", "thursday", "friday", "saturday", "sunday"];
        if w == "today" && day.is_none() {
            day = Some(now.date_naive());
        } else if w == "tonight" && day.is_none() {
            day = Some(now.date_naive());
            time = time.or(Some((20, 0)));
        } else if w == "tomorrow" && day.is_none() {
            day = Some(now.date_naive() + Duration::days(1));
        } else if let Some(idx) = weekdays.iter().position(|d| *d == w) {
            let today = now.date_naive().weekday().num_days_from_monday() as i64;
            let mut diff = idx as i64 - today;
            if diff <= 0 {
                diff += 7;
            }
            day = Some(now.date_naive() + Duration::days(diff));
        } else if w == "at" && i + 1 < words.len() {
            let t = words[i + 1].to_lowercase();
            let t = t.trim_matches(|c: char| c == ',' || c == '.');
            let (num, pm) = if let Some(n) = t.strip_suffix("pm") { (n, Some(true)) } else if let Some(n) = t.strip_suffix("am") { (n, Some(false)) } else { (t, None) };
            let mut parts = num.split(':');
            let h = parts.next().and_then(|x| x.parse::<u32>().ok());
            let m = parts.next().and_then(|x| x.parse::<u32>().ok()).unwrap_or(0);
            if let Some(h) = h {
                let h24 = match pm {
                    Some(true) if h < 12 => h + 12,
                    Some(false) if h == 12 => 0,
                    _ => h,
                };
                if h24 < 24 && m < 60 {
                    time = Some((h24, m));
                    i += 2;
                    continue;
                }
            }
            keep.push(words[i]);
        } else {
            keep.push(words[i]);
        }
        i += 1;
    }
    let title = keep.join(" ");
    if title.is_empty() {
        return (input.trim().to_string(), None);
    }
    let due = match (day, time) {
        (None, None) => None,
        (d, t) => {
            let d = d.unwrap_or(now.date_naive());
            let (h, m) = t.unwrap_or((9, 0));
            Some(mech::at(d, h, m))
        }
    };
    (title, due)
}

fn meeting_link(text: &str) -> Option<String> {
    text.split_whitespace()
        .find(|w| w.starts_with("https://") || w.starts_with("http://"))
        .map(|w| w.trim_matches(|c: char| c == '<' || c == '>' || c == ',' || c == ')' || c == '(').to_string())
}

fn one_line(ui: &Ui, text: &str, size: f32, color: Color32, max_w: f32) -> Arc<Galley> {
    let mut job = LayoutJob::single_section(
        text.to_owned(),
        TextFormat { font_id: FontId::proportional(size), color, ..Default::default() },
    );
    job.wrap.max_width = max_w.max(10.0);
    job.wrap.max_rows = 1;
    job.wrap.break_anywhere = true;
    job.wrap.overflow_character = Some('…');
    ui.fonts(|f| f.layout_job(job))
}

fn avatar(ui: &Ui, center: egui::Pos2, r: f32, name: &str) {
    let p = ui.painter();
    p.circle_filled(center, r, theme::avatar_color(name));
    p.text(center, Align2::CENTER_CENTER, theme::initials(name), FontId::proportional(r * 0.8), Color32::WHITE);
}

fn chip(ui: &mut Ui, text: &str, fill: Color32, fg: Color32) {
    egui::Frame::none()
        .fill(fill)
        .rounding(Rounding::same(9.0))
        .inner_margin(egui::Margin::symmetric(7.0, 2.0))
        .show(ui, |ui| {
            ui.label(RichText::new(text).size(11.0).color(fg));
        });
}

fn primary_button(ui: &mut Ui, text: &str) -> egui::Response {
    ui.add(egui::Button::new(RichText::new(text).color(Color32::WHITE).strong()).fill(ACCENT).rounding(Rounding::same(6.0)))
}

fn tool_button(ui: &mut Ui, text: &str, pal: Pal) -> egui::Response {
    ui.add(egui::Button::new(RichText::new(text).size(12.5).color(pal.text)).fill(pal.alt).stroke(Stroke::new(1.0_f32, pal.border)))
}

fn badge(ui: &mut Ui, text: &str) {
    egui::Frame::none()
        .fill(Color32::from_rgb(0xe5, 0xf2, 0xff))
        .rounding(Rounding::same(4.0))
        .inner_margin(egui::Margin::symmetric(5.0, 1.0))
        .show(ui, |ui| {
            ui.label(RichText::new(text).size(10.0).color(theme::ACCENT_DEEP).strong());
        });
}

fn mail_row(ui: &mut Ui, m: &Mail, selected: bool, pal: Pal, now: DateTime<Local>) -> egui::Response {
    let w = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(w, 62.0), Sense::click());
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let painter = ui.painter_at(rect);
    let bg = if selected {
        pal.sel
    } else if resp.hovered() {
        pal.hover
    } else {
        pal.bg
    };
    painter.rect_filled(rect.shrink2(egui::vec2(6.0, 1.0)), Rounding::same(8.0), bg);
    let left = rect.left() + 14.0;
    if !m.read {
        painter.circle_filled(egui::pos2(left - 4.0, rect.center().y), 3.5, ACCENT);
    }
    avatar(ui, egui::pos2(left + 18.0, rect.center().y), 16.0, &m.from_name);
    let x = left + 44.0;
    let time = mech::list_time(m.received, now);
    let time_g = one_line(ui, &time, 11.5, pal.sub, 90.0);
    let time_w = time_g.size().x;
    painter.galley(egui::pos2(rect.right() - 16.0 - time_w, rect.top() + 9.0), time_g, pal.sub);
    let name_color = if m.read { pal.sub } else { pal.text };
    let name_g = one_line(ui, &m.from_name, 13.5, name_color, rect.right() - x - time_w - 70.0);
    let name_w = name_g.size().x;
    painter.galley(egui::pos2(x, rect.top() + 8.0), name_g, name_color);
    let mut mx = x + name_w + 6.0;
    let my = rect.top() + 16.0;
    if m.priority {
        painter.circle_filled(egui::pos2(mx + 4.0, my), 4.0, RED);
        mx += 12.0;
    }
    if m.pinned {
        painter.circle_filled(egui::pos2(mx + 4.0, my), 4.0, ORANGE);
        mx += 12.0;
    }
    if !m.comments.is_empty() || m.shared {
        painter.circle_filled(egui::pos2(mx + 4.0, my), 4.0, GREEN);
    }
    let subj_color = if m.read { pal.sub } else { pal.text };
    let subj = one_line(ui, &m.subject, 12.5, subj_color, rect.right() - x - 18.0);
    painter.galley(egui::pos2(x, rect.top() + 26.0), subj, subj_color);
    let mut snippet = m.snippet();
    if let Some(t) = m.snoozed_until {
        snippet = format!("Snoozed until {}  -  {}", mech::short_when(t, now), snippet);
    }
    let sn = one_line(ui, &snippet, 12.0, pal.sub, rect.right() - x - 18.0);
    painter.galley(egui::pos2(x, rect.top() + 43.0), sn, pal.sub);
    resp
}

// ---------- panels ----------

impl FlintApp {
    fn sidebar(&mut self, ui: &mut Ui) {
        let now = Local::now();
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            ui.add_space(6.0);
            let (r, _) = ui.allocate_exact_size(egui::vec2(22.0, 22.0), Sense::hover());
            ui.painter().rect_filled(r, Rounding::same(6.0), ACCENT);
            ui.painter().text(r.center(), Align2::CENTER_CENTER, "F", FontId::proportional(14.0), Color32::WHITE);
            ui.label(RichText::new("Flint Mail").size(15.0).color(Color32::WHITE).strong());
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.add_space(6.0);
                badge(ui, "PRO");
            });
        });
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            ui.add_space(6.0);
            let compose = ui.add_sized(
                [ui.available_width() - 8.0, 32.0],
                egui::Button::new(RichText::new("+  New Email").color(Color32::WHITE).strong()).fill(ACCENT).rounding(Rounding::same(8.0)),
            );
            if compose.clicked() {
                self.composer = Some(self.fresh_composer());
            }
        });
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.add_space(6.0);
            let w = (ui.available_width() - 14.0) / 2.0;
            for (label, mode) in [("Mail", Mode::Mail), ("Calendar", Mode::Calendar)] {
                let on = self.mode == mode;
                let b = egui::Button::new(RichText::new(label).size(12.5).color(if on { Color32::WHITE } else { theme::SIDEBAR_TEXT }))
                    .fill(if on { theme::SIDEBAR_SEL } else { Color32::from_rgb(0x1f, 0x21, 0x24) })
                    .rounding(Rounding::same(6.0));
                if ui.add_sized([w, 26.0], b).clicked() {
                    self.mode = mode;
                }
            }
        });
        ui.add_space(8.0);
        egui::ScrollArea::vertical().id_salt("sidebar-scroll").show(ui, |ui| {
            if self.mode == Mode::Mail {
                self.sidebar_mail(ui);
            } else {
                self.sidebar_calendar(ui, now);
            }
        });
    }

    fn side_item(&mut self, ui: &mut Ui, label: &str, count: usize, selected: bool, dot: Option<Color32>) -> bool {
        let w = ui.available_width();
        let (rect, resp) = ui.allocate_exact_size(egui::vec2(w, 28.0), Sense::click());
        let r = rect.shrink2(egui::vec2(6.0, 1.0));
        if selected {
            ui.painter().rect_filled(r, Rounding::same(6.0), theme::SIDEBAR_SEL);
        } else if resp.hovered() {
            ui.painter().rect_filled(r, Rounding::same(6.0), theme::SIDEBAR_HOVER);
        }
        let color = if selected { Color32::WHITE } else { theme::SIDEBAR_TEXT };
        let mut x = r.left() + 10.0;
        if let Some(c) = dot {
            ui.painter().circle_filled(egui::pos2(x + 4.0, r.center().y), 4.0, c);
            x += 16.0;
        }
        ui.painter().text(egui::pos2(x, r.center().y), Align2::LEFT_CENTER, label, FontId::proportional(13.0), color);
        if count > 0 {
            ui.painter().text(
                egui::pos2(r.right() - 10.0, r.center().y),
                Align2::RIGHT_CENTER,
                count.to_string(),
                FontId::proportional(12.0),
                if selected { Color32::WHITE } else { theme::SIDEBAR_SUB },
            );
        }
        resp.clicked()
    }

    fn side_heading(ui: &mut Ui, text: &str) {
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            ui.add_space(16.0);
            ui.label(RichText::new(text.to_uppercase()).size(10.5).color(theme::SIDEBAR_SUB).strong());
        });
        ui.add_space(2.0);
    }

    fn sidebar_mail(&mut self, ui: &mut Ui) {
        let groups: [(&str, &[Folder]); 3] = [
            ("Mail", &[Folder::SmartInbox, Folder::Today, Folder::AllInboxes, Folder::Priority, Folder::Pinned, Folder::Gatekeeper, Folder::SetAside, Folder::Snoozed]),
            ("Team", &[Folder::Shared, Folder::Assigned]),
            ("More", &[Folder::FollowUps, Folder::Scheduled, Folder::Sent, Folder::Done, Folder::Trash]),
        ];
        for (title, folders) in groups {
            Self::side_heading(ui, title);
            for f in folders.iter().copied() {
                let count = self.folder_count(f);
                let dot = match f {
                    Folder::Priority => Some(RED),
                    Folder::Pinned => Some(ORANGE),
                    Folder::Gatekeeper => Some(theme::PURPLE),
                    Folder::Shared | Folder::Assigned => Some(GREEN),
                    _ => None,
                };
                if self.side_item(ui, f.name(), count, self.folder == f && self.search.is_empty(), dot) {
                    self.folder = f;
                    self.search.clear();
                    let first = self.visible_mails(f).first().map(|m| m.id);
                    if f != Folder::Sent && f != Folder::Scheduled && f != Folder::FollowUps {
                        self.selected = first;
                    }
                }
            }
        }
        Self::side_heading(ui, "Labels");
        for l in data::LABELS {
            let n = self.mails.iter().filter(|m| !m.trashed && m.labels.iter().any(|x| x == l)).count();
            let on = self.search == format!("label:{}", l.to_lowercase());
            if self.side_item(ui, l, n, on, Some(theme::avatar_color(l))) {
                self.search = format!("label:{}", l.to_lowercase());
            }
        }
        Self::side_heading(ui, "Accounts");
        for acc in self.accounts() {
            let syncing = self.syncing.contains(&acc);
            let label = if syncing { format!("{}  (syncing)", acc) } else { acc.clone() };
            let n = self.mails.iter().filter(|m| m.account == acc && !m.read && self.in_folder(m, Folder::SmartInbox)).count();
            let on = self.search == format!("to:{}", acc);
            if self.side_item(ui, &label, n, on, Some(theme::avatar_color(&acc))) {
                self.settings_open = true;
                self.settings_tab = SettingsTab::Accounts;
            }
        }
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.add_space(14.0);
            if ui.add(egui::Button::new(RichText::new("+ Add account").size(12.0).color(theme::SIDEBAR_TEXT)).frame(false)).clicked() {
                self.account_form = Some(AccountForm { imap_port: "993".into(), smtp_port: "465".into(), ..Default::default() });
            }
        });
        ui.add_space(12.0);
    }

    fn sidebar_calendar(&mut self, ui: &mut Ui, now: DateTime<Local>) {
        // mini month
        let anchor = self.cal_anchor;
        let first = NaiveDate::from_ymd_opt(anchor.year(), anchor.month(), 1).unwrap();
        ui.horizontal(|ui| {
            ui.add_space(14.0);
            ui.label(RichText::new(first.format("%B %Y").to_string()).color(Color32::WHITE).strong());
        });
        let cell = ((ui.available_width() - 24.0) / 7.0).floor();
        let start = first - Duration::days(first.weekday().num_days_from_sunday() as i64);
        let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), cell * 7.0), Sense::click());
        let x0 = rect.left() + 12.0;
        for (i, d) in ["S", "M", "T", "W", "T", "F", "S"].iter().enumerate() {
            ui.painter().text(
                egui::pos2(x0 + cell * (i as f32 + 0.5), rect.top() + cell * 0.5),
                Align2::CENTER_CENTER,
                *d,
                FontId::proportional(10.5),
                theme::SIDEBAR_SUB,
            );
        }
        for i in 0..42 {
            let d = start + Duration::days(i);
            let c = egui::pos2(x0 + cell * ((i % 7) as f32 + 0.5), rect.top() + cell * ((i / 7) as f32 + 1.5));
            if d == now.date_naive() {
                ui.painter().circle_filled(c, cell * 0.42, ACCENT);
            } else if d == anchor {
                ui.painter().circle_stroke(c, cell * 0.42, Stroke::new(1.0_f32, theme::SIDEBAR_TEXT));
            }
            let col = if d.month() != first.month() { theme::SIDEBAR_SUB } else { Color32::WHITE };
            ui.painter().text(c, Align2::CENTER_CENTER, d.day().to_string(), FontId::proportional(11.5), col);
        }
        if resp.clicked() {
            if let Some(p) = resp.interact_pointer_pos() {
                let col = ((p.x - x0) / cell).floor();
                let row = ((p.y - rect.top()) / cell).floor() - 1.0;
                if (0.0..7.0).contains(&col) && (0.0..6.0).contains(&row) {
                    self.cal_anchor = start + Duration::days((row * 7.0 + col) as i64);
                }
            }
        }
        Self::side_heading(ui, "Calendars");
        for i in 0..self.calendars.len() {
            let color = theme::CALENDAR_COLORS[i % theme::CALENDAR_COLORS.len()];
            let name = self.calendars[i].name.clone();
            let visible = self.calendars[i].visible;
            let label = if visible { name } else { format!("{}  (hidden)", name) };
            if self.side_item(ui, &label, 0, false, Some(if visible { color } else { theme::SIDEBAR_SUB })) {
                self.calendars[i].visible = !visible;
            }
        }
        Self::side_heading(ui, "Up next");
        let next: Vec<(u64, String, DateTime<Local>)> = self.upcoming_events(now).iter().take(5).map(|e| (e.id, e.title.clone(), e.start)).collect();
        for (id, title, start) in next {
            let label = format!("{}  {}", mech::short_when(start, now), title);
            if self.side_item(ui, &label, 0, self.selected_event == Some(id), None) {
                self.selected_event = Some(id);
                self.cal_anchor = start.date_naive();
            }
        }
    }

    fn toolbar(&mut self, ui: &mut Ui, pal: Pal) {
        let now = Local::now();
        ui.horizontal_centered(|ui| {
            ui.add_space(4.0);
            let title = if self.mode == Mode::Calendar {
                "Calendar".to_string()
            } else if !self.search.is_empty() {
                "Search".to_string()
            } else {
                self.folder.name().to_string()
            };
            ui.label(RichText::new(title).size(17.0).strong().color(pal.text));
            ui.add_space(12.0);
            let search = ui.add(
                egui::TextEdit::singleline(&mut self.search)
                    .id(egui::Id::new("search"))
                    .hint_text("Search mail   (from: to: subject: is:unread -label:work)")
                    .desired_width(330.0),
            );
            if search.changed() && self.mode == Mode::Calendar {
                self.mode = Mode::Mail;
            }
            if !self.search.is_empty() && ui.small_button("Clear").clicked() {
                self.search.clear();
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.add_space(6.0);
                if tool_button(ui, "Settings", pal).clicked() {
                    self.settings_open = true;
                }
                let ai_label = if self.copilot_open { "Close AI" } else { "Ask AI" };
                if ui
                    .add(egui::Button::new(RichText::new(ai_label).color(Color32::WHITE).strong()).fill(theme::PURPLE).rounding(Rounding::same(6.0)))
                    .clicked()
                {
                    self.copilot_open = !self.copilot_open;
                }
                if tool_button(ui, "Command  (Cmd+K)", pal).clicked() {
                    self.cmd_open = true;
                    self.cmd_query.clear();
                }
                if !self.syncing.is_empty() || self.cal_syncing || self.tasks_syncing {
                    ui.label(RichText::new("Syncing...").size(11.5).color(pal.sub));
                }
                if let Some(e) = self.upcoming_events(now).first() {
                    if (e.start - now) < Duration::hours(3) {
                        let id = e.id;
                        let text = format!("{}  {}", e.title, mech::countdown(e.start, now));
                        let chip_fg = if self.settings.dark { Color32::from_rgb(0x9c, 0xcd, 0xff) } else { theme::ACCENT_DEEP };
                        let b = egui::Button::new(RichText::new(text).size(12.0).color(chip_fg)).fill(pal.sel);
                        if ui.add(b).clicked() {
                            self.mode = Mode::Calendar;
                            self.selected_event = Some(id);
                        }
                    }
                }
            });
        });
    }

    fn list_panel(&mut self, ui: &mut Ui, pal: Pal) {
        let now = Local::now();
        let mut acts = Vec::new();
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.add_space(10.0);
            ui.selectable_value(&mut self.unread_only, false, "All");
            ui.selectable_value(&mut self.unread_only, true, "Unread");
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.add_space(10.0);
                if self.folder == Folder::SmartInbox && self.search.is_empty() {
                    ui.label(RichText::new(format!("{} unread", self.unread_count())).size(12.0).color(pal.sub));
                }
            });
        });
        ui.add_space(4.0);
        egui::ScrollArea::vertical().id_salt("list-scroll").auto_shrink([false, false]).show(ui, |ui| {
            match self.folder {
                Folder::Sent | Folder::FollowUps if self.search.is_empty() => self.sent_list(ui, pal, now),
                Folder::Scheduled if self.search.is_empty() => self.scheduled_list(ui, pal, now),
                Folder::SmartInbox if self.search.is_empty() && !self.unread_only => self.smart_inbox(ui, pal, now, &mut acts),
                _ => {
                    if self.folder == Folder::Gatekeeper && self.search.is_empty() {
                        self.gatekeeper(ui, pal, &mut acts);
                    } else {
                        let list = self.visible_mails(self.folder);
                        if list.is_empty() {
                            ui.add_space(70.0);
                            let (title, hint) = if !self.search.is_empty() {
                                ("No matches", "Try fewer words, or a filter like from:maya subject:launch is:unread.")
                            } else if self.unread_only {
                                ("All read", "Nothing unread in this folder.")
                            } else {
                                match self.folder {
                                    Folder::Done => ("Nothing done yet", "Press e on an email to mark it done.")
                                    ,
                                    Folder::Trash => ("Trash is empty", "Deleted emails land here."),
                                    Folder::Pinned => ("Nothing pinned", "Press p on an email to pin it."),
                                    Folder::Snoozed => ("Nothing snoozed", "Press h to snooze an email until tomorrow."),
                                    Folder::Today => ("Nothing new today", "Emails that arrive today show up here."),
                                    _ => ("Nothing here", "You're all caught up."),
                                }
                            };
                            ui.vertical_centered(|ui| {
                                ui.label(RichText::new(title).size(17.0).strong().color(pal.text));
                                ui.add_space(4.0);
                                ui.label(RichText::new(hint).size(12.5).color(pal.sub));
                            });
                        }
                        for m in list {
                            let r = mail_row(ui, m, self.selected == Some(m.id), pal, now);
                            if r.clicked() {
                                acts.push(Act::Select(m.id));
                            }
                            self.row_menu(&r, m, &mut acts);
                        }
                    }
                }
            }
            ui.add_space(20.0);
        });
        self.apply(acts, ui.ctx());
    }

    fn row_menu(&self, r: &egui::Response, m: &Mail, acts: &mut Vec<Act>) {
        let id = m.id;
        r.context_menu(|ui| {
            if ui.button(if m.read { "Mark as unread" } else { "Mark as read" }).clicked() {
                acts.push(Act::ToggleRead(id));
                ui.close_menu();
            }
            if ui.button(if m.pinned { "Unpin" } else { "Pin" }).clicked() {
                acts.push(Act::Pin(id));
                ui.close_menu();
            }
            if ui.button("Done").clicked() {
                acts.push(Act::Done(id));
                ui.close_menu();
            }
            ui.menu_button("Snooze", |ui| {
                let now = Local::now();
                for p in Preset::SNOOZE {
                    if let Some(l) = p.label(now, false) {
                        if ui.button(l).clicked() {
                            acts.push(Act::Snooze(id, p));
                            ui.close_menu();
                        }
                    }
                }
            });
            if ui.button("Summarize with AI").clicked() {
                acts.push(Act::Select(id));
                acts.push(Act::Summarize(id));
                ui.close_menu();
            }
            if ui.button("Delete").clicked() {
                acts.push(Act::Trash(id));
                ui.close_menu();
            }
        });
    }

    fn smart_inbox(&self, ui: &mut Ui, pal: Pal, now: DateTime<Local>, acts: &mut Vec<Act>) {
        let pending: Vec<&Mail> = if self.settings.gatekeeper {
            self.mails.iter().filter(|m| self.in_folder(m, Folder::Gatekeeper)).collect()
        } else {
            Vec::new()
        };
        let follow = self.due_follow_ups();
        let today = now.date_naive();
        let todays_events: Vec<&Event> = self.events.iter().filter(|e| e.start.date_naive() == today && e.end > now).collect();
        let due_tasks = self.tasks.iter().filter(|t| t.due.map(|d| d.date_naive() <= today).unwrap_or(false)).count();
        let unread = self.mails.iter().filter(|m| !m.read && !m.trashed && !m.done).count();
        if self.screenshot.is_none() && (!todays_events.is_empty() || due_tasks > 0) {
            let greeting = match now.hour() {
                0..=4 => "Late night",
                5..=11 => "Good morning",
                12..=16 => "Good afternoon",
                _ => "Good evening",
            };
            egui::Frame::none()
                .fill(pal.alt)
                .rounding(Rounding::same(8.0))
                .inner_margin(egui::Margin::same(10.0))
                .outer_margin(egui::Margin::symmetric(10.0, 4.0))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(RichText::new(greeting).strong().color(pal.text));
                    let mut parts = vec![format!("{} unread", unread)];
                    if !todays_events.is_empty() {
                        parts.push(format!("{} event{} left today", todays_events.len(), if todays_events.len() == 1 { "" } else { "s" }));
                    }
                    if due_tasks > 0 {
                        parts.push(format!("{} task{} due", due_tasks, if due_tasks == 1 { "" } else { "s" }));
                    }
                    ui.label(RichText::new(parts.join("  -  ")).size(12.0).color(pal.sub));
                    if let Some(e) = todays_events.iter().min_by_key(|e| e.start) {
                        ui.label(RichText::new(format!("Next: {} {}", e.title, mech::countdown(e.start, now))).size(12.0).color(ACCENT));
                    }
                });
        }
        if !pending.is_empty() || !follow.is_empty() {
            egui::Frame::none()
                .fill(pal.alt)
                .rounding(Rounding::same(8.0))
                .inner_margin(egui::Margin::same(10.0))
                .outer_margin(egui::Margin::symmetric(10.0, 4.0))
                .show(ui, |ui| {
                    if !pending.is_empty() {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(format!("{} new senders want to reach you", pending.len())).color(pal.text).strong());
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if ui.small_button("Review").clicked() {
                                    acts.push(Act::Go(Folder::Gatekeeper));
                                }
                            });
                        });
                        for m in pending.iter().take(3) {
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(&m.from_name).size(12.5).color(pal.text));
                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    if ui.small_button("Block").clicked() {
                                        acts.push(Act::Block(m.from_addr.clone()));
                                    }
                                    if ui.small_button("Accept").clicked() {
                                        acts.push(Act::Accept(m.from_addr.clone()));
                                    }
                                });
                            });
                        }
                    }
                    for s in follow.iter().take(2) {
                        ui.label(RichText::new(format!("Follow up: no reply from {} to \"{}\"", s.to, s.subject)).size(12.0).color(ORANGE));
                    }
                });
        }

        let list = self.visible_mails(Folder::SmartInbox);
        for sec in Section::ORDER {
            let items: Vec<&&Mail> = list.iter().filter(|m| self.section_of(m) == sec).collect();
            if items.is_empty() {
                continue;
            }
            let key = sec.name();
            let expanded = self.expanded.contains(key) || matches!(sec, Section::Priority | Section::People);
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.add_space(14.0);
                ui.label(RichText::new(sec.name()).size(12.5).strong().color(pal.text));
                ui.label(RichText::new(items.len().to_string()).size(12.0).color(pal.sub));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.add_space(12.0);
                    match sec {
                        Section::Notifications | Section::Newsletters => {
                            if ui.small_button("Mark all read").clicked() {
                                acts.push(Act::MarkSectionRead(sec));
                            }
                        }
                        Section::Seen => {
                            if ui.small_button("Done all").clicked() {
                                acts.push(Act::DoneSection(sec));
                            }
                        }
                        _ => {}
                    }
                });
            });
            let limit = if expanded { items.len() } else { mech::CARD_ITEMS_LIMIT.min(items.len()) };
            for m in items.iter().take(limit) {
                let r = mail_row(ui, m, self.selected == Some(m.id), pal, now);
                if r.clicked() {
                    acts.push(Act::Select(m.id));
                }
                self.row_menu(&r, m, acts);
            }
            if items.len() > limit {
                ui.horizontal(|ui| {
                    ui.add_space(58.0);
                    if ui.link(RichText::new(format!("Show {} more", items.len() - limit)).size(12.0)).clicked() {
                        acts.push(Act::Expand(key));
                    }
                });
            }
        }
    }

    fn gatekeeper(&self, ui: &mut Ui, pal: Pal, acts: &mut Vec<Act>) {
        ui.horizontal(|ui| {
            ui.add_space(14.0);
            ui.label(RichText::new("Decide who can email you. Accepted senders go to your inbox; blocked ones never show up again.").size(12.0).color(pal.sub));
        });
        let mut seen = HashSet::new();
        for m in self.mails.iter().filter(|m| self.in_folder(m, Folder::Gatekeeper)) {
            if !seen.insert(m.from_addr.clone()) {
                continue;
            }
            egui::Frame::none()
                .fill(pal.card)
                .stroke(Stroke::new(1.0_f32, pal.border))
                .rounding(Rounding::same(8.0))
                .inner_margin(egui::Margin::same(10.0))
                .outer_margin(egui::Margin::symmetric(10.0, 4.0))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        let (r, _) = ui.allocate_exact_size(egui::vec2(30.0, 30.0), Sense::hover());
                        avatar(ui, r.center(), 15.0, &m.from_name);
                        ui.vertical(|ui| {
                            ui.label(RichText::new(&m.from_name).strong().color(pal.text));
                            ui.label(RichText::new(&m.from_addr).size(11.5).color(pal.sub));
                        });
                    });
                    ui.label(RichText::new(&m.subject).size(12.5).color(pal.text));
                    ui.horizontal(|ui| {
                        if primary_button(ui, "Accept").clicked() {
                            acts.push(Act::Accept(m.from_addr.clone()));
                        }
                        if ui.button("Block").clicked() {
                            acts.push(Act::Block(m.from_addr.clone()));
                        }
                        if ui.button("Preview").clicked() {
                            acts.push(Act::Select(m.id));
                        }
                    });
                });
        }
        if seen.is_empty() {
            ui.add_space(30.0);
            ui.vertical_centered(|ui| ui.label(RichText::new("No new senders waiting").color(pal.sub)));
        }
    }

    fn sent_list(&mut self, ui: &mut Ui, pal: Pal, now: DateTime<Local>) {
        let follow_only = self.folder == Folder::FollowUps;
        let mut mark_replied = None;
        let mut nudge = None;
        for s in self.sent.iter() {
            let due = !s.replied && s.follow_up.map_or(false, |t| t <= now);
            if follow_only && s.follow_up.is_none() {
                continue;
            }
            egui::Frame::none()
                .fill(pal.card)
                .stroke(Stroke::new(1.0_f32, pal.border))
                .rounding(Rounding::same(8.0))
                .inner_margin(egui::Margin::same(10.0))
                .outer_margin(egui::Margin::symmetric(10.0, 4.0))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(format!("To {}", s.to)).strong().color(pal.text));
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            ui.label(RichText::new(mech::list_time(s.at, now)).size(11.5).color(pal.sub));
                        });
                    });
                    ui.label(RichText::new(&s.subject).size(12.5).color(pal.text));
                    ui.horizontal_wrapped(|ui| {
                        if s.read_receipt {
                            if s.opened.is_empty() {
                                chip(ui, "Not opened yet", pal.alt, pal.sub);
                            } else {
                                chip(ui, &format!("Opened {}x, last {}", s.opened.len(), mech::list_time(*s.opened.last().unwrap(), now)), Color32::from_rgb(0xe3, 0xf6, 0xe9), Color32::from_rgb(0x1d, 0x7a, 0x3d));
                            }
                        }
                        if let Some(t) = s.follow_up {
                            if s.replied {
                                chip(ui, "Replied", pal.alt, pal.sub);
                            } else if due {
                                chip(ui, "No reply - follow up", Color32::from_rgb(0xff, 0xf1, 0xdc), Color32::from_rgb(0xa8, 0x5a, 0x00));
                            } else {
                                chip(ui, &format!("Reminder {}", mech::short_when(t, now)), pal.alt, pal.sub);
                            }
                        }
                    });
                    if due {
                        ui.horizontal(|ui| {
                            if primary_button(ui, "Nudge").clicked() {
                                nudge = Some(s.clone());
                            }
                            if ui.button("They replied").clicked() {
                                mark_replied = Some(s.id);
                            }
                        });
                    }
                });
        }
        if let Some(id) = mark_replied {
            if let Some(s) = self.sent.iter_mut().find(|s| s.id == id) {
                s.replied = true;
            }
        }
        if let Some(s) = nudge {
            let mut c = Composer::blank();
            c.to = s.to.clone();
            c.subject = format!("Re: {}", s.subject);
            c.body = format!("Hi,\n\nJust bumping this to the top of your inbox. Any thoughts?\n\n{}", self.settings.signature);
            self.composer = Some(c);
        }
        if self.sent.is_empty() {
            ui.label(RichText::new("Nothing sent yet").color(pal.sub));
        }
    }

    fn scheduled_list(&mut self, ui: &mut Ui, pal: Pal, now: DateTime<Local>) {
        let mut cancel = None;
        let mut send_now = None;
        if self.scheduled.is_empty() {
            ui.add_space(30.0);
            ui.vertical_centered(|ui| ui.label(RichText::new("No emails scheduled. Use Send Later in a new email.").color(pal.sub)));
        }
        for s in &self.scheduled {
            egui::Frame::none()
                .fill(pal.card)
                .stroke(Stroke::new(1.0_f32, pal.border))
                .rounding(Rounding::same(8.0))
                .inner_margin(egui::Margin::same(10.0))
                .outer_margin(egui::Margin::symmetric(10.0, 4.0))
                .show(ui, |ui| {
                    ui.label(RichText::new(format!("To {}", s.to)).strong().color(pal.text));
                    ui.label(RichText::new(&s.subject).size(12.5).color(pal.text));
                    ui.label(RichText::new(format!("Sends {}", mech::short_when(s.send_at, now))).size(12.0).color(ACCENT));
                    ui.horizontal(|ui| {
                        if ui.button("Send now").clicked() {
                            send_now = Some(s.id);
                        }
                        if ui.button("Cancel").clicked() {
                            cancel = Some(s.id);
                        }
                    });
                });
        }
        if let Some(id) = send_now {
            if let Some(s) = self.scheduled.iter_mut().find(|s| s.id == id) {
                s.send_at = now;
            }
        }
        if let Some(id) = cancel {
            self.scheduled.retain(|s| s.id != id);
        }
    }

    fn reader(&mut self, ui: &mut Ui, pal: Pal) {
        let now = Local::now();
        let Some(m) = self.selected.and_then(|id| self.mail(id)).cloned() else {
            ui.add_space(120.0);
            ui.vertical_centered(|ui| {
                ui.label(RichText::new("No email selected").size(16.0).color(pal.sub));
                ui.label(RichText::new("j / k to move, e to mark done, r to reply, c to compose, Cmd+K for everything").size(12.0).color(pal.sub));
            });
            return;
        };
        let mut acts = Vec::new();
        let id = m.id;
        // action bar
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.add_space(8.0);
            if tool_button(ui, if m.done { "Undo Done" } else { "Done" }, pal).clicked() {
                acts.push(Act::Done(id));
            }
            ui.menu_button(RichText::new("Snooze").size(12.5), |ui| {
                if m.snoozed_until.is_some() || m.someday {
                    if ui.button("Unsnooze").clicked() {
                        acts.push(Act::Unsnooze(id));
                        ui.close_menu();
                    }
                    ui.separator();
                }
                for p in Preset::SNOOZE {
                    if let Some(l) = p.label(now, false) {
                        if ui.button(l).clicked() {
                            acts.push(Act::Snooze(id, p));
                            ui.close_menu();
                        }
                    }
                }
            });
            if tool_button(ui, if m.pinned { "Unpin" } else { "Pin" }, pal).clicked() {
                acts.push(Act::Pin(id));
            }
            if tool_button(ui, "Delete", pal).clicked() {
                acts.push(Act::Trash(id));
            }
            ui.menu_button(RichText::new("More").size(12.5), |ui| {
                if ui.button(if m.priority { "Remove priority" } else { "Mark priority" }).clicked() {
                    acts.push(Act::Priority(id));
                    ui.close_menu();
                }
                if ui.button(if m.set_aside { "Return to inbox" } else { "Set aside" }).clicked() {
                    acts.push(Act::SetAside(id));
                    ui.close_menu();
                }
                if ui.button(if m.read { "Mark unread" } else { "Mark read" }).clicked() {
                    acts.push(Act::ToggleRead(id));
                    ui.close_menu();
                }
                ui.separator();
                ui.menu_button("Label", |ui| {
                    for l in data::LABELS {
                        let on = m.labels.iter().any(|x| x == l);
                        if ui.selectable_label(on, l).clicked() {
                            acts.push(Act::Label(id, l.to_string()));
                        }
                    }
                });
                ui.menu_button("Team", |ui| {
                    if ui.button("Share thread with team").clicked() {
                        acts.push(Act::Share(id));
                        ui.close_menu();
                    }
                    ui.separator();
                    ui.label(RichText::new("Assign to").size(11.0).color(pal.sub));
                    for t in data::TEAMMATES {
                        if ui.button(t).clicked() {
                            acts.push(Act::Assign(id, t.to_string()));
                            ui.close_menu();
                        }
                    }
                });
                ui.menu_button("Send to...", |ui| {
                    ui.label(RichText::new("On this Mac").size(11.0).color(pal.sub));
                    for i in Integration::ALL.iter().filter(|i| i.native()) {
                        if ui.button(i.name()).clicked() {
                            acts.push(Act::Export(id, *i));
                            ui.close_menu();
                        }
                    }
                    ui.separator();
                    ui.label(RichText::new("Web services").size(11.0).color(pal.sub));
                    for i in Integration::ALL.iter().filter(|i| !i.native()) {
                        if ui.button(i.name()).clicked() {
                            acts.push(Act::Export(id, *i));
                            ui.close_menu();
                        }
                    }
                });
            });
        });
        ui.separator();
        egui::ScrollArea::vertical().id_salt("reader-scroll").auto_shrink([false, false]).show(ui, |ui| {
            ui.add_space(6.0);
            egui::Frame::none().inner_margin(egui::Margin::symmetric(22.0, 4.0)).show(ui, |ui| {
                ui.label(RichText::new(&m.subject).size(21.0).strong().color(pal.text));
                ui.horizontal_wrapped(|ui| {
                    for l in &m.labels {
                        chip(ui, l, pal.alt, theme::avatar_color(l));
                    }
                    if m.new_sender {
                        chip(ui, "New sender", Color32::from_rgb(0xf0, 0xe8, 0xff), theme::PURPLE);
                    }
                    if let Some(a) = &m.assigned {
                        chip(ui, &format!("Assigned: {}", a), Color32::from_rgb(0xe3, 0xf6, 0xe9), Color32::from_rgb(0x1d, 0x7a, 0x3d));
                    }
                });
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    let (r, _) = ui.allocate_exact_size(egui::vec2(38.0, 38.0), Sense::hover());
                    avatar(ui, r.center(), 19.0, &m.from_name);
                    ui.vertical(|ui| {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(&m.from_name).strong().color(pal.text));
                            ui.label(RichText::new(format!("<{}>", m.from_addr)).size(12.0).color(pal.sub));
                        });
                        ui.label(RichText::new(format!("to {}   {}", m.to, m.received.format("%a, %b %-d at %-I:%M %p"))).size(12.0).color(pal.sub));
                    });
                });
                ui.add_space(10.0);

                // AI summary card
                egui::Frame::none()
                    .fill(if self.settings.dark { Color32::from_rgb(0x2a, 0x24, 0x3a) } else { Color32::from_rgb(0xf5, 0xf0, 0xff) })
                    .rounding(Rounding::same(8.0))
                    .inner_margin(egui::Margin::same(10.0))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("AI Summary").strong().color(theme::PURPLE));
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if ui.small_button(if m.summary.is_some() { "Refresh" } else { "Summarize" }).clicked() {
                                    acts.push(Act::Summarize(id));
                                }
                            });
                        });
                        match &m.summary {
                            Some(s) => {
                                ui.label(RichText::new(s).size(13.0).color(pal.text));
                            }
                            None => {
                                ui.label(RichText::new("Get the gist and the asks in one click.").size(12.5).color(pal.sub));
                            }
                        }
                    });
                ui.add_space(12.0);
                ui.label(RichText::new(&m.body).size(14.0).color(pal.text));
                ui.add_space(10.0);
                if !m.attachments.is_empty() {
                    ui.horizontal_wrapped(|ui| {
                        for a in &m.attachments {
                            egui::Frame::none()
                                .stroke(Stroke::new(1.0_f32, pal.border))
                                .rounding(Rounding::same(6.0))
                                .inner_margin(egui::Margin::symmetric(10.0, 6.0))
                                .show(ui, |ui| {
                                    ui.label(RichText::new(a).size(12.0).color(pal.text));
                                });
                        }
                    });
                    ui.add_space(8.0);
                }
                if m.can_unsubscribe {
                    if ui.link("Unsubscribe from this sender").clicked() {
                        acts.push(Act::Unsubscribe(id));
                    }
                    ui.add_space(6.0);
                }
                ui.separator();
                ui.label(RichText::new("Quick replies").size(12.0).color(pal.sub));
                ui.horizontal_wrapped(|ui| {
                    for (label, intent) in [("Sounds good", "interested"), ("Not for me", "not_interested"), ("Thank you!", "thanks")] {
                        if ui.add(egui::Button::new(RichText::new(label).color(ACCENT)).fill(pal.sel)).clicked() {
                            acts.push(Act::QuickReply(id, intent));
                        }
                    }
                });
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if primary_button(ui, "Reply").clicked() {
                        acts.push(Act::Reply(id, false));
                    }
                    if ui.button("Reply All").clicked() {
                        acts.push(Act::Reply(id, true));
                    }
                    if ui.button("Forward").clicked() {
                        acts.push(Act::Forward(id));
                    }
                    if !m.new_sender && ui.button("Block sender").clicked() {
                        acts.push(Act::Block(m.from_addr.clone()));
                    }
                });
                ui.add_space(14.0);

                // team comments
                egui::Frame::none()
                    .fill(pal.alt)
                    .rounding(Rounding::same(8.0))
                    .inner_margin(egui::Margin::same(10.0))
                    .show(ui, |ui| {
                        ui.label(RichText::new("Team comments").strong().color(pal.text));
                        ui.label(RichText::new("Private to your team. The sender never sees these.").size(11.5).color(pal.sub));
                        for c in &m.comments {
                            ui.horizontal(|ui| {
                                let (r, _) = ui.allocate_exact_size(egui::vec2(22.0, 22.0), Sense::hover());
                                avatar(ui, r.center(), 11.0, &c.author);
                                ui.label(RichText::new(&c.author).strong().size(12.5).color(pal.text));
                                ui.label(RichText::new(mech::list_time(c.at, now)).size(11.0).color(pal.sub));
                            });
                            ui.label(RichText::new(&c.text).size(12.5).color(pal.text));
                        }
                        ui.horizontal(|ui| {
                            let r = ui.add(egui::TextEdit::singleline(&mut self.comment_input).hint_text("Write a comment, @mention a teammate").desired_width(ui.available_width() - 70.0));
                            let enter = r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
                            if ui.button("Post").clicked() || enter {
                                acts.push(Act::Comment(id));
                            }
                        });
                    });
                ui.add_space(30.0);
            });
        });
        self.apply(acts, ui.ctx());
    }

    fn copilot(&mut self, ui: &mut Ui, pal: Pal) {
        ui.add_space(10.0);
        ui.label(RichText::new("Ask AI").size(16.0).strong().color(theme::PURPLE));
        ui.label(
            RichText::new(if self.ai_key.is_empty() {
                "Answers come from the local AI model on this Mac (plain search if it isn't running)."
            } else {
                "Answers come from your recent emails."
            })
            .size(11.5)
            .color(pal.sub),
        );
        ui.add_space(6.0);
        let suggestions = ["What needs a reply today?", "Summarize my unread emails", "Anything about the contract?", "When is the launch?"];
        let mut ask: Option<String> = None;
        ui.horizontal_wrapped(|ui| {
            for s in suggestions {
                if ui.add(egui::Button::new(RichText::new(s).size(11.5).color(theme::PURPLE)).fill(pal.alt)).clicked() {
                    ask = Some(s.to_string());
                }
            }
        });
        ui.separator();
        let input_h = 70.0;
        egui::ScrollArea::vertical()
            .id_salt("copilot-scroll")
            .max_height(ui.available_height() - input_h)
            .stick_to_bottom(true)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for (mine, text) in &self.copilot_log {
                    let (fill, fg) = if *mine { (ACCENT, Color32::WHITE) } else { (pal.alt, pal.text) };
                    let layout = if *mine { Layout::right_to_left(Align::Min) } else { Layout::left_to_right(Align::Min) };
                    ui.with_layout(layout, |ui| {
                        egui::Frame::none().fill(fill).rounding(Rounding::same(10.0)).inner_margin(egui::Margin::same(8.0)).show(ui, |ui| {
                            ui.set_max_width(260.0);
                            ui.label(RichText::new(text).size(12.5).color(fg));
                        });
                    });
                    ui.add_space(4.0);
                }
                if self.copilot_busy {
                    ui.label(RichText::new("Thinking...").color(pal.sub));
                }
            });
        ui.horizontal(|ui| {
            let r = ui.add(egui::TextEdit::singleline(&mut self.copilot_input).hint_text("Ask about your inbox").desired_width(ui.available_width() - 60.0));
            let enter = r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
            if (ui.button("Ask").clicked() || enter) && !self.copilot_input.trim().is_empty() {
                ask = Some(std::mem::take(&mut self.copilot_input));
            }
        });
        if let Some(q) = ask {
            self.copilot_log.push((true, q.clone()));
            self.copilot_busy = true;
            let list: Vec<&Mail> = self.mails.iter().filter(|m| !m.trashed).collect();
            let fallback = ai::copilot_offline(&q, &list);
            let prompt = format!("Here are my recent emails:\n\n{}\n\nQuestion: {}", ai::mail_context(&list), q);
            let ctx = ui.ctx().clone();
            self.run_ai(&ctx, "You are an email assistant. Answer briefly using only the emails provided. Name senders and subjects you rely on.", prompt, fallback, Target::Copilot);
        }
    }

    // ---------- calendar ----------

    fn calendar(&mut self, ui: &mut Ui, pal: Pal) {
        let now = Local::now();
        ui.add_space(6.0);
        if !self.cal_warning.is_empty() {
            egui::Frame::none().fill(pal.alt).rounding(Rounding::same(8.0)).inner_margin(egui::Margin::symmetric(12.0, 8.0)).outer_margin(egui::Margin::symmetric(8.0, 0.0)).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Your calendar and tasks aren't loading yet.").strong().color(ORANGE));
                    ui.label(RichText::new("Allow Flint Mail in Privacy & Security > Calendars and Reminders.").size(12.0).color(pal.sub));
                    if ui.small_button("Grant access").clicked() {
                        let tx = self.net_tx.clone();
                        let ctx = ui.ctx().clone();
                        std::thread::spawn(move || {
                            mac::grant_access_blocking();
                            let _ = tx.send(NetResult::RefreshPlanner);
                            ctx.request_repaint();
                        });
                    }
                    if ui.small_button("Open settings").clicked() {
                        let _ = mac::open_url("x-apple.systempreferences:com.apple.preference.security?Privacy_Calendars");
                    }
                });
            });
            ui.add_space(4.0);
        }
        ui.horizontal(|ui| {
            ui.add_space(8.0);
            if tool_button(ui, "Today", pal).clicked() {
                self.cal_anchor = now.date_naive();
            }
            let step = match self.cal_view {
                CalView::Day => 1,
                CalView::ThreeDays => 3,
                CalView::Week => 7,
                CalView::Month => 30,
            };
            if tool_button(ui, "<", pal).clicked() {
                self.cal_anchor = if self.cal_view == CalView::Month {
                    let d = self.cal_anchor.with_day(1).unwrap() - Duration::days(1);
                    d.with_day(1).unwrap()
                } else {
                    self.cal_anchor - Duration::days(step)
                };
            }
            if tool_button(ui, ">", pal).clicked() {
                self.cal_anchor = if self.cal_view == CalView::Month {
                    (self.cal_anchor.with_day(1).unwrap() + Duration::days(32)).with_day(1).unwrap()
                } else {
                    self.cal_anchor + Duration::days(step)
                };
            }
            ui.label(RichText::new(self.cal_anchor.format("%B %Y").to_string()).size(17.0).strong().color(pal.text));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.add_space(8.0);
                if primary_button(ui, "+ New Event").clicked() {
                    self.event_form = Some(self.new_event_form(None));
                }
                if tool_button(ui, "Snapshot", pal).on_hover_text("Capture this planner (Cmd+Shift+B)").clicked() {
                    let ctx = ui.ctx().clone();
                    self.start_snapshot(&ctx);
                }
                for (label, v) in [("Month", CalView::Month), ("Week", CalView::Week), ("3 Days", CalView::ThreeDays), ("Day", CalView::Day)] {
                    ui.selectable_value(&mut self.cal_view, v, label);
                }
            });
        });
        ui.add_space(4.0);
        match self.cal_view {
            CalView::Month => self.month_grid(ui, pal, now),
            v => {
                let days = match v {
                    CalView::Day => 1,
                    CalView::ThreeDays => 3,
                    _ => 7,
                };
                let start = if v == CalView::Week {
                    self.cal_anchor - Duration::days(self.cal_anchor.weekday().num_days_from_sunday() as i64)
                } else {
                    self.cal_anchor
                };
                self.time_grid(ui, pal, now, start, days);
            }
        }
    }

    fn visible_events(&self) -> Vec<Event> {
        self.events.iter().filter(|e| self.calendars.get(e.calendar).map_or(true, |c| c.visible)).cloned().collect()
    }

    fn time_grid(&mut self, ui: &mut Ui, pal: Pal, now: DateTime<Local>, start: NaiveDate, days: i64) {
        let gutter = 52.0;
        let head_h = 34.0;
        let events = self.visible_events();
        let all_day_on = |e: &Event, d: NaiveDate| {
            e.all_day && e.start.date_naive() <= d && d < e.end.date_naive().max(e.start.date_naive() + Duration::days(1))
        };
        let all_day_rows = (0..days)
            .map(|i| events.iter().filter(|e| all_day_on(e, start + Duration::days(i))).count())
            .max()
            .unwrap_or(0)
            .min(3);
        let strip_h = if all_day_rows > 0 { all_day_rows as f32 * 20.0 + 8.0 } else { 0.0 };
        let hour_h = ((ui.available_height() - head_h - strip_h) / mech::CALENDAR_VISIBLE_HOURS).max(30.0);
        let full_w = ui.available_width();
        let col_w = (full_w - gutter - 8.0) / days as f32;
        // day headers
        let (hrect, _) = ui.allocate_exact_size(egui::vec2(full_w, head_h), Sense::hover());
        for i in 0..days {
            let d = start + Duration::days(i);
            let x = hrect.left() + gutter + col_w * i as f32;
            let is_today = d == now.date_naive();
            let col = if is_today { ACCENT } else { pal.sub };
            ui.painter().text(egui::pos2(x + 8.0, hrect.center().y), Align2::LEFT_CENTER, d.format("%a").to_string().to_uppercase(), FontId::proportional(11.0), col);
            let num_pos = egui::pos2(x + 52.0, hrect.center().y);
            if is_today {
                ui.painter().circle_filled(num_pos, 12.0, ACCENT);
            }
            ui.painter().text(num_pos, Align2::CENTER_CENTER, d.day().to_string(), FontId::proportional(14.0), if is_today { Color32::WHITE } else { pal.text });
        }
        let mut clicked_event = None;
        let mut new_at = None;
        if strip_h > 0.0 {
            let (srect, sresp) = ui.allocate_exact_size(egui::vec2(full_w, strip_h), Sense::click());
            let sp = ui.painter_at(srect);
            sp.text(egui::pos2(srect.left() + gutter - 8.0, srect.top() + 14.0), Align2::RIGHT_CENTER, "all-day", FontId::proportional(10.0), pal.sub);
            let mut shits: Vec<(egui::Rect, u64)> = Vec::new();
            for i in 0..days {
                let d = start + Duration::days(i);
                let x = srect.left() + gutter + col_w * i as f32;
                let day_events: Vec<&Event> = events.iter().filter(|e| all_day_on(e, d)).collect();
                for (row, e) in day_events.iter().take(3).enumerate() {
                    let y = srect.top() + 4.0 + row as f32 * 20.0;
                    let er = egui::Rect::from_min_size(egui::pos2(x + 3.0, y), egui::vec2(col_w - 6.0, 17.0));
                    let color = theme::CALENDAR_COLORS[e.calendar % theme::CALENDAR_COLORS.len()];
                    let more = row == 2 && day_events.len() > 3;
                    let label = if more { format!("{} (+{} more)", e.title, day_events.len() - 3) } else { e.title.clone() };
                    sp.rect_filled(er, Rounding::same(4.0), color.gamma_multiply(if self.settings.dark { 0.45 } else { 0.28 }));
                    let g = one_line(ui, &label, 11.0, pal.text, er.width() - 10.0);
                    sp.galley(egui::pos2(er.left() + 5.0, er.top() + 2.0), g, pal.text);
                    shits.push((er, e.id));
                }
            }
            sp.line_segment([egui::pos2(srect.left() + gutter, srect.bottom()), egui::pos2(srect.right(), srect.bottom())], Stroke::new(1.0_f32, pal.border));
            if sresp.clicked() {
                if let Some(pos) = sresp.interact_pointer_pos() {
                    if let Some((_, id)) = shits.iter().find(|(r, _)| r.contains(pos)) {
                        clicked_event = Some(*id);
                    }
                }
            }
        }
        let scrolled_id = egui::Id::new("cal-scrolled");
        let mut area = egui::ScrollArea::vertical().id_salt("cal-scroll").auto_shrink([false, false]);
        if !ui.ctx().data(|d| d.get_temp::<bool>(scrolled_id).unwrap_or(false)) {
            let nowf = now.hour() as f32 + now.minute() as f32 / 60.0;
            let focus = if !(7.0..20.0).contains(&nowf) { 7.0 } else { (nowf - 1.5).max(6.0) };
            area = area.vertical_scroll_offset(focus * hour_h);
            ui.ctx().data_mut(|d| d.insert_temp(scrolled_id, true));
        }
        area.show(ui, |ui| {
                let (rect, resp) = ui.allocate_exact_size(egui::vec2(full_w, hour_h * 24.0), Sense::click());
                let p = ui.painter_at(rect);
                for h in 0..24 {
                    let y = rect.top() + h as f32 * hour_h;
                    p.line_segment([egui::pos2(rect.left() + gutter, y), egui::pos2(rect.right(), y)], Stroke::new(1.0_f32, pal.border));
                    if h > 0 {
                        let t = mech::at(now.date_naive(), h, 0).format("%-I %p").to_string();
                        p.text(egui::pos2(rect.left() + gutter - 8.0, y), Align2::RIGHT_CENTER, t, FontId::proportional(10.5), pal.sub);
                    }
                }
                for i in 0..days {
                    if start + Duration::days(i) == now.date_naive() {
                        let x = rect.left() + gutter + col_w * i as f32;
                        p.rect_filled(egui::Rect::from_min_max(egui::pos2(x, rect.top()), egui::pos2(x + col_w, rect.bottom())), Rounding::ZERO, ACCENT.gamma_multiply(if self.settings.dark { 0.10 } else { 0.05 }));
                    }
                }
                for i in 0..=days {
                    let x = rect.left() + gutter + col_w * i as f32;
                    p.line_segment([egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())], Stroke::new(1.0_f32, pal.border));
                }
                let mut hit: Vec<(egui::Rect, u64)> = Vec::new();
                for i in 0..days {
                    let d = start + Duration::days(i);
                    let mut todays: Vec<&Event> = events.iter().filter(|e| !e.all_day && e.start.date_naive() == d).collect();
                    todays.sort_by_key(|e| e.start);
                    let x = rect.left() + gutter + col_w * i as f32;
                    // side-by-side lanes for events that overlap in time
                    let mut lane_end: Vec<DateTime<Local>> = Vec::new();
                    let mut lanes = Vec::new();
                    for e in &todays {
                        let lane = match lane_end.iter().position(|end| *end <= e.start) {
                            Some(l) => l,
                            None => {
                                lane_end.push(e.end);
                                lane_end.len() - 1
                            }
                        };
                        lane_end[lane] = e.end;
                        lanes.push(lane);
                    }
                    let mut hidden = 0usize;
                    let mut hidden_y = 0.0f32;
                    for (k, e) in todays.iter().enumerate() {
                        if lanes[k] >= 3 {
                            if hidden == 0 {
                                hidden_y = rect.top() + (e.start.hour() as f32 + e.start.minute() as f32 / 60.0) * hour_h;
                            }
                            hidden += 1;
                            continue;
                        }
                        let width_lanes = todays
                            .iter()
                            .zip(lanes.iter())
                            .filter(|(o, _)| o.start < e.end && o.end > e.start)
                            .map(|(_, l)| *l + 1)
                            .max()
                            .unwrap_or(1)
                            .min(3) as f32;
                        let lane_w = (col_w - 6.0) / width_lanes;
                        let y0 = rect.top() + (e.start.hour() as f32 + e.start.minute() as f32 / 60.0) * hour_h;
                        let mins = (e.end - e.start).num_minutes().max(20) as f32;
                        let y1 = (y0 + mins / 60.0 * hour_h).min(rect.bottom());
                        let lx = x + 3.0 + lane_w * lanes[k] as f32;
                        let er = egui::Rect::from_min_max(egui::pos2(lx, y0 + 1.0), egui::pos2(lx + lane_w - 2.0, y1 - 1.0));
                        let color = theme::CALENDAR_COLORS[e.calendar % theme::CALENDAR_COLORS.len()];
                        let selected = self.selected_event == Some(e.id);
                        let fill = if selected { color } else { color.gamma_multiply(0.18) };
                        p.rect_filled(er, Rounding::same(5.0), if self.settings.dark && !selected { color.gamma_multiply(0.35) } else { fill });
                        p.rect_filled(egui::Rect::from_min_max(er.min, egui::pos2(er.left() + 3.0, er.bottom())), Rounding::same(2.0), color);
                        let fg = if selected { Color32::WHITE } else { pal.text };
                        let head = if er.height() <= 34.0 && er.width() > 90.0 { format!("{}  {}", e.start.format("%-I:%M"), e.title) } else { e.title.clone() };
                        let g = one_line(ui, &head, 12.0, fg, er.width() - 12.0);
                        p.galley(egui::pos2(er.left() + 8.0, er.top() + 3.0), g, fg);
                        if er.height() > 34.0 {
                            let t = format!("{} - {}", e.start.format("%-I:%M"), e.end.format("%-I:%M %p"));
                            let g = one_line(ui, &t, 10.5, if selected { Color32::WHITE } else { pal.sub }, er.width() - 12.0);
                            p.galley(egui::pos2(er.left() + 8.0, er.top() + 19.0), g, pal.sub);
                        }
                        hit.push((er, e.id));
                    }
                    if hidden > 0 {
                        let label = format!("+{} more", hidden);
                        let g = p.layout_no_wrap(label, FontId::proportional(10.5), pal.text);
                        let r = egui::Rect::from_min_size(egui::pos2(x + col_w - g.size().x - 12.0, hidden_y + 2.0), g.size() + egui::vec2(8.0, 2.0));
                        p.rect_filled(r, Rounding::same(4.0), pal.alt);
                        p.rect_stroke(r, Rounding::same(4.0), Stroke::new(1.0_f32, pal.border));
                        p.galley(r.min + egui::vec2(4.0, 1.0), g, pal.text);
                    }
                    if d == now.date_naive() {
                        let y = rect.top() + (now.hour() as f32 + now.minute() as f32 / 60.0) * hour_h;
                        p.line_segment([egui::pos2(x, y), egui::pos2(x + col_w, y)], Stroke::new(2.0_f32, RED));
                        p.circle_filled(egui::pos2(x, y), 4.0, RED);
                    }
                }
                if resp.clicked() || resp.double_clicked() {
                    if let Some(pos) = resp.interact_pointer_pos() {
                        if let Some((_, id)) = hit.iter().rev().find(|(r, _)| r.contains(pos)) {
                            clicked_event = Some(*id);
                        } else if resp.double_clicked() && pos.x > rect.left() + gutter {
                            let day = ((pos.x - rect.left() - gutter) / col_w).floor() as i64;
                            let hour_f = (pos.y - rect.top()) / hour_h;
                            let h = hour_f.floor() as u32;
                            let m = if hour_f.fract() >= 0.5 { 30 } else { 0 };
                            new_at = Some(mech::at(start + Duration::days(day), h.min(23), m));
                        }
                    }
                }
            });
        if let Some(id) = clicked_event {
            self.selected_event = Some(id);
        }
        if let Some(t) = new_at {
            self.event_form = Some(self.new_event_form(Some(t)));
        }
    }

    fn month_grid(&mut self, ui: &mut Ui, pal: Pal, now: DateTime<Local>) {
        let first = self.cal_anchor.with_day(1).unwrap();
        let start = first - Duration::days(first.weekday().num_days_from_sunday() as i64);
        let w = ui.available_width();
        let h = ui.available_height() - 8.0;
        let (rect, resp) = ui.allocate_exact_size(egui::vec2(w, h), Sense::click());
        let p = ui.painter_at(rect);
        let head = 24.0;
        let cw = w / 7.0;
        let ch = (h - head) / 6.0;
        for (i, d) in ["SUN", "MON", "TUE", "WED", "THU", "FRI", "SAT"].iter().enumerate() {
            p.text(egui::pos2(rect.left() + cw * i as f32 + 8.0, rect.top() + head / 2.0), Align2::LEFT_CENTER, *d, FontId::proportional(11.0), pal.sub);
        }
        let events = self.visible_events();
        let mut hit = Vec::new();
        for i in 0..42 {
            let d = start + Duration::days(i);
            let cell = egui::Rect::from_min_size(
                egui::pos2(rect.left() + cw * (i % 7) as f32, rect.top() + head + ch * (i / 7) as f32),
                egui::vec2(cw, ch),
            );
            if d == now.date_naive() {
                p.rect_filled(cell, Rounding::ZERO, ACCENT.gamma_multiply(if self.settings.dark { 0.10 } else { 0.06 }));
            } else if d.month() != first.month() {
                p.rect_filled(cell, Rounding::ZERO, pal.alt.gamma_multiply(0.6));
            }
            p.rect_stroke(cell, Rounding::ZERO, Stroke::new(1.0_f32, pal.border));
            let num_pos = egui::pos2(cell.left() + 16.0, cell.top() + 14.0);
            if d == now.date_naive() {
                p.circle_filled(num_pos, 11.0, ACCENT);
            }
            let col = if d == now.date_naive() {
                Color32::WHITE
            } else if d.month() != first.month() {
                pal.sub
            } else {
                pal.text
            };
            p.text(num_pos, Align2::CENTER_CENTER, d.day().to_string(), FontId::proportional(12.5), col);
            let mut todays: Vec<&Event> = events.iter().filter(|e| e.start.date_naive() == d).collect();
            todays.sort_by_key(|e| e.start);
            let max = ((ch - 30.0) / 18.0).floor().max(0.0) as usize;
            for (k, e) in todays.iter().take(max).enumerate() {
                let r = egui::Rect::from_min_size(egui::pos2(cell.left() + 4.0, cell.top() + 28.0 + k as f32 * 18.0), egui::vec2(cw - 8.0, 16.0));
                let color = theme::CALENDAR_COLORS[e.calendar % theme::CALENDAR_COLORS.len()];
                p.rect_filled(r, Rounding::same(4.0), color.gamma_multiply(0.18));
                p.circle_filled(egui::pos2(r.left() + 7.0, r.center().y), 3.0, color);
                let g = one_line(ui, &format!("{} {}", e.start.format("%-I:%M"), e.title), 11.0, pal.text, r.width() - 16.0);
                p.galley(egui::pos2(r.left() + 14.0, r.top() + 1.0), g, pal.text);
                hit.push((r, e.id, d));
            }
            if todays.len() > max && max > 0 {
                p.text(egui::pos2(cell.right() - 6.0, cell.bottom() - 8.0), Align2::RIGHT_CENTER, format!("+{} more", todays.len() - max), FontId::proportional(10.5), pal.sub);
            }
        }
        if resp.clicked() || resp.double_clicked() {
            if let Some(pos) = resp.interact_pointer_pos() {
                if let Some((_, id, _)) = hit.iter().find(|(r, _, _)| r.contains(pos)) {
                    self.selected_event = Some(*id);
                } else {
                    let col = ((pos.x - rect.left()) / cw).floor() as i64;
                    let row = ((pos.y - rect.top() - head) / ch).floor() as i64;
                    if (0..7).contains(&col) && (0..6).contains(&row) {
                        let d = start + Duration::days(row * 7 + col);
                        if resp.double_clicked() {
                            self.event_form = Some(self.new_event_form(Some(mech::at(d, mech::MORNING_HOUR, 0))));
                        } else {
                            self.cal_anchor = d;
                        }
                    }
                }
            }
        }
    }

    fn free_slots(&self, from: DateTime<Local>, count: usize) -> Vec<DateTime<Local>> {
        let mut out = Vec::new();
        let mut t = mech::round_up_half_hour(from);
        let mut guard = 0;
        while out.len() < count && guard < 400 {
            guard += 1;
            let h = t.hour();
            if t.weekday().number_from_monday() <= 5 && (9..17).contains(&h) {
                let end = t + Duration::minutes(30);
                let busy = self.events.iter().any(|e| e.start < end && e.end > t);
                if !busy {
                    out.push(t);
                    t += Duration::hours(2);
                    continue;
                }
            }
            t += Duration::minutes(30);
        }
        out
    }

    fn new_event_form(&self, at: Option<DateTime<Local>>) -> EventForm {
        let t = at.unwrap_or_else(|| mech::round_up_half_hour(Local::now()));
        EventForm {
            title: String::new(),
            date: t.format("%Y-%m-%d").to_string(),
            time: t.format("%H:%M").to_string(),
            minutes: 30,
            calendar: 0,
            location: String::new(),
            link: String::new(),
            attendees: String::new(),
            editing: None,
        }
    }

    fn tasks_panel(&mut self, ui: &mut Ui, pal: Pal) {
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            ui.label(RichText::new("Tasks").size(16.0).strong().color(pal.text));
            ui.label(RichText::new(format!("{}", self.tasks.len())).size(12.0).color(pal.sub));
            if ui.small_button("Refresh").clicked() {
                self.tasks_syncing = false;
                let ctx = ui.ctx().clone();
                self.sync_tasks(&ctx);
            }
        });
        ui.add_space(4.0);
        if !self.cal_warning.is_empty() {
            egui::Frame::none().fill(pal.alt).rounding(Rounding::same(8.0)).inner_margin(egui::Margin::same(8.0)).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(RichText::new(&self.cal_warning).size(11.5).color(ORANGE));
                ui.horizontal_wrapped(|ui| {
                    if ui.small_button("Calendars").clicked() {
                        let _ = mac::open_url("x-apple.systempreferences:com.apple.preference.security?Privacy_Calendars");
                    }
                    if ui.small_button("Reminders").clicked() {
                        let _ = mac::open_url("x-apple.systempreferences:com.apple.preference.security?Privacy_Reminders");
                    }
                    if ui.small_button("Automation").clicked() {
                        let _ = mac::open_url("x-apple.systempreferences:com.apple.preference.security?Privacy_Automation");
                    }
                });
            });
            ui.add_space(6.0);
        }
        let r = ui.add(egui::TextEdit::singleline(&mut self.task_input).hint_text("Add a task (try: call Mom tomorrow at 3pm)").desired_width(f32::INFINITY));
        if r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) && !self.task_input.trim().is_empty() {
            let (title, due) = parse_quick_task(self.task_input.trim(), Local::now());
            self.task_input.clear();
            self.tasks.push(mac::Task { id: String::new(), list: "Reminders".into(), title: title.clone(), due, flagged: false });
            let tx = self.net_tx.clone();
            let ctx = ui.ctx().clone();
            std::thread::spawn(move || {
                if mac::add_reminder(&title, due) {
                    let _ = tx.send(NetResult::Tasks(mac::planner().map(|d| d.tasks)));
                    ctx.request_repaint();
                }
            });
            r.request_focus();
        }
        ui.add_space(6.0);
        if self.tasks.is_empty() {
            ui.label(RichText::new(if self.tasks_syncing { "Loading your reminders..." } else { "No open reminders." }).size(12.0).color(pal.sub));
            return;
        }
        let now = Local::now();
        let today = now.date_naive();
        let mut done: Option<usize> = None;
        let mut last_group = "";
        for (i, t) in self.tasks.iter().take(40).enumerate() {
            let group = match t.due {
                Some(d) if d.date_naive() < today => "OVERDUE",
                Some(d) if d.date_naive() == today => "TODAY",
                Some(d) if d.date_naive() <= today + Duration::days(7) => "THIS WEEK",
                Some(_) => "LATER",
                None => "NO DATE",
            };
            if group != last_group {
                last_group = group;
                ui.add_space(6.0);
                ui.label(RichText::new(group).size(10.5).strong().color(if group == "OVERDUE" { RED } else { pal.sub }));
                ui.add_space(2.0);
            }
            ui.horizontal(|ui| {
                let (ring, resp) = ui.allocate_exact_size(egui::vec2(18.0, 18.0), Sense::click());
                let hot = resp.hovered();
                ui.painter().circle_stroke(ring.center(), 7.5, Stroke::new(1.6_f32, if hot { GREEN } else { pal.sub }));
                if hot {
                    ui.painter().circle_filled(ring.center(), 4.0, GREEN.gamma_multiply(0.6));
                }
                if resp.on_hover_text("Mark done").clicked() {
                    done = Some(i);
                }
                ui.vertical(|ui| {
                    let g = one_line(ui, &t.title, 14.0, pal.text, ui.available_width() - 4.0);
                    let (tr, tresp) = ui.allocate_exact_size(g.size(), Sense::hover());
                    ui.painter().galley(tr.min, g, pal.text);
                    tresp.on_hover_text(&t.title);
                    let (when, color) = match t.due {
                        Some(d) if d.date_naive() < today => (d.format("%b %-d").to_string(), RED),
                        Some(d) if d.date_naive() == today => (d.format("%-I:%M %p").to_string(), ORANGE),
                        Some(d) => (d.format("%a %b %-d").to_string(), pal.sub),
                        None => (String::new(), pal.sub),
                    };
                    let sub = if when.is_empty() { t.list.clone() } else { format!("{}  -  {}", when, t.list) };
                    ui.label(RichText::new(sub).size(11.0).color(color));
                });
            });
            ui.add_space(3.0);
        }
        if let Some(i) = done {
            let t = self.tasks.remove(i);
            let id = t.id;
            std::thread::spawn(move || mac::complete_reminder(&id));
        }
    }

    fn event_details(&mut self, ui: &mut Ui, pal: Pal) {
        let now = Local::now();
        let Some(idx) = self.selected_event.and_then(|id| self.events.iter().position(|e| e.id == id)) else {
            ui.add_space(12.0);
            ui.label(RichText::new("Agenda").size(16.0).strong().color(pal.text));
            ui.add_space(6.0);
            let list: Vec<(u64, String, DateTime<Local>, usize)> = self.upcoming_events(now).iter().take(if self.tasks.is_empty() { 5 } else { 3 }).map(|e| (e.id, e.title.clone(), e.start, e.calendar)).collect();
            if list.is_empty() {
                ui.label(RichText::new("Nothing coming up. Double-click the calendar to add an event.").size(12.0).color(pal.sub));
            }
            for (id, title, start, cal) in list {
                let color = theme::CALENDAR_COLORS[cal % theme::CALENDAR_COLORS.len()];
                let r = egui::Frame::none().fill(pal.alt).rounding(Rounding::same(8.0)).inner_margin(egui::Margin::same(8.0)).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        let (r, _) = ui.allocate_exact_size(egui::vec2(4.0, 30.0), Sense::hover());
                        ui.painter().rect_filled(r, Rounding::same(2.0), color);
                        ui.vertical(|ui| {
                            ui.label(RichText::new(&title).strong().color(pal.text));
                            ui.label(RichText::new(format!("{}  ({})", mech::short_when(start, now), mech::countdown(start, now))).size(11.5).color(pal.sub));
                        });
                    });
                });
                if r.response.interact(Sense::click()).clicked() {
                    self.selected_event = Some(id);
                }
                ui.add_space(4.0);
            }
            self.tasks_panel(ui, pal);
            return;
        };
        let e = self.events[idx].clone();
        let color = theme::CALENDAR_COLORS[e.calendar % theme::CALENDAR_COLORS.len()];
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            if ui.small_button("< Agenda").clicked() {
                self.selected_event = None;
            }
        });
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            let (r, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), Sense::hover());
            ui.painter().circle_filled(r.center(), 5.0, color);
            ui.label(RichText::new(&e.title).size(18.0).strong().color(pal.text));
        });
        ui.label(RichText::new(format!("{}  -  {}", e.start.format("%a, %b %-d  %-I:%M %p"), e.end.format("%-I:%M %p"))).color(pal.sub));
        ui.label(RichText::new(format!("{}  |  {}", self.calendars.get(e.calendar).map(|c| c.name.as_str()).unwrap_or(""), mech::countdown(e.start, now))).size(12.0).color(pal.sub));
        if !e.location.is_empty() {
            ui.label(RichText::new(format!("Location: {}", e.location)).color(pal.text));
        }
        ui.add_space(6.0);
        if let Some(link) = &e.link {
            ui.horizontal(|ui| {
                if primary_button(ui, "Join meeting").clicked() {
                    if let Err(err) = mac::open_url(link) {
                        self.toast(err);
                    }
                }
                if ui.button("Copy link").clicked() {
                    ui.ctx().output_mut(|o| o.copied_text = link.clone());
                }
            });
        }
        if !e.attendees.is_empty() {
            ui.add_space(6.0);
            ui.label(RichText::new("Guests").strong().color(pal.text));
            for a in &e.attendees {
                ui.horizontal(|ui| {
                    let (r, _) = ui.allocate_exact_size(egui::vec2(20.0, 20.0), Sense::hover());
                    avatar(ui, r.center(), 10.0, a);
                    ui.label(RichText::new(a).size(12.5).color(pal.text));
                });
            }
            if ui.button("Email guests").clicked() {
                let mut c = Composer::blank();
                c.to = e.attendees.iter().map(|a| format!("{}@studio.example", a.split_whitespace().next().unwrap_or("").to_lowercase())).collect::<Vec<_>>().join(", ");
                c.subject = e.title.clone();
                c.body = format!("Hi all,\n\nAbout {} on {}:\n\n\n{}", e.title, e.start.format("%a %b %-d at %-I:%M %p"), self.settings.signature);
                self.composer = Some(c);
            }
        }
        ui.add_space(8.0);
        ui.label(RichText::new("Meeting notes").strong().color(pal.text));
        ui.add(egui::TextEdit::multiline(&mut self.events[idx].notes).desired_rows(7).desired_width(f32::INFINITY).hint_text("Take notes during the meeting..."));
        ui.horizontal(|ui| {
            if ui.add(egui::Button::new(RichText::new("Summarize notes with AI").color(Color32::WHITE)).fill(theme::PURPLE)).clicked() {
                let notes = self.events[idx].notes.clone();
                let fallback = ai::sentences(&notes).into_iter().take(3).map(|s| format!("- {}", s)).collect::<Vec<_>>().join("\n");
                let prompt = format!("Summarize these meeting notes for '{}' into key decisions and action items with owners:\n\n{}", e.title, notes);
                let ctx = ui.ctx().clone();
                self.run_ai(&ctx, "You turn meeting notes into crisp summaries.", prompt, if fallback.is_empty() { "No notes yet.".into() } else { fallback }, Target::EventNotes(e.id));
            }
            if ui.button("Send to Reminders").clicked() {
                match Integration::Reminders.export(&e.title, &self.events[idx].notes) {
                    Ok((m, _)) => self.toast(m),
                    Err(err) => self.toast(err),
                }
            }
        });
        ui.add_space(8.0);
        if ui.button("Edit").clicked() {
            self.event_form = Some(EventForm {
                title: e.title.clone(),
                date: e.start.format("%Y-%m-%d").to_string(),
                time: e.start.format("%H:%M").to_string(),
                minutes: (e.end - e.start).num_minutes().max(1),
                calendar: e.calendar,
                location: e.location.clone(),
                link: e.link.clone().unwrap_or_default(),
                attendees: e.attendees.join(", "),
                editing: Some(e.id),
            });
        }
        ui.add_space(4.0);
        let confirming = self.confirm_delete == Some(e.id);
        let label = if confirming { "Click again to delete from your calendar" } else { "Delete event" };
        if ui.add(egui::Button::new(RichText::new(label).color(RED)).frame(false)).clicked() {
            if confirming {
                if mac::has_helper() && self.screenshot.is_none() {
                    let cal = self.calendars.get(e.calendar).map(|c| c.name.clone()).unwrap_or_default();
                    let (title, start) = (e.title.clone(), e.start);
                    std::thread::spawn(move || {
                        mac::delete_event(&cal, &title, start);
                    });
                }
                self.events.remove(idx);
                self.selected_event = None;
                self.confirm_delete = None;
            } else {
                self.confirm_delete = Some(e.id);
            }
        }
    }

    // ---------- windows ----------

    fn composer_window(&mut self, ctx: &egui::Context, pal: Pal) {
        let Some(mut c) = self.composer.take() else { return };
        let mut open = true;
        let mut send: Option<Option<DateTime<Local>>> = None;
        let mut ai_req: Option<(String, String, String)> = None;
        let now = Local::now();
        let accounts = self.accounts();
        let slots = self.free_slots(now, 3);
        egui::Window::new(if c.reply_to.is_some() { "Reply" } else { "New Email" })
            .open(&mut open)
            .default_size([660.0, 560.0])
            .default_pos([340.0, 90.0])
            .collapsible(false)
            .show(ctx, |ui| {
                egui::Grid::new("composer-fields").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
                    ui.label(RichText::new("From").color(pal.sub));
                    ui.menu_button(accounts.get(c.from).cloned().unwrap_or_default(), |ui| {
                        for (i, a) in accounts.iter().enumerate() {
                            if ui.button(a).clicked() {
                                c.from = i;
                                ui.close_menu();
                            }
                        }
                    });
                    ui.end_row();
                    ui.label(RichText::new("To").color(pal.sub));
                    ui.add(egui::TextEdit::singleline(&mut c.to).desired_width(f32::INFINITY).hint_text("name@example.com, ..."));
                    ui.end_row();
                    ui.label(RichText::new("Subject").color(pal.sub));
                    ui.add(egui::TextEdit::singleline(&mut c.subject).desired_width(f32::INFINITY));
                    ui.end_row();
                });
                ui.separator();
                ui.horizontal_wrapped(|ui| {
                    ui.menu_button("Templates", |ui| {
                        for t in &self.settings.templates {
                            if ui.button(&t.name).clicked() {
                                let first = c.to_name.split_whitespace().next().unwrap_or("there").to_string();
                                c.body = format!("{}\n\n{}", t.body.replace("{name}", &first), self.settings.signature);
                                if c.subject.is_empty() {
                                    c.subject = t.subject.clone();
                                }
                                ui.close_menu();
                            }
                        }
                    });
                    if ui.button("Signature").clicked() {
                        c.body = format!("{}\n\n{}", c.body.trim_end(), self.settings.signature);
                    }
                    if ui.button("Attach file").clicked() {
                        let tx = self.net_tx.clone();
                        let ctx = ui.ctx().clone();
                        std::thread::spawn(move || {
                            if let Some(p) = mac::choose_file() {
                                let _ = tx.send(NetResult::Attached(p));
                                ctx.request_repaint();
                            }
                        });
                    }
                    ui.menu_button("Share availability", |ui| {
                        ui.label(RichText::new("Free times from your calendar").size(11.0).color(pal.sub));
                        let text = slots.iter().map(|s| format!("- {}", s.format("%a %b %-d, %-I:%M %p"))).collect::<Vec<_>>().join("\n");
                        ui.label(&text);
                        if ui.button("Insert").clicked() {
                            c.body = format!("{}\n\nHere are a few times that work for me:\n{}\n", c.body.trim_end(), text);
                            ui.close_menu();
                        }
                    });
                    ui.menu_button("Video link", |ui| {
                        for (n, url) in [("Zoom", "https://zoom.us/j/"), ("Teams", "https://teams.microsoft.com/l/meetup-join/"), ("Meet", "https://meet.google.com/")] {
                            if ui.button(format!("Add {} link", n)).clicked() {
                                c.body = format!("{}\n\nJoin: {}  (paste your meeting ID)\n", c.body.trim_end(), url);
                                ui.close_menu();
                            }
                        }
                    });
                    ui.checkbox(&mut c.read_receipt, "Read receipt");
                    ui.menu_button(
                        match c.follow_up_days {
                            Some(d) => format!("Remind me: {}d", d),
                            None => "Follow-up reminder".into(),
                        },
                        |ui| {
                            if ui.button("None").clicked() {
                                c.follow_up_days = None;
                                ui.close_menu();
                            }
                            for d in mech::FOLLOW_UP_DAYS {
                                if ui.button(format!("If no reply in {} day{}", d, if d == 1 { "" } else { "s" })).clicked() {
                                    c.follow_up_days = Some(d);
                                    ui.close_menu();
                                }
                            }
                        },
                    );
                    if ui.selectable_label(c.show_ai, "AI Assistant").clicked() {
                        c.show_ai = !c.show_ai;
                    }
                });
                if c.show_ai {
                    egui::Frame::none()
                        .fill(if self.settings.dark { Color32::from_rgb(0x2a, 0x24, 0x3a) } else { Color32::from_rgb(0xf5, 0xf0, 0xff) })
                        .rounding(Rounding::same(8.0))
                        .inner_margin(egui::Margin::same(8.0))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.add(egui::TextEdit::singleline(&mut c.ai_prompt).hint_text("Describe the email you want, e.g. 'decline politely, offer next month'").desired_width(ui.available_width() - 80.0));
                                if ui.add(egui::Button::new(RichText::new("Write").color(Color32::WHITE)).fill(theme::PURPLE)).clicked() && !c.ai_prompt.trim().is_empty() {
                                    let reply_ctx = c.reply_to.and_then(|id| self.mail(id)).map(|m| format!("\n\nThis is a reply to:\nFrom: {}\nSubject: {}\n{}", m.from_name, m.subject, m.body)).unwrap_or_default();
                                    ai_req = Some((
                                        "You write clear, friendly emails. Return only the email body.".into(),
                                        format!("Write an email to {}. Instructions: {}. Sign off with:\n{}{}", if c.to_name.is_empty() { c.to.clone() } else { c.to_name.clone() }, c.ai_prompt, self.settings.signature, reply_ctx),
                                        format!("{}\n\n{}", ai::draft_from_prompt(&c.ai_prompt, &c.to_name), self.settings.signature),
                                    ));
                                }
                            });
                            ui.horizontal_wrapped(|ui| {
                                let body = c.body.clone();
                                let name = if c.to_name.is_empty() { "there".to_string() } else { c.to_name.clone() };
                                let mut edit = |label: &str, instr: &str, fb: String, ui: &mut Ui| {
                                    if ui.button(label).clicked() && !body.trim().is_empty() {
                                        ai_req = Some((
                                            "You edit emails. Return only the edited email body, nothing else.".into(),
                                            format!("{}:\n\n{}", instr, body),
                                            fb,
                                        ));
                                    }
                                };
                                edit("Rephrase", "Rephrase this email, keep the meaning", ai::paragraphs(&body), ui);
                                edit("Shorten", "Make this email shorter", ai::shorten(&body), ui);
                                edit("Expand", "Expand this email with a bit more detail", ai::expand(&body), ui);
                                edit("Proofread", "Fix spelling and grammar in this email", ai::proofread(&body), ui);
                                edit("Paragraphs", "Break this email into short paragraphs", ai::paragraphs(&body), ui);
                                ui.menu_button("Tone", |ui| {
                                    for (l, t) in [("Friendly", Tone::Friendly), ("Neutral", Tone::Neutral), ("Formal", Tone::Formal)] {
                                        if ui.button(l).clicked() && !body.trim().is_empty() {
                                            ai_req = Some((
                                                "You edit emails. Return only the edited email body.".into(),
                                                format!("Rewrite this email in a {} tone:\n\n{}", l.to_lowercase(), body),
                                                ai::apply_tone(&body, t, &name),
                                            ));
                                            ui.close_menu();
                                        }
                                    }
                                });
                                ui.menu_button("Translate", |ui| {
                                    for lang in ["English", "Spanish", "French", "German", "Italian", "Portuguese", "Japanese", "Chinese", "Ukrainian"] {
                                        if ui.button(lang).clicked() && !body.trim().is_empty() {
                                            ai_req = Some((
                                                "You translate emails. Return only the translation.".into(),
                                                format!("Translate this email into {}:\n\n{}", lang, body),
                                                format!("{}\n\n(Translation needs an AI key - add one in Settings.)", body),
                                            ));
                                            ui.close_menu();
                                        }
                                    }
                                });
                                if c.previous_body.is_some() && ui.button("Undo AI").clicked() {
                                    c.body = c.previous_body.take().unwrap_or_default();
                                }
                            });
                            if c.ai_busy {
                                ui.label(RichText::new("Writing...").size(11.5).color(theme::PURPLE));
                            } else if !c.ai_note.is_empty() {
                                ui.label(RichText::new(&c.ai_note).size(11.5).color(pal.sub));
                            }
                        });
                }
                let body_h = (ui.available_height() - 44.0).max(120.0);
                egui::ScrollArea::vertical().id_salt("composer-body").max_height(body_h).show(ui, |ui| {
                    if !c.attachments.is_empty() {
            let mut remove = None;
            ui.horizontal_wrapped(|ui| {
                for (i, a) in c.attachments.iter().enumerate() {
                    let name = a.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                    if ui.small_button(format!("{}  x", name)).on_hover_text("Remove attachment").clicked() {
                        remove = Some(i);
                    }
                }
            });
            if let Some(i) = remove {
                c.attachments.remove(i);
            }
        }
        let body_id = egui::Id::new("composer-body");
        if c.focus_top {
            c.focus_top = false;
            let mut st = egui::text_edit::TextEditState::load(ui.ctx(), body_id).unwrap_or_default();
            st.cursor.set_char_range(Some(egui::text::CCursorRange::one(egui::text::CCursor::new(0))));
            st.store(ui.ctx(), body_id);
            ui.ctx().memory_mut(|m| m.request_focus(body_id));
        }
        ui.add_sized([ui.available_width(), body_h], egui::TextEdit::multiline(&mut c.body).id(body_id).hint_text("Write your message..."));
                });
                ui.horizontal(|ui| {
                    let cmd_enter = ui.input(|i| i.modifiers.command && i.key_pressed(Key::Enter));
                    if primary_button(ui, "Send  (Cmd+Enter)").clicked() || cmd_enter {
                        send = Some(None);
                    }
                    ui.menu_button("Send Later", |ui| {
                        for p in Preset::SEND_LATER {
                            if let Some(l) = p.label(now, true) {
                                if ui.button(l).clicked() {
                                    send = Some(p.resolve(now, true));
                                    ui.close_menu();
                                }
                            }
                        }
                    });
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(RichText::new(format!("Undo send: {}s", self.settings.undo_send_secs)).size(11.0).color(pal.sub));
                    });
                });
            });
        if let Some((sys, prompt, fb)) = ai_req {
            c.ai_busy = true;
            self.composer = Some(c);
            self.run_ai(ctx, &sys, prompt, fb, Target::Composer);
            return;
        }
        if !open {
            if send.is_none() && c.reply_to.is_none() && (!c.body.trim().is_empty() || !c.subject.trim().is_empty() || !c.to.trim().is_empty()) {
                self.draft = Some(c);
                self.toast("Draft saved. New Email brings it back");
            }
            return;
        }
        self.composer = Some(c);
        if let Some(at) = send {
            self.send_composer(at);
        }
    }

    fn event_window(&mut self, ctx: &egui::Context, pal: Pal) {
        let Some(mut f) = self.event_form.take() else { return };
        let mut open = true;
        let mut save = false;
        let mut find_time = false;
        egui::Window::new(if f.editing.is_some() { "Edit Event" } else { "New Event" }).open(&mut open).collapsible(false).resizable(false).default_pos([420.0, 140.0]).show(ctx, |ui| {
            egui::Grid::new("event-grid").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
                ui.label("Title");
                ui.text_edit_singleline(&mut f.title);
                ui.end_row();
                ui.label("Date");
                ui.add(egui::TextEdit::singleline(&mut f.date).hint_text("YYYY-MM-DD"));
                ui.end_row();
                ui.label("Starts");
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut f.time).desired_width(60.0).hint_text("HH:MM"));
                    if ui.small_button("Find a free time").clicked() {
                        find_time = true;
                    }
                });
                ui.end_row();
                ui.label("Length");
                ui.horizontal(|ui| {
                    for m in [15, 30, 45, 60, 90] {
                        ui.selectable_value(&mut f.minutes, m, format!("{}m", m));
                    }
                });
                ui.end_row();
                ui.label("Calendar");
                ui.horizontal(|ui| {
                    for (i, c) in self.calendars.iter().enumerate() {
                        ui.selectable_value(&mut f.calendar, i, &c.name);
                    }
                });
                ui.end_row();
                ui.label("Location");
                ui.text_edit_singleline(&mut f.location);
                ui.end_row();
                ui.label("Video link");
                ui.add(egui::TextEdit::singleline(&mut f.link).hint_text("Paste a Zoom, Teams or Meet link"));
                ui.end_row();
                ui.label("Guests");
                ui.add(egui::TextEdit::singleline(&mut f.attendees).hint_text("Names, separated by commas"));
                ui.end_row();
            });
            ui.label(RichText::new("You'll get a Mac notification one minute before it starts.").size(11.5).color(pal.sub));
            ui.horizontal(|ui| {
                if primary_button(ui, "Save").clicked() {
                    save = true;
                }
            });
        });
        if find_time {
            if let Some(t) = self.free_slots(Local::now(), 1).first() {
                f.date = t.format("%Y-%m-%d").to_string();
                f.time = t.format("%H:%M").to_string();
            }
        }
        if save {
            let date = NaiveDate::parse_from_str(f.date.trim(), "%Y-%m-%d");
            let mut parts = f.time.trim().split(':');
            let h = parts.next().and_then(|x| x.parse::<u32>().ok());
            let m = parts.next().and_then(|x| x.parse::<u32>().ok()).unwrap_or(0);
            match (date, h) {
                (Ok(d), Some(h)) if h < 24 => {
                    let start = mech::at(d, h, m);
                    if let Some(eid) = f.editing {
                        if let Some(i) = self.events.iter().position(|e| e.id == eid) {
                            let old = self.events[i].clone();
                            let end = start + Duration::minutes(f.minutes);
                            let title = if f.title.trim().is_empty() { "New event".to_string() } else { f.title.trim().to_string() };
                            let link = f.link.trim().to_string();
                            if mac::has_helper() && self.screenshot.is_none() {
                                let cal = self.calendars.get(old.calendar).map(|c| c.name.clone()).unwrap_or_default();
                                let (ot, os, nt, loc, lk) = (old.title.clone(), old.start, title.clone(), f.location.clone(), link.clone());
                                std::thread::spawn(move || {
                                    mac::update_event(&cal, &ot, os, &nt, start, end, &loc, &lk);
                                });
                            }
                            let ev = &mut self.events[i];
                            ev.title = title;
                            ev.start = start;
                            ev.end = end;
                            ev.location = f.location.clone();
                            ev.link = (!link.is_empty()).then(|| link);
                            ev.attendees = f.attendees.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
                            ev.notified = start < Local::now();
                            self.cal_anchor = d;
                            self.toast("Event updated");
                            return;
                        }
                    }
                    let id = self.id();
                    self.events.push(Event {
                        id,
                        title: if f.title.trim().is_empty() { "New event".into() } else { f.title.clone() },
                        start,
                        end: start + Duration::minutes(f.minutes),
                        calendar: f.calendar,
                        location: f.location.clone(),
                        link: (!f.link.trim().is_empty()).then(|| f.link.trim().to_string()),
                        attendees: f.attendees.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect(),
                        notes: String::new(),
                        notified: false,
                        all_day: false,
                    });
                    self.selected_event = Some(id);
                    self.cal_anchor = d;
                    if mac::has_helper() && self.screenshot.is_none() {
                        let cal = self.calendars.get(f.calendar).map(|c| c.name.clone()).unwrap_or_default();
                        let (title, loc, link, end) = (f.title.trim().to_string(), f.location.clone(), f.link.trim().to_string(), start + Duration::minutes(f.minutes));
                        std::thread::spawn(move || {
                            mac::add_event(&cal, if title.is_empty() { "New event" } else { &title }, start, end, &loc, &link);
                        });
                    }
                    self.toast("Event added");
                    return;
                }
                _ => self.toast("Check the date (YYYY-MM-DD) and time (HH:MM)"),
            }
        }
        if open {
            self.event_form = Some(f);
        }
    }

    fn account_window(&mut self, ctx: &egui::Context, pal: Pal) {
        let Some(mut f) = self.account_form.take() else { return };
        let mut open = true;
        let mut save = false;
        egui::Window::new("Add Mail Account").open(&mut open).collapsible(false).resizable(false).default_pos([420.0, 120.0]).show(ctx, |ui| {
            ui.label(RichText::new("Use an app password from your provider's security settings. It's kept in your Mac's Keychain.").size(12.0).color(pal.sub));
            ui.horizontal_wrapped(|ui| {
                for p in net::PROVIDERS.iter() {
                    if ui.button(p.name).clicked() {
                        f.imap_host = p.imap.0.into();
                        f.imap_port = p.imap.1.to_string();
                        f.smtp_host = p.smtp.0.into();
                        f.smtp_port = p.smtp.1.to_string();
                    }
                }
            });
            egui::Grid::new("acct-grid").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
                ui.label("Your name");
                ui.text_edit_singleline(&mut f.name);
                ui.end_row();
                ui.label("Email");
                if ui.text_edit_singleline(&mut f.email).changed() {
                    f.username = f.email.clone();
                }
                ui.end_row();
                ui.label("Incoming (IMAP)");
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut f.imap_host).desired_width(180.0));
                    ui.add(egui::TextEdit::singleline(&mut f.imap_port).desired_width(50.0));
                });
                ui.end_row();
                ui.label("Outgoing (SMTP)");
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut f.smtp_host).desired_width(180.0));
                    ui.add(egui::TextEdit::singleline(&mut f.smtp_port).desired_width(50.0));
                });
                ui.end_row();
                ui.label("Username");
                ui.text_edit_singleline(&mut f.username);
                ui.end_row();
                ui.label("App password");
                ui.add(egui::TextEdit::singleline(&mut f.password).password(true));
                ui.end_row();
            });
            if !f.error.is_empty() {
                ui.label(RichText::new(&f.error).color(RED));
            }
            if primary_button(ui, "Add and sync").clicked() {
                save = true;
            }
        });
        if save {
            f.email = f.email.trim().to_string();
            f.password = f.password.split_whitespace().collect();
            let imap_port = f.imap_port.trim().parse::<u16>();
            let smtp_port = f.smtp_port.trim().parse::<u16>();
            if !f.email.contains('@') || f.imap_host.is_empty() || f.smtp_host.is_empty() || f.password.is_empty() {
                f.error = "Fill in email, both servers and the app password.".into();
            } else if imap_port.is_err() || smtp_port.is_err() {
                f.error = "Ports must be numbers (usually 993 and 465 or 587).".into();
            } else {
                match mac::keychain_set(&f.email, &f.password) {
                    Ok(()) => {
                        let acc = Account {
                            name: if f.name.is_empty() { f.email.clone() } else { f.name.clone() },
                            email: f.email.trim().to_string(),
                            imap_host: f.imap_host.trim().to_string(),
                            imap_port: imap_port.unwrap(),
                            smtp_host: f.smtp_host.trim().to_string(),
                            smtp_port: smtp_port.unwrap(),
                            username: if f.username.trim().is_empty() { f.email.clone() } else { f.username.trim().to_string() },
                        };
                        self.settings.accounts.retain(|a| a.email != acc.email);
                        self.settings.accounts.push(acc.clone());
                        self.save_settings();
                        self.sync_account(&acc, ctx);
                        return;
                    }
                    Err(e) => f.error = format!("Couldn't save to Keychain: {}", e),
                }
            }
        }
        if open {
            self.account_form = Some(f);
        }
    }

    fn settings_window(&mut self, ctx: &egui::Context, pal: Pal) {
        if !self.settings_open {
            return;
        }
        let mut open = true;
        let mut changed = false;
        let mut theme_changed = false;
        let mut sync: Option<Account> = None;
        let mut refresh_cal = false;
        let mut remove: Option<String> = None;
        egui::Window::new("Settings").open(&mut open).collapsible(false).default_size([640.0, 470.0]).default_pos([320.0, 110.0]).show(ctx, |ui| {
            ui.horizontal(|ui| {
                for (l, t) in [
                    ("General", SettingsTab::General),
                    ("Accounts", SettingsTab::Accounts),
                    ("AI", SettingsTab::Ai),
                    ("Menu Bar", SettingsTab::MenuBar),
                    ("Integrations", SettingsTab::Integrations),
                    ("Templates", SettingsTab::Templates),
                    ("Plan", SettingsTab::Plan),
                ] {
                    ui.selectable_value(&mut self.settings_tab, t, l);
                }
            });
            ui.separator();
            egui::ScrollArea::vertical().id_salt("settings-scroll").max_height(400.0).show(ui, |ui| match self.settings_tab {
                SettingsTab::General => {
                    ui.spacing_mut().item_spacing.y = 9.0;
                    ui.visuals_mut().widgets.inactive.bg_stroke = Stroke::new(1.2_f32, pal.sub);
                    ui.visuals_mut().widgets.hovered.bg_stroke = Stroke::new(1.2_f32, ACCENT);
                    theme_changed |= ui.checkbox(&mut self.settings.dark, "Dark appearance").changed();
                    changed |= ui.checkbox(&mut self.settings.notifications, "Mac notifications (snoozes, follow-ups, meetings)").changed();
                    changed |= ui.checkbox(&mut self.settings.gatekeeper, "Gatekeeper: hold emails from new senders for approval").changed();
                    ui.horizontal(|ui| {
                        ui.label("Text size");
                        for (label, z) in [("Small", 0.9_f32), ("Normal", 1.0), ("Large", 1.15), ("Larger", 1.3)] {
                            if ui.selectable_label((self.settings.zoom - z).abs() < 0.01, label).clicked() {
                                self.settings.zoom = z;
                                ui.ctx().set_zoom_factor(z);
                                changed = true;
                            }
                        }
                    });
                    changed |= ui.checkbox(&mut self.settings.sync_read_to_gmail, "Mark emails read in Gmail when I open them").changed();
                    let mut login = mac::login_item_enabled();
                    if ui.checkbox(&mut login, "Open Flint when I log in (keeps Cmd+Shift+B ready)").changed() {
                        if let Err(e) = mac::set_login_item(login) {
                            self.toast(e);
                        }
                    }
                    changed |= ui.checkbox(&mut self.settings.close_to_tray, "Keep running in the menu bar when the window is closed (needed for Cmd+Shift+B)").changed();
                    ui.horizontal(|ui| {
                        ui.label("Undo send window");
                        for s in mech::UNDO_SEND_CHOICES {
                            changed |= ui.selectable_value(&mut self.settings.undo_send_secs, s, format!("{} s", s)).changed();
                        }
                    });
                    ui.add_space(6.0);
                    ui.label(RichText::new("Press ? anywhere for all keyboard shortcuts. Cmd+Shift+B snapshots your planner, Cmd+K opens commands.").size(12.0).color(pal.sub));
                    if !self.settings.blocked.is_empty() {
                        ui.add_space(6.0);
                        ui.label(RichText::new("Blocked senders").strong());
                        let mut unblock = None;
                        for b in &self.settings.blocked {
                            ui.horizontal(|ui| {
                                ui.label(b);
                                if ui.small_button("Unblock").clicked() {
                                    unblock = Some(b.clone());
                                }
                            });
                        }
                        if let Some(b) = unblock {
                            self.settings.blocked.retain(|x| *x != b);
                            changed = true;
                        }
                    }
                }
                SettingsTab::Accounts => {
                    ui.label(RichText::new("Sample accounts").strong());
                    for a in data::SAMPLE_ACCOUNTS {
                        ui.label(RichText::new(format!("{}  (sample mail)", a)).color(pal.sub));
                    }
                    ui.add_space(6.0);
                    ui.label(RichText::new("Your accounts").strong());
                    if self.settings.accounts.is_empty() {
                        ui.label(RichText::new("None yet. Add one to read and send real mail.").color(pal.sub));
                    }
                    for a in &self.settings.accounts {
                        ui.horizontal(|ui| {
                            ui.label(format!("{}  ({})", a.email, a.imap_host));
                            if self.syncing.contains(&a.email) {
                                ui.spinner();
                            } else if ui.small_button("Sync now").clicked() {
                                sync = Some(a.clone());
                            }
                            if ui.small_button("Remove").clicked() {
                                remove = Some(a.email.clone());
                            }
                        });
                    }
                    if primary_button(ui, "+ Add account").clicked() {
                        self.account_form = Some(AccountForm { imap_port: "993".into(), smtp_port: "465".into(), ..Default::default() });
                    }
                    ui.add_space(10.0);
                    ui.label(RichText::new("Calendars").strong());
                    ui.label(RichText::new("Flint reads your Mac's calendars and Reminders through Flint Calendar Helper (allow it in System Settings > Privacy & Security > Calendars and Reminders). To load Google Calendar directly instead, paste its secret iCal address below, one per line (Google Calendar on the web > Settings > your calendar > Integrate calendar > Secret address in iCal format). Kept in your Mac's Keychain.").size(12.0).color(pal.sub));
                    ui.add(egui::TextEdit::multiline(&mut self.ical_text).password(false).desired_rows(3).desired_width(f32::INFINITY).hint_text("https://calendar.google.com/calendar/ical/.../basic.ics"));
                    if ui.button("Save and refresh calendar").clicked() {
                        if self.ical_text.trim().is_empty() {
                            mac::keychain_delete("ical-urls");
                        } else {
                            let _ = mac::keychain_set("ical-urls", self.ical_text.trim());
                        }
                        refresh_cal = true;
                    }
                }
                SettingsTab::Ai => {
                    ui.label(RichText::new("Where AI runs").strong());
                    ui.label(RichText::new("By default Flint uses the local model qwen3.5:9b through Ollama on this Mac. It is free and nothing leaves your Mac. If Ollama isn't running, Flint falls back to simple on-device versions.").size(12.0).color(pal.sub));
                    ui.add_space(6.0);
                    ui.label(RichText::new("Anthropic API key (optional)").strong());
                    ui.label(RichText::new("Paste a key to use Claude instead, for better writing and summaries. This uses your API credit. The key is kept in your Mac's Keychain.").size(12.0).color(pal.sub));
                    ui.horizontal(|ui| {
                        ui.add(egui::TextEdit::singleline(&mut self.ai_key).password(true).desired_width(320.0).hint_text("sk-ant-..."));
                        if ui.button("Save key").clicked() {
                            if self.ai_key.trim().is_empty() {
                                mac::keychain_delete("ai-key");
                                self.ai_key_saved = false;
                            } else if mac::keychain_set("ai-key", self.ai_key.trim()).is_ok() {
                                self.ai_key_saved = true;
                            }
                        }
                    });
                    ui.label(RichText::new(if self.ai_key_saved { "Key saved" } else { "No key saved" }).size(11.5).color(if self.ai_key_saved { GREEN } else { pal.sub }));
                    ui.horizontal(|ui| {
                        ui.label("Claude model (used with a key)");
                        changed |= ui.text_edit_singleline(&mut self.settings.ai_model).changed();
                    });
                    changed |= ui.checkbox(&mut self.settings.ai_auto_summary, "Summarize each email when I open it").changed();
                    if ui.checkbox(&mut self.settings.ai_auto_labels, "Auto-label incoming emails (Work, Finance, Travel...)").changed() {
                        changed = true;
                        if self.settings.ai_auto_labels {
                            self.auto_label_all();
                        }
                    }
                }
                SettingsTab::MenuBar => {
                    ui.label(RichText::new("The menu bar item shows your unread count and a countdown to your next meeting, with today's agenda and quick actions in its menu.").size(12.0).color(pal.sub));
                    changed |= ui.checkbox(&mut self.settings.menu_bar, "Show in the menu bar (takes effect on next launch)").changed();
                    changed |= ui.checkbox(&mut self.settings.menu_bar_unread, "Show unread count").changed();
                    changed |= ui.checkbox(&mut self.settings.menu_bar_countdown, "Show next meeting countdown (within the hour)").changed();
                }
                SettingsTab::Integrations => {
                    ui.label(RichText::new("Send any email to these apps from the 'Send to...' button.").size(12.0).color(pal.sub));
                    egui::Grid::new("integrations").num_columns(2).striped(true).show(ui, |ui| {
                        for i in Integration::ALL {
                            ui.label(RichText::new(i.name()).strong());
                            ui.label(RichText::new(i.how()).size(12.0).color(if i.native() { GREEN } else { pal.sub }));
                            ui.end_row();
                        }
                    });
                }
                SettingsTab::Templates => {
                    ui.label(RichText::new("Signature").strong());
                    changed |= ui.add(egui::TextEdit::multiline(&mut self.settings.signature).desired_rows(3).desired_width(f32::INFINITY)).changed();
                    ui.add_space(6.0);
                    ui.label(RichText::new("Templates  ({name} becomes the recipient's first name)").strong());
                    let mut del = None;
                    for (i, t) in self.settings.templates.iter_mut().enumerate() {
                        ui.horizontal(|ui| {
                            changed |= ui.text_edit_singleline(&mut t.name).changed();
                            if ui.small_button("Delete").clicked() {
                                del = Some(i);
                            }
                        });
                        changed |= ui.add(egui::TextEdit::multiline(&mut t.body).desired_rows(3).desired_width(f32::INFINITY)).changed();
                    }
                    if let Some(i) = del {
                        self.settings.templates.remove(i);
                        changed = true;
                    }
                    if ui.button("+ New template").clicked() {
                        self.settings.templates.push(data::Template { name: "New template".into(), subject: String::new(), body: String::new() });
                        changed = true;
                    }
                }
                SettingsTab::Plan => {
                    ui.horizontal(|ui| {
                        badge(ui, "PRO");
                        ui.label(RichText::new("Every feature is included. No subscription, no AI limits.").strong());
                    });
                    ui.add_space(6.0);
                    for f in [
                        "AI writing: write, reply, rephrase, shorten, expand, proofread, tone, translate",
                        "AI summaries and Ask AI about your inbox",
                        "Auto-labels and smart quick replies",
                        "Gatekeeper for new senders, block and unsubscribe",
                        "Read receipts and follow-up reminders",
                        "Send later, snooze, set aside, priority and pins",
                        "Templates and signatures",
                        "Team sharing, private comments and assignments",
                        "Share availability from your calendar",
                        "Integrations with Reminders, Notes, Calendar, Things, OmniFocus, Todoist, Bear, Agenda and more",
                        "Menu bar agenda, meeting countdown and Mac notifications",
                        "Meeting notes with AI summaries",
                    ] {
                        ui.label(format!("-  {}", f));
                    }
                }
            });
        });
        if let Some(a) = sync {
            self.sync_account(&a, ctx);
        }
        if refresh_cal {
            self.sync_calendar(ctx);
        }
        if let Some(e) = remove {
            self.settings.accounts.retain(|a| a.email != e);
            self.mails.retain(|m| m.account != e);
            mac::keychain_delete(&e);
            changed = true;
        }
        if theme_changed {
            theme::apply(ctx, self.settings.dark);
            changed = true;
        }
        if changed {
            self.save_settings();
        }
        self.settings_open = open;
    }

    fn command_bar(&mut self, ctx: &egui::Context, pal: Pal) {
        if !self.cmd_open {
            return;
        }
        let mut commands: Vec<(String, u8)> = vec![
            ("New email".into(), 0),
            ("Ask AI about my inbox".into(), 1),
            ("Open calendar".into(), 2),
            ("New calendar event".into(), 3),
            ("Go to Smart Inbox".into(), 4),
            ("Go to New Senders (Gatekeeper)".into(), 5),
            ("Go to Send Later".into(), 6),
            ("Go to Follow-ups".into(), 7),
            ("Mark selected as done".into(), 8),
            ("Snooze selected until tomorrow".into(), 9),
            ("Summarize selected email".into(), 10),
            ("Reply to selected email".into(), 11),
            ("Auto-label all emails".into(), 12),
            ("Toggle dark appearance".into(), 13),
            ("Open settings".into(), 14),
            ("Sync all accounts".into(), 15),
            ("Mark all notifications as read".into(), 16),
            ("Go to Snoozed".into(), 17),
            ("Snapshot my planner (Cmd+Shift+B)".into(), 18),
            ("Keyboard shortcuts (?)".into(), 19),
            ("Brief me on my day".into(), 20),
        ];
        let q = self.cmd_query.to_lowercase();
        commands.retain(|(l, _)| q.split_whitespace().all(|w| l.to_lowercase().contains(w)));
        let mut run: Option<u8> = None;
        egui::Area::new(egui::Id::new("cmd-backdrop")).order(egui::Order::Foreground).fixed_pos(egui::pos2(0.0, 0.0)).show(ctx, |ui| {
            let screen = ctx.screen_rect();
            let r = ui.allocate_rect(screen, Sense::click());
            ui.painter().rect_filled(screen, Rounding::ZERO, Color32::from_black_alpha(70));
            if r.clicked() {
                self.cmd_open = false;
            }
        });
        egui::Window::new("command-bar")
            .title_bar(false)
            .resizable(false)
            .anchor(Align2::CENTER_TOP, egui::vec2(0.0, 110.0))
            .fixed_size([520.0, 360.0])
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                let r = ui.add(egui::TextEdit::singleline(&mut self.cmd_query).hint_text("Type a command...").desired_width(f32::INFINITY).font(FontId::proportional(16.0)));
                r.request_focus();
                if ui.input(|i| i.key_pressed(Key::Enter)) {
                    run = commands.first().map(|c| c.1);
                }
                ui.separator();
                egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
                    for (i, (label, id)) in commands.iter().enumerate() {
                        let text = RichText::new(label).size(13.5).color(if i == 0 { ACCENT } else { pal.text });
                        if ui.add_sized([ui.available_width(), 26.0], egui::Button::new(text).frame(i == 0).fill(pal.sel)).clicked() {
                            run = Some(*id);
                        }
                    }
                    if commands.is_empty() {
                        ui.label(RichText::new("No matching command").color(pal.sub));
                    }
                });
            });
        if let Some(id) = run {
            self.cmd_open = false;
            let sel = self.selected;
            let mut acts = Vec::new();
            match id {
                0 => self.composer = Some(self.fresh_composer()),
                1 => {
                    self.mode = Mode::Mail;
                    self.copilot_open = true;
                }
                2 => self.mode = Mode::Calendar,
                3 => {
                    self.mode = Mode::Calendar;
                    self.event_form = Some(self.new_event_form(None));
                }
                4 => self.folder = Folder::SmartInbox,
                5 => self.folder = Folder::Gatekeeper,
                6 => self.folder = Folder::Scheduled,
                7 => self.folder = Folder::FollowUps,
                8 => acts.extend(sel.map(Act::Done)),
                9 => acts.extend(sel.map(|s| Act::Snooze(s, Preset::Tomorrow))),
                10 => acts.extend(sel.map(Act::Summarize)),
                11 => acts.extend(sel.map(|s| Act::Reply(s, false))),
                18 => self.start_snapshot(ctx),
                19 => self.help_open = true,
                20 => self.brief_me(ctx),
                12 => {
                    self.auto_label_all();
                    self.toast("Labels applied");
                }
                13 => {
                    self.settings.dark = !self.settings.dark;
                    theme::apply(ctx, self.settings.dark);
                    self.save_settings();
                }
                14 => self.settings_open = true,
                15 => {
                    for a in self.settings.accounts.clone() {
                        self.sync_account(&a, ctx);
                    }
                    if self.settings.accounts.is_empty() {
                        self.toast("No real accounts yet - add one in Settings");
                    }
                }
                16 => acts.push(Act::MarkSectionRead(Section::Notifications)),
                17 => self.folder = Folder::Snoozed,
                _ => {}
            }
            if matches!(id, 4..=7 | 17) {
                self.mode = Mode::Mail;
                self.search.clear();
            }
            self.apply(acts, ctx);
        }
    }

    fn overlays(&mut self, ctx: &egui::Context, pal: Pal) {
        let now = Local::now();
        let mut undo = false;
        let has_pending = self.pending.is_some();
        if has_pending || !self.toasts.is_empty() {
            egui::Area::new(egui::Id::new("toasts")).anchor(Align2::CENTER_BOTTOM, egui::vec2(0.0, -24.0)).order(egui::Order::Foreground).show(ctx, |ui| {
                if let Some(p) = &self.pending {
                    let left = (p.deadline - now).num_seconds().max(0);
                    egui::Frame::none().fill(Color32::from_rgb(0x28, 0x2a, 0x2e)).rounding(Rounding::same(8.0)).inner_margin(egui::Margin::symmetric(14.0, 8.0)).show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(format!("Sending to {} in {}s", p.item.to, left)).color(Color32::WHITE));
                            if ui.add(egui::Button::new(RichText::new("Undo").color(Color32::from_rgb(0x81, 0xbe, 0xff)).strong()).frame(false)).clicked() {
                                undo = true;
                            }
                        });
                    });
                    ui.add_space(6.0);
                }
                for t in self.toasts.iter().rev().take(3) {
                    egui::Frame::none().fill(Color32::from_rgb(0x28, 0x2a, 0x2e)).rounding(Rounding::same(8.0)).inner_margin(egui::Margin::symmetric(14.0, 8.0)).show(ui, |ui| {
                        ui.label(RichText::new(&t.text).color(Color32::WHITE));
                    });
                    ui.add_space(4.0);
                }
            });
        }
        if undo {
            self.undo_send();
        }
        let _ = pal;
    }

    fn brief_me(&mut self, ctx: &egui::Context) {
        let now = Local::now();
        let today = now.date_naive();
        self.mode = Mode::Mail;
        self.copilot_open = true;
        self.copilot_log.push((true, "Brief me on my day".to_string()));
        self.copilot_busy = true;
        let events: Vec<String> = self
            .events
            .iter()
            .filter(|e| e.start.date_naive() == today && e.end > now)
            .map(|e| format!("{} {}", e.start.format("%-I:%M %p"), e.title))
            .collect();
        let tasks: Vec<String> = self
            .tasks
            .iter()
            .filter(|t| t.due.map(|d| d.date_naive() <= today).unwrap_or(false))
            .map(|t| t.title.clone())
            .collect();
        let list: Vec<&Mail> = self.mails.iter().filter(|m| !m.trashed && !m.done && !m.read).take(15).collect();
        let fallback = format!(
            "{} unread email{}, {} event{} left today, {} task{} due.",
            list.len(),
            if list.len() == 1 { "" } else { "s" },
            events.len(),
            if events.len() == 1 { "" } else { "s" },
            tasks.len(),
            if tasks.len() == 1 { "" } else { "s" }
        );
        let prompt = format!(
            "Today's remaining events:\n{}\n\nTasks due:\n{}\n\nUnread emails:\n\n{}\n\nGive me a short morning briefing: what needs a reply first, what is on my schedule, and what is due.",
            if events.is_empty() { "none".to_string() } else { events.join("\n") },
            if tasks.is_empty() { "none".to_string() } else { tasks.join("\n") },
            ai::mail_context(&list)
        );
        self.run_ai(ctx, "You are a concise personal assistant. Brief the user in at most 6 short lines using only the data provided.", prompt, fallback, Target::Copilot);
    }

    fn fresh_composer(&mut self) -> Composer {
        match self.draft.take() {
            Some(d) => {
                self.toast("Draft restored");
                d
            }
            None => Composer::blank(),
        }
    }

    fn help_window(&mut self, ctx: &egui::Context, pal: Pal) {
        if !self.help_open {
            return;
        }
        let mut open = true;
        let rows: [(&str, &str); 19] = [
            ("Cmd+Shift+B", "Snapshot your planner (works from any app)"),
            ("Cmd+K", "Command bar"),
            ("j / k", "Next / previous email"),
            ("e", "Mark done"),
            ("p", "Pin"),
            ("r", "Reply"),
            ("c", "Compose"),
            ("h", "Snooze until tomorrow"),
            ("u", "Toggle read / unread"),
            ("Delete", "Move to trash"),
            ("Cmd+Z", "Undo the last done or delete"),
            ("/", "Search"),
            ("Search filters", "from: to: subject: label: is:unread/read/pinned/priority/done has:attachment/comment, - to exclude"),
            ("?", "This list"),
            ("Cmd+Enter", "Send the email you are writing"),
            ("Esc", "Close dialogs"),
            ("Double-click a time", "New calendar event"),
            ("t / arrows", "Calendar: today / previous or next period"),
            ("m", "Calendar: back to mail"),
        ];
        egui::Window::new("Keyboard shortcuts").open(&mut open).collapsible(false).resizable(false).anchor(Align2::CENTER_CENTER, egui::vec2(0.0, 0.0)).show(ctx, |ui| {
            egui::Grid::new("shortcut-grid").num_columns(2).spacing([24.0, 8.0]).show(ui, |ui| {
                for (k, d) in rows {
                    ui.label(RichText::new(k).strong().color(ACCENT));
                    ui.label(RichText::new(d).color(pal.text));
                    ui.end_row();
                }
            });
        });
        if !open {
            self.help_open = false;
        }
    }

    fn poll_hotkey(&mut self, ctx: &egui::Context) {
        if !self.cal_syncing && !self.tasks_syncing && std::fs::remove_file("/tmp/flint-snap").is_ok() {
            self.start_snapshot(ctx);
        }
        #[cfg(target_os = "macos")]
        {
            let fired = self.hotkey.as_ref().map(|h| h.fired.swap(false, std::sync::atomic::Ordering::SeqCst)).unwrap_or(false);
            if fired {
                self.start_snapshot(ctx);
            }
        }
        let _ = ctx;
    }

    fn start_snapshot(&mut self, ctx: &egui::Context) {
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        self.hidden = false;
        if self.snap_prev_view.is_none() {
            self.snap_prev_view = Some((self.mode, self.cal_view));
        }
        self.mode = Mode::Calendar;
        self.cal_view = CalView::Week;
        self.cal_anchor = Local::now().date_naive();
        ctx.data_mut(|d| d.insert_temp(egui::Id::new("cal-scrolled"), false));
        self.cmd_open = false;
        self.snap_open = false;
        self.selected_event = None;
        self.toasts.clear();
        self.composer = None;
        self.settings_open = false;
        self.snap_stage = 1;
        self.snap_wait = 0;
        self.snap_started = Some(std::time::Instant::now());
        self.sync_calendar(ctx);
        ctx.request_repaint();
    }

    fn handle_snapshot(&mut self, ctx: &egui::Context, pal: Pal) {
        #[cfg(target_os = "macos")]
        {
            match self.snap_stage {
                1 => {
                    self.snap_wait += 1;
                    ctx.request_repaint();
                    let syncing = self.cal_syncing || self.tasks_syncing;
                    let waited = self.snap_started.map(|t| t.elapsed().as_secs() >= 10).unwrap_or(true);
                    if self.snap_wait >= 8 && (!syncing || waited) {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot);
                        self.snap_stage = 2;
                    }
                }
                2 => {
                    let image = ctx.input(|i| {
                        i.raw.events.iter().find_map(|e| match e {
                            egui::Event::Screenshot { image, .. } => Some(image.clone()),
                            _ => None,
                        })
                    });
                    if let Some(img) = image {
                        self.snap_stage = 0;
                        if let Some((_, v)) = self.snap_prev_view.take() {
                            self.cal_view = v;
                        }
                        let img = std::sync::Arc::new(crate::snap::crop_left(&img, (232.0 * ctx.pixels_per_point()) as usize));
                        match crate::snap::save(&img) {
                            Ok(path) => {
                                crate::snap::copy_to_clipboard(&path);
                                self.snap_tex = Some(ctx.load_texture("planner-snap", (*img).clone(), egui::TextureOptions::LINEAR));
                                self.snap_path = Some(path);
                                self.snap_open = true;
                                self.toast("Planner snapshot saved and copied".to_string());
                            }
                            Err(e) => self.toast(format!("Snapshot failed: {}", e)),
                        }
                    } else {
                        ctx.request_repaint();
                    }
                }
                _ => {}
            }
        }
        if self.snap_open {
            let mut open = true;
            let mut attach = false;
            egui::Window::new("Planner snapshot").open(&mut open).collapsible(false).default_size([760.0, 520.0]).show(ctx, |ui| {
                if let Some(tex) = &self.snap_tex {
                    let avail = ui.available_width().min(760.0);
                    let ratio = tex.size_vec2().y / tex.size_vec2().x;
                    ui.add(egui::Image::new((tex.id(), egui::vec2(avail, avail * ratio))).rounding(6.0));
                }
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if let Some(p) = self.snap_path.clone() {
                        ui.label(RichText::new(p.display().to_string()).size(11.0).color(pal.sub));
                        if ui.button("Copy").clicked() {
                            #[cfg(target_os = "macos")]
                            crate::snap::copy_to_clipboard(&p);
                        }
                        if ui.button("Show in Finder").clicked() {
                            #[cfg(target_os = "macos")]
                            crate::snap::reveal(&p);
                        }
                        if ui.button("Email it").clicked() {
                            attach = true;
                        }
                    }
                });
            });
            if attach {
                let mut c = Composer::blank();
                c.subject = "My planner".into();
                c.body = "Here is my planner.\n".into();
                if let Some(p) = &self.snap_path {
                    c.attachments.push(p.clone());
                }
                self.composer = Some(c);
                open = false;
            }
            if !open {
                self.snap_open = false;
            }
        }
    }

    fn handle_screenshot(&mut self, ctx: &egui::Context) {
        let Some(path) = self.screenshot.clone() else { return };
        ctx.request_repaint();
        if self.frame >= 120 && !self.shot_requested {
            self.shot_requested = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot);
        }
        let image = ctx.input(|i| {
            i.raw.events.iter().find_map(|e| match e {
                egui::Event::Screenshot { image, .. } => Some(image.clone()),
                _ => None,
            })
        });
        if let Some(img) = image {
            let [w, h] = img.size;
            let mut bytes = Vec::with_capacity(w * h * 4);
            for p in &img.pixels {
                bytes.extend_from_slice(&[p.r(), p.g(), p.b(), 255]);
            }
            if let Some(dir) = path.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            if let Err(e) = image::save_buffer(&path, &bytes, w as u32, h as u32, image::ColorType::Rgba8) {
                eprintln!("screenshot failed: {}", e);
            }
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        if self.frame > 600 {
            eprintln!("screenshot timed out");
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
}

impl eframe::App for FlintApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.frame += 1;
        let pal = Pal::new(self.settings.dark);
        self.poll_background();
        self.handle_tray(ctx);
        if self.refresh_planner {
            self.refresh_planner = false;
            self.sync_calendar(ctx);
        }
        if self.last_mail_sync.elapsed().as_secs() > 120 && self.syncing.is_empty() && self.screenshot.is_none() {
            self.last_mail_sync = std::time::Instant::now();
            for acc in self.settings.accounts.clone() {
                self.sync_account(&acc, ctx);
            }
        }
        let cal_interval = if self.cal_warning.is_empty() { 300 } else { 1800 };
        if self.last_cal_sync.elapsed().as_secs() > cal_interval && !self.cal_syncing {
            self.sync_calendar(ctx);
        }
        let unread_now = self.mails.iter().filter(|m| !m.read && !m.trashed && !m.done).count();
        if unread_now != self.last_title_unread && self.screenshot.is_none() {
            self.last_title_unread = unread_now;
            let title = if unread_now > 0 { format!("Flint Mail ({})", unread_now) } else { "Flint Mail".to_string() };
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title));
        }
        let cv = match self.cal_view {
            CalView::Day => "day",
            CalView::ThreeDays => "3days",
            CalView::Week => "week",
            CalView::Month => "month",
        };
        if self.settings.cal_view != cv {
            self.settings.cal_view = cv.to_string();
            self.save_settings();
        }
        self.poll_hotkey(ctx);
        self.tick(ctx);
        self.shortcuts(ctx);

        if ctx.input(|i| i.viewport().close_requested()) && self.settings.close_to_tray && !self.quitting && self.screenshot.is_none() {
            #[cfg(target_os = "macos")]
            if self.menubar.is_some() {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
                self.hidden = true;
            }
        }

        egui::SidePanel::left("sidebar")
            .exact_width(232.0)
            .resizable(false)
            .frame(egui::Frame::none().fill(theme::SIDEBAR_BG).stroke(Stroke::new(1.0_f32, theme::SIDEBAR_LINE)))
            .show(ctx, |ui| self.sidebar(ui));

        egui::TopBottomPanel::top("toolbar")
            .exact_height(50.0)
            .frame(egui::Frame::none().fill(pal.bg).inner_margin(egui::Margin::symmetric(10.0, 6.0)).stroke(Stroke::new(1.0_f32, pal.border)))
            .show(ctx, |ui| self.toolbar(ui, pal));

        if self.copilot_open && self.mode == Mode::Mail {
            egui::SidePanel::right("copilot")
                .default_width(320.0)
                .frame(egui::Frame::none().fill(pal.bg).inner_margin(egui::Margin::same(10.0)).stroke(Stroke::new(1.0_f32, pal.border)))
                .show(ctx, |ui| self.copilot(ui, pal));
        }

        match self.mode {
            Mode::Mail => {
                egui::SidePanel::left("list")
                    .default_width(390.0)
                    .width_range(300.0..=560.0)
                    .frame(egui::Frame::none().fill(pal.bg).stroke(Stroke::new(1.0_f32, pal.border)))
                    .show(ctx, |ui| self.list_panel(ui, pal));
                egui::CentralPanel::default().frame(egui::Frame::none().fill(pal.bg)).show(ctx, |ui| self.reader(ui, pal));
            }
            Mode::Calendar => {
                egui::SidePanel::right("event")
                    .default_width(300.0)
                    .frame(egui::Frame::none().fill(pal.bg).inner_margin(egui::Margin::same(12.0)).stroke(Stroke::new(1.0_f32, pal.border)))
                    .show(ctx, |ui| {
                        egui::ScrollArea::vertical().id_salt("event-scroll").show(ui, |ui| self.event_details(ui, pal));
                    });
                egui::CentralPanel::default().frame(egui::Frame::none().fill(pal.bg).inner_margin(egui::Margin::symmetric(6.0, 0.0))).show(ctx, |ui| self.calendar(ui, pal));
            }
        }

        self.composer_window(ctx, pal);
        self.event_window(ctx, pal);
        self.account_window(ctx, pal);
        self.settings_window(ctx, pal);
        self.command_bar(ctx, pal);
        self.overlays(ctx, pal);
        self.handle_screenshot(ctx);
        self.handle_snapshot(ctx, pal);
        self.help_window(ctx, pal);

        let wait = if self.pending.is_some() || !self.toasts.is_empty() { 0.25 } else { 1.0 };
        ctx.request_repaint_after(std::time::Duration::from_secs_f32(wait));
    }
}

#[cfg(test)]
mod quick_task_tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn parses_day_and_time() {
        let now = Local.with_ymd_and_hms(2026, 11, 5, 10, 0, 0).unwrap();
        let (t, d) = parse_quick_task("call Mom tomorrow at 3pm", now);
        assert_eq!(t, "call Mom");
        let d = d.unwrap();
        assert_eq!((d.day(), d.hour(), d.minute()), (6, 15, 0));
        let (t, d) = parse_quick_task("buy milk", now);
        assert_eq!(t, "buy milk");
        assert!(d.is_none());
        let (t, d) = parse_quick_task("review plan friday", now);
        assert_eq!(t, "review plan");
        assert_eq!(d.unwrap().day(), 6);
        let (t, d) = parse_quick_task("tomorrow", now);
        assert_eq!(t, "tomorrow");
        assert!(d.is_none());
    }
}
