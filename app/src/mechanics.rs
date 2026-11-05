// Values come from app/MECHANICS.md; keep the two in step.
use chrono::{DateTime, Datelike, Duration, Local, NaiveDate, TimeZone, Timelike, Weekday};

pub const SNOOZE_LATER_TODAY_HOURS: i64 = 3;
pub const SEND_LATER_LATER_TODAY_HOURS: i64 = 2;
pub const EVENING_HOUR: u32 = 20;
pub const MORNING_HOUR: u32 = 9;
pub const WEEKEND_WEEKDAY: Weekday = Weekday::Sat;
pub const NEXT_WEEK_WEEKDAY: Weekday = Weekday::Mon;
pub const TRAY_REFRESH_MS: i64 = 60000;
pub const MEETING_LEAD_MS: i64 = 60000;
pub const UNDO_SEND_SECS: i64 = 5;
pub const UNDO_SEND_CHOICES: [i64; 5] = [5, 10, 15, 30, 60];
pub const CALENDAR_VISIBLE_HOURS: f32 = 10.0;
pub const CARD_ITEMS_LIMIT: usize = 3;
pub const AI_MAX_TOKENS: u32 = 1024;
pub const IMAP_FETCH_LIMIT: u32 = 40;
pub const AI_DEFAULT_MODEL: &str = "claude-sonnet-5-5";
pub const FOLLOW_UP_DAYS: [i64; 4] = [1, 2, 3, 7];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Preset {
    LaterToday,
    ThisEvening,
    Tomorrow,
    Weekend,
    NextWeek,
    Someday,
}

impl Preset {
    pub const SNOOZE: [Preset; 6] = [
        Preset::LaterToday,
        Preset::ThisEvening,
        Preset::Tomorrow,
        Preset::Weekend,
        Preset::NextWeek,
        Preset::Someday,
    ];
    pub const SEND_LATER: [Preset; 5] = [
        Preset::LaterToday,
        Preset::ThisEvening,
        Preset::Tomorrow,
        Preset::Weekend,
        Preset::NextWeek,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Preset::LaterToday => "Later Today",
            Preset::ThisEvening => "This Evening",
            Preset::Tomorrow => "Tomorrow",
            Preset::Weekend => "This Weekend",
            Preset::NextWeek => "Next Week",
            Preset::Someday => "Someday",
        }
    }

    pub fn resolve(self, now: DateTime<Local>, send_later: bool) -> Option<DateTime<Local>> {
        let today = now.date_naive();
        match self {
            Preset::LaterToday => {
                let h = if send_later { SEND_LATER_LATER_TODAY_HOURS } else { SNOOZE_LATER_TODAY_HOURS };
                Some(now + Duration::hours(h))
            }
            Preset::ThisEvening => {
                let t = at(today, EVENING_HOUR, 0);
                (t > now).then_some(t)
            }
            Preset::Tomorrow => Some(at(today + Duration::days(1), MORNING_HOUR, 0)),
            Preset::Weekend => Some(at(next_weekday(today, WEEKEND_WEEKDAY), MORNING_HOUR, 0)),
            Preset::NextWeek => Some(at(next_weekday(today, NEXT_WEEK_WEEKDAY), MORNING_HOUR, 0)),
            Preset::Someday => None,
        }
    }

    pub fn label(self, now: DateTime<Local>, send_later: bool) -> Option<String> {
        if self == Preset::Someday {
            return Some("Someday".into());
        }
        let t = self.resolve(now, send_later)?;
        Some(format!("{}   {}", self.name(), short_when(t, now)))
    }
}

pub fn at(d: NaiveDate, h: u32, m: u32) -> DateTime<Local> {
    let naive = d.and_hms_opt(h.min(23), m.min(59), 0).unwrap();
    Local
        .from_local_datetime(&naive)
        .earliest()
        .unwrap_or_else(|| Local.from_utc_datetime(&naive))
}

pub fn next_weekday(from: NaiveDate, wd: Weekday) -> NaiveDate {
    let mut d = from + Duration::days(1);
    while d.weekday() != wd {
        d += Duration::days(1);
    }
    d
}

pub fn short_when(t: DateTime<Local>, now: DateTime<Local>) -> String {
    let days = (t.date_naive() - now.date_naive()).num_days();
    if days == 0 {
        t.format("%-I:%M %p").to_string()
    } else if days == 1 {
        format!("Tomorrow {}", t.format("%-I:%M %p"))
    } else if (2..7).contains(&days) {
        t.format("%a %-I:%M %p").to_string()
    } else {
        t.format("%b %-d, %-I:%M %p").to_string()
    }
}

pub fn list_time(t: DateTime<Local>, now: DateTime<Local>) -> String {
    let days = (now.date_naive() - t.date_naive()).num_days();
    if days == 0 {
        t.format("%-I:%M %p").to_string()
    } else if days == 1 {
        "Yesterday".into()
    } else if (2..7).contains(&days) {
        t.format("%a").to_string()
    } else {
        t.format("%b %-d").to_string()
    }
}

pub fn countdown(start: DateTime<Local>, now: DateTime<Local>) -> String {
    let mins = (start - now).num_minutes();
    if mins <= 0 {
        "now".into()
    } else if mins < 60 {
        format!("in {}m", mins)
    } else {
        let h = mins / 60;
        let m = mins % 60;
        if m == 0 { format!("in {}h", h) } else { format!("in {}h {}m", h, m) }
    }
}

pub fn round_up_half_hour(now: DateTime<Local>) -> DateTime<Local> {
    let mut t = at(now.date_naive(), now.hour(), 0);
    while t <= now {
        t += Duration::minutes(30);
    }
    t
}
