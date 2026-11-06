// Per-email analysis: what the email wants from you, plus any event, task or
// deadline hiding in it. Parsed leniently because models drift from the schema.
use std::collections::HashMap;
use std::path::PathBuf;

use chrono::{DateTime, Datelike, Local, NaiveDate, NaiveDateTime, TimeZone};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::data::{Category, Mail};

#[derive(Clone, Serialize, Deserialize, Default, Debug, PartialEq)]
pub struct InsightEvent {
    pub title: String,
    pub start: String,
    pub end: String,
    pub location: String,
    pub link: String,
}

#[derive(Clone, Serialize, Deserialize, Default, Debug, PartialEq)]
pub struct InsightTask {
    pub title: String,
    pub due: String,
}

#[derive(Clone, Serialize, Deserialize, Default, Debug, PartialEq)]
#[serde(default)]
pub struct Insight {
    pub summary: String,
    pub needs_reply: bool,
    pub reply_by: String,
    pub urgency: u8,
    pub kind: String,
    pub event: Option<InsightEvent>,
    pub task: Option<InsightTask>,
    pub deadline_what: String,
    pub deadline_date: String,
    pub replies: Vec<String>,
    pub facts: Vec<String>,
    pub from_ai: bool,
    pub dismissed: Vec<String>,
    pub on_calendar: bool,
}

pub const SYSTEM: &str = "You read one email and return ONLY a JSON object, no prose. Be conservative: leave fields empty rather than guess.";

pub fn trim_body(body: &str, max: usize) -> String {
    let mut out = String::new();
    for line in body.lines() {
        let t = line.trim();
        if t.starts_with('>') {
            continue;
        }
        let l = t.to_lowercase();
        if (l.starts_with("on ") && l.contains(" wrote:")) || l.starts_with("-----original message") || l.starts_with("from: ") && out.len() > 200 {
            break;
        }
        out.push_str(t);
        out.push('\n');
        if out.chars().count() > max {
            break;
        }
    }
    out.chars().take(max).collect::<String>().trim().to_string()
}

const SCHEMA: &str = "Return JSON with exactly these keys:
\"summary\": one plain sentence, max 18 words, what this is and what it wants
\"needs_reply\": true only if a person is waiting for an answer from me
\"reply_by\": YYYY-MM-DD if a reply deadline is stated or clearly implied, else \"\"
\"urgency\": 0 none, 1 normal, 2 soon, 3 urgent
\"kind\": one of person, scheduling, receipt, travel, newsletter, notification
\"event\": null, or {\"title\",\"start\":\"YYYY-MM-DD HH:MM\",\"end\":\"YYYY-MM-DD HH:MM or empty\",\"location\",\"link\"} only for a real meeting, appointment, trip or reservation with a date
\"task\": null, or {\"title\":\"short imperative\",\"due\":\"YYYY-MM-DD or empty\"} only if I must do something
\"deadline\": null, or {\"what\",\"date\":\"YYYY-MM-DD\"} for a stated deadline
\"replies\": up to 3 short reply options I could send, written as me, only when a reply makes sense (for example an accept, a decline, a 'let me check and get back to you')
\"facts\": up to 3 short key facts (amounts, confirmation numbers, names)";

fn today_line(today: NaiveDate) -> String {
    format!("Today is {} ({}).", today.format("%Y-%m-%d"), today.format("%A"))
}

// Splits a message into what the sender wrote and the quoted history below it.
pub fn split_quoted(body: &str) -> (String, String) {
    let lines: Vec<&str> = body.lines().collect();
    let cut = lines.iter().position(|l| {
        let t = l.trim();
        let low = t.to_lowercase();
        t.starts_with('>') || (low.starts_with("on ") && low.ends_with("wrote:")) || low.starts_with("-----original message")
    });
    match cut {
        Some(i) if i > 0 => (lines[..i].join("\n").trim_end().to_string(), lines[i..].join("\n")),
        _ => (body.to_string(), String::new()),
    }
}

pub fn build_prompt(m: &Mail, today: NaiveDate) -> String {
    format!("{}\nFrom: {} <{}>\nSubject: {}\n\n{}\n\n{}", today_line(today), m.from_name, m.from_addr, m.subject, trim_body(&m.body, 3500), SCHEMA)
}

pub fn build_clip_prompt(text: &str, today: NaiveDate) -> String {
    format!("{}\nThis text came from my clipboard.\n\n{}\n\n{}", today_line(today), trim_body(text, 3500), SCHEMA)
}

fn s(v: &Value, key: &str) -> String {
    match &v[key] {
        Value::String(x) => x.trim().to_string(),
        Value::Number(n) => n.to_string(),
        _ => String::new(),
    }
}

fn truthy(v: &Value) -> bool {
    match v {
        Value::Bool(b) => *b,
        Value::String(x) => matches!(x.trim().to_lowercase().as_str(), "true" | "yes"),
        Value::Number(n) => n.as_i64().unwrap_or(0) != 0,
        _ => false,
    }
}

fn obj(v: &Value) -> Option<&Value> {
    v.is_object().then_some(v)
}

