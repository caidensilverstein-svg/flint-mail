// Reads an iCal feed (for example a Google Calendar "secret address in iCal format")
// and expands repeating events for the window the calendar view needs.
use std::collections::{HashMap, HashSet};
use std::time::Duration as StdDuration;

use chrono::{DateTime, Datelike, Duration, Local, NaiveDate, NaiveDateTime, TimeZone, Utc, Weekday};

use crate::mac::MacEvent;

#[derive(Clone, Copy)]
enum Zone {
    Utc,
    Local,
    Tz(chrono_tz::Tz),
}

impl Zone {
    fn resolve(self, n: NaiveDateTime) -> Option<DateTime<Local>> {
        match self {
            Zone::Utc => Some(Utc.from_utc_datetime(&n).with_timezone(&Local)),
            Zone::Local => Local.from_local_datetime(&n).earliest(),
            Zone::Tz(tz) => tz.from_local_datetime(&n).earliest().map(|d| d.with_timezone(&Local)),
        }
    }
}

struct Stamp {
    naive: NaiveDateTime,
    zone: Zone,
    all_day: bool,
}

fn parse_stamp(params: &str, value: &str) -> Option<Stamp> {
    let v = value.trim();
    let all_day = params.contains("VALUE=DATE") && !params.contains("DATE-TIME") || v.len() == 8;
    if all_day {
        let d = NaiveDate::parse_from_str(&v[..8], "%Y%m%d").ok()?;
        return Some(Stamp { naive: d.and_hms_opt(0, 0, 0)?, zone: Zone::Local, all_day: true });
    }
    let utc = v.ends_with('Z');
    let naive = NaiveDateTime::parse_from_str(v.trim_end_matches('Z'), "%Y%m%dT%H%M%S").ok()?;
    let zone = if utc {
        Zone::Utc
    } else if let Some(tz) = params.split(';').find_map(|p| p.strip_prefix("TZID=")) {
        tz.trim_matches('"').parse::<chrono_tz::Tz>().map(Zone::Tz).unwrap_or(Zone::Local)
    } else {
        Zone::Local
    };
    Some(Stamp { naive, zone, all_day: false })
}

fn unescape(s: &str) -> String {
    s.replace("\\n", " ").replace("\\N", " ").replace("\\,", ",").replace("\\;", ";").replace("\\\\", "\\")
}

#[derive(Default)]
struct Raw {
    uid: String,
    title: String,
    location: String,
    start: Option<Stamp>,
    end: Option<Stamp>,
    rrule: String,
    exdates: Vec<NaiveDateTime>,
    recurrence_id: Option<Stamp>,
    cancelled: bool,
}

fn weekday_code(w: Weekday) -> &'static str {
    ["MO", "TU", "WE", "TH", "FR", "SA", "SU"][w.num_days_from_monday() as usize]
}

fn nth_weekday_of_month(d: NaiveDate) -> (i32, i32) {
    let nth = (d.day() as i32 - 1) / 7 + 1;
    let next_month = if d.month() == 12 { NaiveDate::from_ymd_opt(d.year() + 1, 1, 1) } else { NaiveDate::from_ymd_opt(d.year(), d.month() + 1, 1) };
    let days_in_month = next_month.map(|n| (n - NaiveDate::from_ymd_opt(d.year(), d.month(), 1).unwrap()).num_days()).unwrap_or(30);
    let from_end = -(((days_in_month - d.day() as i64) / 7) as i32 + 1);
    (nth, from_end)
}

