// Menu bar content as plain data. The macOS shim in tray.rs only renders this.
use chrono::{DateTime, Local, Timelike};

#[derive(Clone, Debug, PartialEq)]
pub enum Do {
    OpenApp,
    NewEmail,
    Calendar,
    Capture,
    AskAi,
    Settings,
    Quit,
    SyncNow,
    QuickTask,
    QuickEvent,
    ClipTask,
    ClipEvent,
    ClipSummary,
    Quiet(i64),
    Card(String),
    Join(u64),
    Mail(u64),
    TaskDone(String),
    Event(u64),
}

impl Do {
    pub fn needs_window(&self) -> bool {
        !matches!(self, Do::Quit | Do::SyncNow | Do::QuickTask | Do::QuickEvent | Do::ClipTask | Do::ClipEvent | Do::ClipSummary | Do::Quiet(_) | Do::TaskDone(_))
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Row {
    Item { label: String, enabled: bool, act: Option<Do>, checked: Option<bool> },
    Sub { label: String, rows: Vec<Row> },
    Sep,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum TitleStyle {
    #[default]
    Smart,
    Unread,
    NextEvent,
    NeedsYou,
    Minimal,
}

impl TitleStyle {
    pub fn from_key(k: &str) -> Self {
        match k {
            "unread" => Self::Unread,
            "event" => Self::NextEvent,
            "needs" => Self::NeedsYou,
            "minimal" => Self::Minimal,
            _ => Self::Smart,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Icon {
    Plain,
    Unread,
    Meeting,
    Overdue,
}

pub struct EventRow {
    pub id: u64,
    pub title: String,
    pub start: DateTime<Local>,
    pub end: DateTime<Local>,
    pub has_link: bool,
}

pub struct TaskRow {
    pub key: String,
    pub title: String,
    pub overdue: bool,
}

pub struct MailRow {
    pub id: u64,
    pub from: String,
    pub subject: String,
}

pub struct NeedRow {
    pub id: String,
    pub title: String,
}

pub struct Sections {
    pub needs: bool,
    pub today: bool,
    pub unread: bool,
    pub quick: bool,
    pub clipboard: bool,
}

impl Default for Sections {
    fn default() -> Self {
        Sections { needs: true, today: true, unread: true, quick: true, clipboard: true }
    }
}

pub struct Inputs {
    pub now: DateTime<Local>,
    pub unread: usize,
    pub needs: Vec<NeedRow>,
    pub events: Vec<EventRow>,
    pub tasks: Vec<TaskRow>,
    pub tasks_total: usize,
    pub top_unread: Vec<MailRow>,
    pub quiet_until: Option<DateTime<Local>>,
    pub sync_error: String,
    pub last_sync: Option<DateTime<Local>>,
    pub syncing: bool,
    pub style: TitleStyle,
    pub sections: Sections,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Model {
    pub title: String,
    pub icon: Icon,
    pub rows: Vec<Row>,
}

pub fn clip(s: &str, n: usize) -> String {
    let s = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if s.chars().count() <= n {
        s
    } else {
        format!("{}...", s.chars().take(n.saturating_sub(3)).collect::<String>().trim_end())
    }
}

pub fn countdown(start: DateTime<Local>, now: DateTime<Local>) -> String {
    let m = (start - now).num_minutes();
    if m <= 0 {
        "now".into()
    } else if m < 60 {
        format!("in {}m", m)
    } else {
        format!("in {}h {}m", m / 60, m % 60)
    }
}

fn clock(t: DateTime<Local>) -> String {
    let h = t.hour() % 12;
    format!("{}:{:02} {}", if h == 0 { 12 } else { h }, t.minute(), if t.hour() < 12 { "AM" } else { "PM" })
}

fn item(label: impl Into<String>, act: Do) -> Row {
    Row::Item { label: label.into(), enabled: true, act: Some(act), checked: None }
}

fn info(label: impl Into<String>) -> Row {
    Row::Item { label: label.into(), enabled: false, act: None, checked: None }
}

fn join_target(inp: &Inputs) -> Option<&EventRow> {
    inp.events.iter().find(|e| e.has_link && e.end > inp.now && (e.start - inp.now).num_minutes() <= 10)
}

pub fn title_for(inp: &Inputs) -> String {
    let next = inp.events.iter().find(|e| e.end > inp.now);
    let event_title = |e: &EventRow| -> Option<String> {
        let mins = (e.start - inp.now).num_minutes();
        (mins < 60).then(|| format!("{} {}", clip(&e.title, 16), if mins <= 0 { "now".to_string() } else { countdown(e.start, inp.now) }))
    };
    let needs = || (!inp.needs.is_empty()).then(|| format!("{} for you", inp.needs.len()));
    let unread = || (inp.unread > 0).then(|| inp.unread.to_string());
    match inp.style {
        TitleStyle::Minimal => String::new(),
        TitleStyle::Unread => unread().unwrap_or_default(),
        TitleStyle::NextEvent => next.and_then(event_title).unwrap_or_default(),
        TitleStyle::NeedsYou => needs().unwrap_or_default(),
        TitleStyle::Smart => next.and_then(event_title).or_else(needs).or_else(unread).unwrap_or_default(),
    }
}

pub fn build(inp: &Inputs) -> Model {
    let now = inp.now;
    let mut rows: Vec<Row> = Vec::new();

    let status = if inp.needs.is_empty() && inp.unread == 0 { "All clear".to_string() } else { format!("{} for you  -  {} unread", inp.needs.len(), inp.unread) };
    rows.push(info(status));

    if let Some(e) = join_target(inp) {
        let live = e.start <= now;
        rows.push(item(format!("Join: {}  ({})", clip(&e.title, 28), if live { "started".to_string() } else { countdown(e.start, now) }), Do::Join(e.id)));
    }

    if inp.sections.needs && !inp.needs.is_empty() {
        rows.push(Row::Sep);
        rows.push(info("Needs you"));
        for n in inp.needs.iter().take(3) {
            rows.push(item(clip(&n.title, 44), Do::Card(n.id.clone())));
        }
    }

    if inp.sections.today && (!inp.events.is_empty() || !inp.tasks.is_empty()) {
        rows.push(Row::Sep);
        rows.push(info("Today"));
        for e in inp.events.iter().take(3) {
            rows.push(item(format!("{}  {}  ({})", clock(e.start), clip(&e.title, 28), countdown(e.start, now)), Do::Event(e.id)));
        }
        for t in inp.tasks.iter().take(5) {
            rows.push(Row::Item {
                label: format!("{}{}", clip(&t.title, 40), if t.overdue { "  (overdue)" } else { "" }),
                enabled: true,
                act: Some(Do::TaskDone(t.key.clone())),
                checked: Some(false),
            });
        }
        if inp.tasks_total > 5 {
            rows.push(info(format!("+ {} more tasks", inp.tasks_total - 5)));
        }
    }

    if inp.sections.unread && !inp.top_unread.is_empty() {
        rows.push(Row::Sep);
        rows.push(info("Unread"));
        for m in inp.top_unread.iter().take(3) {
            rows.push(item(format!("{}  -  {}", clip(&m.from, 16), clip(&m.subject, 30)), Do::Mail(m.id)));
        }
    }

    if inp.sections.quick {
        rows.push(Row::Sep);
        rows.push(item("Quick Task...", Do::QuickTask));
        rows.push(item("Quick Event...", Do::QuickEvent));
        rows.push(item("New Email", Do::NewEmail));
        if inp.sections.clipboard {
            rows.push(Row::Sub {
                label: "From Clipboard".into(),
                rows: vec![item("Make a task", Do::ClipTask), item("Make an event", Do::ClipEvent), item("Summarize", Do::ClipSummary)],
            });
        }
    }

    rows.push(Row::Sep);
    rows.push(item("Open Flint Mail", Do::OpenApp));
    rows.push(item("Open Calendar", Do::Calendar));
    rows.push(item("Capture Screen to Tasks", Do::Capture));
    rows.push(item("Ask AI...", Do::AskAi));

    rows.push(Row::Sep);
    match inp.quiet_until {
        Some(t) if t > now => {
            rows.push(Row::Item { label: format!("Notifications paused until {}", clock(t)), enabled: false, act: None, checked: None });
            rows.push(item("Resume notifications", Do::Quiet(0)));
        }
        _ => rows.push(Row::Sub { label: "Pause Notifications".into(), rows: vec![item("For 1 hour", Do::Quiet(60)), item("For 4 hours", Do::Quiet(240)), item("Until tomorrow", Do::Quiet(-1))] }),
    }
    if !inp.sync_error.is_empty() {
        rows.push(info(format!("Sync problem: {}", clip(&inp.sync_error, 48))));
    }
    let sync_label = if inp.syncing {
        "Syncing...".to_string()
    } else {
        match inp.last_sync {
            Some(t) => format!("Sync now  (last {})", clock(t)),
            None => "Sync now".to_string(),
        }
    };
    rows.push(Row::Item { label: sync_label, enabled: !inp.syncing, act: Some(Do::SyncNow), checked: None });
    rows.push(item("Settings...", Do::Settings));
    rows.push(Row::Sep);
    rows.push(item("Quit Flint Mail", Do::Quit));

    let icon = if inp.events.iter().any(|e| e.start <= now && e.end > now) {
        Icon::Meeting
    } else if inp.tasks.iter().any(|t| t.overdue) {
        Icon::Overdue
    } else if inp.unread > 0 {
        Icon::Unread
    } else {
        Icon::Plain
    };
    Model { title: title_for(inp), icon, rows }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, TimeZone};

    fn now() -> DateTime<Local> {
        Local.with_ymd_and_hms(2026, 11, 6, 9, 0, 0).unwrap()
    }

    fn base() -> Inputs {
        Inputs { now: now(), unread: 0, needs: vec![], events: vec![], tasks: vec![], tasks_total: 0, top_unread: vec![], quiet_until: None, sync_error: String::new(), last_sync: None, syncing: false, style: TitleStyle::Smart, sections: Sections::default() }
    }

    fn labels(m: &Model) -> Vec<String> {
        m.rows.iter().filter_map(|r| if let Row::Item { label, .. } = r { Some(label.clone()) } else { None }).collect()
    }

    #[test]
    fn empty_state_is_calm() {
        let m = build(&base());
        assert_eq!(m.title, "");
        assert_eq!(m.icon, Icon::Plain);
        assert_eq!(labels(&m)[0], "All clear");
        assert!(!labels(&m).iter().any(|l| l == "Needs you" || l == "Today"));
    }

    #[test]
    fn join_row_appears_only_with_a_link_inside_ten_minutes() {
        let mut inp = base();
        inp.events = vec![EventRow { id: 1, title: "Standup".into(), start: now() + Duration::minutes(8), end: now() + Duration::minutes(38), has_link: true }];
        assert!(labels(&build(&inp)).iter().any(|l| l.starts_with("Join: Standup")));
        inp.events[0].has_link = false;
        assert!(!labels(&build(&inp)).iter().any(|l| l.starts_with("Join")));
        inp.events[0].has_link = true;
        inp.events[0].start = now() + Duration::minutes(30);
        assert!(!labels(&build(&inp)).iter().any(|l| l.starts_with("Join")));
    }

    #[test]
    fn smart_title_prefers_soon_event_then_needs_then_unread() {
        let mut inp = base();
        inp.unread = 4;
        assert_eq!(title_for(&inp), "4");
        inp.needs = vec![NeedRow { id: "a".into(), title: "x".into() }];
        assert_eq!(title_for(&inp), "1 for you");
        inp.events = vec![EventRow { id: 1, title: "A very long meeting title".into(), start: now() + Duration::minutes(12), end: now() + Duration::minutes(60), has_link: false }];
        assert_eq!(title_for(&inp), "A very long m... in 12m");
        inp.style = TitleStyle::Minimal;
        assert_eq!(title_for(&inp), "");
    }

    #[test]
    fn tasks_are_checkable_overdue_flagged_and_capped() {
        let mut inp = base();
        inp.tasks = (0..5).map(|i| TaskRow { key: format!("t{}", i), title: format!("Task {}", i), overdue: i == 0 }).collect();
        inp.tasks_total = 8;
        let m = build(&inp);
        assert_eq!(m.icon, Icon::Overdue);
        let l = labels(&m);
        assert!(l.contains(&"Task 0  (overdue)".to_string()));
        assert!(l.contains(&"+ 3 more tasks".to_string()));
        assert!(m.rows.iter().any(|r| matches!(r, Row::Item { act: Some(Do::TaskDone(k)), checked: Some(false), .. } if k == "t1")));
    }

    #[test]
    fn quiet_mode_and_sync_error_show() {
        let mut inp = base();
        inp.quiet_until = Some(now() + Duration::hours(1));
        inp.sync_error = "Login failed".into();
        let l = labels(&build(&inp));
        assert!(l.contains(&"Notifications paused until 10:00 AM".to_string()));
        assert!(l.contains(&"Resume notifications".to_string()));
        assert!(l.contains(&"Sync problem: Login failed".to_string()));
    }

    #[test]
    fn background_actions_do_not_raise_the_window() {
        assert!(!Do::QuickTask.needs_window());
        assert!(!Do::TaskDone("x".into()).needs_window());
        assert!(Do::Card("reply:1".into()).needs_window());
        assert!(Do::OpenApp.needs_window());
    }

    #[test]
    fn clip_shortens_on_character_boundaries() {
        assert_eq!(clip("  hello   world ", 20), "hello world");
        assert_eq!(clip("abcdefghij", 6), "abc...");
        assert_eq!(clip("cafe\u{301}s are great", 8), "cafe\u{301}...");
    }
}