pub fn parse(text: &str) -> Option<Insight> {
    let v = crate::ai::extract_json(text)?;
    let event = obj(&v["event"]).map(|e| InsightEvent { title: s(e, "title"), start: s(e, "start"), end: s(e, "end"), location: s(e, "location"), link: s(e, "link") }).filter(|e| !e.title.is_empty() && parse_when(&e.start).is_some());
    let task = obj(&v["task"]).map(|t| InsightTask { title: s(t, "title"), due: s(t, "due") }).filter(|t| !t.title.is_empty());
    let (deadline_what, deadline_date) = obj(&v["deadline"]).map(|d| (s(d, "what"), s(d, "date"))).filter(|(w, d)| !w.is_empty() && parse_when(d).is_some()).unwrap_or_default();
    let strings = |key: &str, max: usize| -> Vec<String> {
        v[key].as_array().map(|a| a.iter().filter_map(|x| x.as_str()).map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).take(max).collect()).unwrap_or_default()
    };
    let summary = s(&v, "summary");
    if summary.is_empty() && event.is_none() && task.is_none() {
        return None;
    }
    Some(Insight {
        summary,
        needs_reply: truthy(&v["needs_reply"]),
        reply_by: s(&v, "reply_by"),
        urgency: v["urgency"].as_u64().unwrap_or(1).min(3) as u8,
        kind: s(&v, "kind"),
        event,
        task,
        deadline_what,
        deadline_date,
        replies: strings("replies", 3),
        facts: strings("facts", 3),
        from_ai: true,
        dismissed: Vec::new(),
        on_calendar: false,
    })
}

pub fn parse_when(text: &str) -> Option<DateTime<Local>> {
    let t = text.trim().replace('T', " ");
    if t.is_empty() {
        return None;
    }
    let naive = NaiveDateTime::parse_from_str(&t, "%Y-%m-%d %H:%M")
        .or_else(|_| NaiveDateTime::parse_from_str(&t, "%Y-%m-%d %H:%M:%S"))
        .ok()
        .or_else(|| NaiveDate::parse_from_str(&t, "%Y-%m-%d").ok().and_then(|d| d.and_hms_opt(9, 0, 0)))?;
    Local.from_local_datetime(&naive).single()
}

// No-AI estimate so the home screen still ranks mail without a model.
pub fn heuristic(m: &Mail) -> Insight {
    let text = format!("{} {}", m.subject, trim_body(&m.body, 1500)).to_lowercase();
    let asks = text.contains('?') || ["could you", "can you", "please", "let me know", "reply", "get back to me", "thoughts"].iter().any(|k| text.contains(k));
    let person = matches!(m.category, Category::People);
    let urgent = ["urgent", "asap", "today", "deadline", "by eod", "tomorrow"].iter().any(|k| text.contains(k));
    Insight {
        summary: crate::ai::sentences(&trim_body(&m.body, 600)).into_iter().next().unwrap_or_default().chars().take(140).collect(),
        needs_reply: person && asks,
        urgency: if urgent { 2 } else { 1 },
        kind: if person { "person".into() } else { "notification".into() },
        ..Default::default()
    }
}

pub fn reply_by_date(i: &Insight) -> Option<NaiveDate> {
    parse_when(&i.reply_by).map(|d| d.date_naive())
}

pub fn days_until(d: NaiveDate, today: NaiveDate) -> i64 {
    (d - today).num_days()
}

pub fn weekday_label(d: NaiveDate) -> String {
    format!("{} {}", d.format("%a"), d.day())
}

fn path() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("Flint Mail").join("insights.json"))
}

pub fn load() -> HashMap<String, Insight> {
    path().and_then(|p| std::fs::read_to_string(p).ok()).and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

pub fn save(map: &HashMap<String, Insight>) {
    if let Some(p) = path() {
        if let Some(dir) = p.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        // keep the file small: drop the oldest entries beyond a few thousand
        if let Ok(s) = serde_json::to_string(map) {
            let _ = std::fs::write(p, s);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_full_answer() {
        let text = r#"```json
{"summary":"Maya invites you to dinner Thursday.","needs_reply":"true","reply_by":"2026-11-10","urgency":2,"kind":"scheduling",
"event":{"title":"Dinner with Maya","start":"2026-11-12 19:00","end":"","location":"Nopa","link":""},
"task":null,"deadline":null,"replies":["Yes, see you there","Can't make it"],"facts":[],}
```"#;
        let i = parse(text).unwrap();
        assert!(i.needs_reply);
        assert_eq!(i.urgency, 2);
        assert_eq!(i.event.as_ref().unwrap().location, "Nopa");
        assert_eq!(i.replies.len(), 2);
        assert!(i.task.is_none());
    }

    #[test]
    fn drops_events_without_a_valid_date() {
        let i = parse(r#"{"summary":"Hi","event":{"title":"Lunch","start":"sometime soon"}}"#).unwrap();
        assert!(i.event.is_none());
    }

    #[test]
    fn splits_quoted_history() {
        let (main, quoted) = split_quoted("Sounds good!\n\nOn Mon, Nov 2, Maya wrote:\n> are you free?");
        assert_eq!(main, "Sounds good!");
        assert!(quoted.starts_with("On Mon"));
        let (main, quoted) = split_quoted("No quotes here");
        assert_eq!(main, "No quotes here");
        assert!(quoted.is_empty());
        let (main, _) = split_quoted("> only quoted");
        assert_eq!(main, "> only quoted");
    }

    #[test]
    fn rejects_empty_answers() {
        assert!(parse("sorry I cannot").is_none());
        assert!(parse("{}").is_none());
    }

    #[test]
    fn parses_dates_and_trims_quotes() {
        assert!(parse_when("2026-11-12 19:00").is_some());
        assert!(parse_when("2026-11-12T19:00").is_some());
        assert!(parse_when("2026-11-12").is_some());
        assert!(parse_when("tomorrow").is_none());
        let body = "Sounds good!\n\nOn Mon, Nov 2, Maya wrote:\n> are you free?\n> thanks";
        assert_eq!(trim_body(body, 500), "Sounds good!");
    }
}