fn occurrences(raw: &Raw, start: &Stamp, win_start: DateTime<Local>, win_end: DateTime<Local>) -> Vec<NaiveDateTime> {
    let first = start.naive;
    if raw.rrule.is_empty() {
        return vec![first];
    }
    let mut freq = "";
    let mut interval = 1i64;
    let mut count: Option<usize> = None;
    let mut until: Option<DateTime<Local>> = None;
    let mut byday: Vec<String> = Vec::new();
    for part in raw.rrule.split(';') {
        let (k, v) = part.split_once('=').unwrap_or((part, ""));
        match k {
            "FREQ" => freq = v,
            "INTERVAL" => interval = v.parse().unwrap_or(1).max(1),
            "COUNT" => count = v.parse().ok(),
            "UNTIL" => until = parse_stamp("", v).and_then(|s| s.zone.resolve(s.naive)),
            "BYDAY" => byday = v.split(',').map(|s| s.to_string()).collect(),
            _ => {}
        }
    }
    let start_date = first.date();
    let time = first.time();
    let mut out = Vec::new();
    let monday = |d: NaiveDate| d - Duration::days(d.weekday().num_days_from_monday() as i64);
    let matches = |d: NaiveDate| -> bool {
        if d < start_date {
            return false;
        }
        match freq {
            "DAILY" => (d - start_date).num_days() % interval == 0,
            "WEEKLY" => {
                let weeks = (monday(d) - monday(start_date)).num_days() / 7;
                let ok_day = if byday.is_empty() { d.weekday() == start_date.weekday() } else { byday.iter().any(|b| b.trim_start_matches(|c: char| c == '-' || c == '+' || c.is_ascii_digit()) == weekday_code(d.weekday())) };
                weeks % interval == 0 && ok_day
            }
            "MONTHLY" => {
                let months = (d.year() - start_date.year()) as i64 * 12 + d.month() as i64 - start_date.month() as i64;
                if months % interval != 0 {
                    return false;
                }
                if byday.is_empty() {
                    d.day() == start_date.day()
                } else {
                    let (nth, from_end) = nth_weekday_of_month(d);
                    byday.iter().any(|b| {
                        let code = b.trim_start_matches(|c: char| c == '-' || c == '+' || c.is_ascii_digit());
                        let ord: i32 = b[..b.len() - code.len()].parse().unwrap_or(0);
                        code == weekday_code(d.weekday()) && (ord == 0 || ord == nth || ord == from_end)
                    })
                }
            }
            "YEARLY" => (d.year() - start_date.year()) as i64 % interval == 0 && d.month() == start_date.month() && d.day() == start_date.day(),
            _ => false,
        }
    };
    let last_date = win_end.date_naive();
    let mut d = if count.is_some() { start_date } else { (win_start.date_naive() - Duration::days(2)).max(start_date) };
    let mut n = 0usize;
    while d <= last_date && n < 5000 {
        if matches(d) {
            let at = d.and_time(time);
            if count.map_or(true, |c| out.len() < c) {
                if let Some(u) = until {
                    if let Some(l) = start.zone.resolve(at) {
                        if l > u {
                            break;
                        }
                    }
                }
                out.push(at);
            } else {
                break;
            }
        }
        d += Duration::days(1);
        n += 1;
    }
    out
}

pub fn fetch(url: &str) -> Result<(String, Vec<MacEvent>), String> {
    let url = url.trim().replacen("webcal://", "https://", 1);
    let body = ureq::get(&url)
        .timeout(StdDuration::from_secs(30))
        .call()
        .map_err(|e| format!("Could not load the calendar link: {}", e))?
        .into_string()
        .map_err(|e| e.to_string())?;
    let now = Local::now();
    parse(&body, now - Duration::days(7), now + Duration::days(45))
}

pub fn parse(body: &str, win_start: DateTime<Local>, win_end: DateTime<Local>) -> Result<(String, Vec<MacEvent>), String> {
    if !body.contains("BEGIN:VCALENDAR") {
        return Err("That link is not an iCal calendar".into());
    }
    let mut lines: Vec<String> = Vec::new();
    for l in body.lines() {
        if (l.starts_with(' ') || l.starts_with('\t')) && !lines.is_empty() {
            lines.last_mut().unwrap().push_str(&l[1..]);
        } else {
            lines.push(l.to_string());
        }
    }
    let mut cal_name = String::from("Google Calendar");
    let mut raws: Vec<Raw> = Vec::new();
    let mut cur: Option<Raw> = None;
    for l in &lines {
        if l == "BEGIN:VEVENT" {
            cur = Some(Raw::default());
            continue;
        }
        if l == "END:VEVENT" {
            if let Some(r) = cur.take() {
                raws.push(r);
            }
            continue;
        }
        let Some((head, value)) = l.split_once(':') else { continue };
        let (name, params) = head.split_once(';').unwrap_or((head, ""));
        let Some(r) = cur.as_mut() else {
            if name == "X-WR-CALNAME" {
                cal_name = unescape(value);
            }
            continue;
        };
        match name {
            "UID" => r.uid = value.to_string(),
            "SUMMARY" => r.title = unescape(value),
            "LOCATION" => r.location = unescape(value),
            "DTSTART" => r.start = parse_stamp(params, value),
            "DTEND" => r.end = parse_stamp(params, value),
            "RRULE" => r.rrule = value.to_string(),
            "EXDATE" => {
                for v in value.split(',') {
                    if let Some(s) = parse_stamp(params, v) {
                        r.exdates.push(s.naive);
                    }
                }
            }
            "RECURRENCE-ID" => r.recurrence_id = parse_stamp(params, value),
            "STATUS" => r.cancelled = value.trim() == "CANCELLED",
            _ => {}
        }
    }
    let overridden: HashSet<(String, NaiveDateTime)> = raws.iter().filter_map(|r| r.recurrence_id.as_ref().map(|s| (r.uid.clone(), s.naive))).collect();
    let mut events = Vec::new();
    let mut seen: HashMap<(String, i64), ()> = HashMap::new();
    for r in &raws {
        let Some(start) = &r.start else { continue };
        if r.cancelled {
            continue;
        }
        let dur = match &r.end {
            Some(e) => e.naive - start.naive,
            None if start.all_day => Duration::days(1),
            None => Duration::hours(1),
        };
        let occ = if r.recurrence_id.is_some() { vec![start.naive] } else { occurrences(r, start, win_start, win_end) };
        for naive in occ {
            if r.recurrence_id.is_none() && (r.exdates.contains(&naive) || overridden.contains(&(r.uid.clone(), naive))) {
                continue;
            }
            let Some(s) = start.zone.resolve(naive) else { continue };
            let Some(e) = start.zone.resolve(naive + dur) else { continue };
            if e < win_start || s > win_end {
                continue;
            }
            if seen.insert((r.uid.clone(), s.timestamp()), ()).is_some() {
                continue;
            }
            events.push(MacEvent { calendar: cal_name.clone(), title: if r.title.is_empty() { "(no title)".into() } else { r.title.clone() }, start: s, end: e, location: r.location.clone(), all_day: start.all_day });
        }
    }
    Ok((cal_name, events))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expands_weekly_and_honors_exdate_and_overrides() {
        let ics = "BEGIN:VCALENDAR\r\nX-WR-CALNAME:Test\r\nBEGIN:VEVENT\r\nUID:a\r\nDTSTART;TZID=America/New_York:20261102T090000\r\nDTEND;TZID=America/New_York:20261102T093000\r\nRRULE:FREQ=WEEKLY;BYDAY=MO,WE\r\nEXDATE;TZID=America/New_York:20261104T090000\r\nSUMMARY:Standup\r\nEND:VEVENT\r\nBEGIN:VEVENT\r\nUID:b\r\nDTSTART;VALUE=DATE:20261106\r\nSUMMARY:Holiday\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
        let ws = Local.with_ymd_and_hms(2026, 11, 1, 0, 0, 0).unwrap();
        let we = Local.with_ymd_and_hms(2026, 11, 15, 0, 0, 0).unwrap();
        let (name, ev) = parse(ics, ws, we).unwrap();
        assert_eq!(name, "Test");
        let standups = ev.iter().filter(|e| e.title == "Standup").count();
        assert_eq!(standups, 3, "Mon 2, Mon 9, Wed 11 (Wed 4 excluded)... got {:?}", ev.iter().map(|e| e.start.to_string()).collect::<Vec<_>>());
        assert_eq!(ev.iter().filter(|e| e.title == "Holiday").count(), 1);
    }
}
