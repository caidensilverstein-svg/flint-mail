// What needs Caiden right now: a ranked, short list shared by the Needs You
// screen and the menu bar. Pure logic, no UI.
use std::collections::HashSet;

use chrono::{DateTime, Local, NaiveDate, Timelike};

pub struct MailIn {
    pub id: u64,
    pub from: String,
    pub subject: String,
    pub summary: String,
    pub received: DateTime<Local>,
    pub unread: bool,
    pub needs_reply: bool,
    pub urgency: u8,
    pub reply_by: Option<NaiveDate>,
    pub vip: bool,
    pub priority: bool,
    pub event: Option<(String, DateTime<Local>)>,
    pub deadline: Option<(String, NaiveDate)>,
}

pub struct EventIn {
    pub id: u64,
    pub title: String,
    pub start: DateTime<Local>,
    pub end: DateTime<Local>,
    pub has_link: bool,
    pub attendees: usize,
    pub prep: String,
}

pub struct CleanIn {
    pub id: u64,
    pub from: String,
    pub count: usize,
}

#[derive(Default)]
pub struct Extras {
    pub cleanups: Vec<CleanIn>,
    pub newsletters: usize,
}

pub struct TaskIn {
    pub key: String,
    pub title: String,
    pub due: Option<DateTime<Local>>,
}

pub struct WaitIn {
    pub id: u64,
    pub to: String,
    pub subject: String,
    pub sent: DateTime<Local>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Kind {
    Reply(u64),
    Meeting(u64),
    Task(String),
    Found(u64),
    Deadline(u64),
    Waiting(u64),
    Cleanup(u64),
    Digest,
}

#[derive(Clone, Debug)]
pub struct Card {
    pub id: String,
    pub kind: Kind,
    pub title: String,
    pub detail: String,
    pub score: f32,
    pub primary: &'static str,
    pub secondary: &'static str,
}

pub fn greeting(now: DateTime<Local>) -> &'static str {
    match now.hour() {
        5..=11 => "Good morning",
        12..=17 => "Good afternoon",
        _ => "Good evening",
    }
}

fn ago(now: DateTime<Local>, t: DateTime<Local>) -> String {
    let h = (now - t).num_hours();
    if h < 1 {
        "just now".into()
    } else if h < 24 {
        format!("{}h ago", h)
    } else {
        let d = h / 24;
        format!("{} day{} ago", d, if d == 1 { "" } else { "s" })
    }
}

fn in_words(mins: i64) -> String {
    if mins <= 0 {
        "now".into()
    } else if mins < 60 {
        format!("in {} min", mins)
    } else {
        format!("in {}h {:02}m", mins / 60, mins % 60)
    }
}

pub fn rank(now: DateTime<Local>, mails: &[MailIn], events: &[EventIn], tasks: &[TaskIn], waits: &[WaitIn], extras: &Extras, dismissed: &HashSet<String>, max: usize) -> Vec<Card> {
    let today = now.date_naive();
    let mut cards: Vec<Card> = Vec::new();

    for m in mails {
        if m.needs_reply {
            let age_days = (now - m.received).num_hours().max(0) as f32 / 24.0;
            let mut score = 50.0 + m.urgency as f32 * 12.0 + age_days.min(7.0) * 2.0;
            if m.priority {
                score += 10.0;
            }
            if m.vip {
                score += 12.0;
            }
            if m.unread {
                score += 5.0;
            }
            let mut detail = if m.summary.is_empty() { m.subject.clone() } else { m.summary.clone() };
            if let Some(d) = m.reply_by {
                let left = (d - today).num_days();
                score += if left < 0 { 30.0 } else if left == 0 { 25.0 } else if left <= 2 { 12.0 } else { 0.0 };
                if left < 0 {
                    detail.push_str("  (reply was due)");
                } else if left == 0 {
                    detail.push_str("  (reply due today)");
                }
            }
            cards.push(Card { id: format!("reply:{}", m.id), kind: Kind::Reply(m.id), title: format!("{} is waiting on you", m.from), detail, score, primary: "Reply", secondary: "Later" });
        } else if let Some((what, d)) = &m.deadline {
            let left = (*d - today).num_days();
            if (0..=5).contains(&left) {
                let when = if left == 0 { "today".to_string() } else if left == 1 { "tomorrow".to_string() } else { format!("in {} days", left) };
                cards.push(Card { id: format!("deadline:{}", m.id), kind: Kind::Deadline(m.id), title: format!("{} due {}", what, when), detail: format!("From {}: {}", m.from, m.subject), score: 58.0 - left as f32 * 3.0, primary: "Open", secondary: "Dismiss" });
            }
        }
        if let Some((title, start)) = &m.event {
            if *start > now && (*start - now).num_days() <= 30 {
                let soon = (*start - now).num_days() <= 2;
                cards.push(Card {
                    id: format!("found:{}", m.id),
                    kind: Kind::Found(m.id),
                    title: format!("{} on {}", title, start.format("%a %b %-d at %-I:%M %p")),
                    detail: format!("Found in an email from {}", m.from),
                    score: 40.0 + if soon { 15.0 } else { 0.0 },
                    primary: "Add to calendar",
                    secondary: "Dismiss",
                });
            }
        }
    }

    for e in events {
        let mins = (e.start - now).num_minutes();
        let live = e.start <= now && e.end > now;
        if live || (0..=180).contains(&mins) {
            let score = if live { 95.0 } else { 100.0 - mins as f32 / 10.0 };
            let when = if live { "happening now".to_string() } else { in_words(mins) };
            let mut detail = when;
            if e.attendees > 0 {
                detail.push_str(&format!("  -  {} attendee{}", e.attendees, if e.attendees == 1 { "" } else { "s" }));
            }
            if !e.prep.is_empty() {
                detail.push_str(&format!("  -  {}", e.prep));
            }
            cards.push(Card { id: format!("meet:{}", e.id), kind: Kind::Meeting(e.id), title: e.title.clone(), detail, score, primary: if e.has_link { "Join" } else { "Open" }, secondary: "Dismiss" });
        }
    }

    for t in tasks {
        if let Some(due) = t.due {
            let left = (due.date_naive() - today).num_days();
            if left <= 0 {
                let score = if left < 0 { 70.0 + (-left as f32 * 2.0).min(20.0) } else { 65.0 };
                let detail = if left < 0 { format!("{} day{} overdue", -left, if left == -1 { "" } else { "s" }) } else { "Due today".to_string() };
                cards.push(Card { id: format!("task:{}", t.key), kind: Kind::Task(t.key.clone()), title: t.title.clone(), detail, score, primary: "Done", secondary: "Later" });
            }
        }
    }

    for w in waits {
        let days = (now - w.sent).num_days();
        if days >= 3 {
            cards.push(Card {
                id: format!("wait:{}", w.id),
                kind: Kind::Waiting(w.id),
                title: format!("No reply from {} yet", w.to),
                detail: format!("{}  -  sent {}", w.subject, ago(now, w.sent)),
                score: 30.0 + (days as f32 * 3.0).min(20.0),
                primary: "Nudge",
                secondary: "Drop",
            });
        }
    }

    if let Some(c) = extras.cleanups.iter().filter(|c| c.count >= 4).max_by_key(|c| c.count) {
        cards.push(Card {
            id: format!("clean:{}", c.from),
            kind: Kind::Cleanup(c.id),
            title: format!("You have not opened {} lately", c.from),
            detail: format!("{} unread emails. Unsubscribe?", c.count),
            score: 18.0 + c.count.min(10) as f32,
            primary: "Unsubscribe",
            secondary: "Keep",
        });
    }
    if extras.newsletters >= 3 {
        cards.push(Card { id: "digest".into(), kind: Kind::Digest, title: format!("{} newsletters waiting", extras.newsletters), detail: "Skim them later, or clear them in one go".into(), score: 22.0, primary: "Mark all read", secondary: "Later" });
    }

    cards.retain(|c| !dismissed.contains(&c.id));
    cards.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
    cards.truncate(max);
    cards
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, TimeZone};

    fn now() -> DateTime<Local> {
        Local.with_ymd_and_hms(2026, 11, 6, 10, 0, 0).unwrap()
    }

    fn mail(id: u64) -> MailIn {
        MailIn { id, from: "Maya".into(), subject: "Launch".into(), summary: String::new(), received: now() - Duration::hours(5), unread: true, needs_reply: false, urgency: 1, reply_by: None, vip: false, priority: false, event: None, deadline: None }
    }

    #[test]
    fn urgent_reply_outranks_plain_reply_and_vip_boosts() {
        let mut a = mail(1);
        a.needs_reply = true;
        let mut b = mail(2);
        b.needs_reply = true;
        b.urgency = 3;
        let mut c = mail(3);
        c.needs_reply = true;
        c.vip = true;
        let cards = rank(now(), &[a, b, c], &[], &[], &[], &Extras::default(), &HashSet::new(), 5);
        assert_eq!(cards[0].kind, Kind::Reply(2));
        assert_eq!(cards[1].kind, Kind::Reply(3));
        assert_eq!(cards[2].kind, Kind::Reply(1));
    }

    #[test]
    fn meeting_in_ten_minutes_beats_everything_and_far_meetings_are_hidden() {
        let mut m = mail(1);
        m.needs_reply = true;
        m.urgency = 3;
        let soon = EventIn { id: 7, title: "Standup".into(), start: now() + Duration::minutes(10), end: now() + Duration::minutes(40), has_link: true, attendees: 4, prep: String::new() };
        let far = EventIn { id: 8, title: "Later".into(), start: now() + Duration::hours(6), end: now() + Duration::hours(7), has_link: false, attendees: 0, prep: String::new() };
        let cards = rank(now(), &[m], &[soon, far], &[], &[], &Extras::default(), &HashSet::new(), 5);
        assert_eq!(cards[0].kind, Kind::Meeting(7));
        assert_eq!(cards[0].primary, "Join");
        assert_eq!(cards.len(), 2);
    }

    #[test]
    fn tasks_only_when_due_or_overdue_and_dismissed_cards_vanish() {
        let t = |key: &str, days: i64| TaskIn { key: key.into(), title: key.into(), due: Some(now() + Duration::days(days)) };
        let tasks = [t("late", -2), t("today", 0), t("later", 3)];
        let mut dismissed = HashSet::new();
        let cards = rank(now(), &[], &[], &tasks, &[], &Extras::default(), &dismissed, 5);
        assert_eq!(cards.len(), 2);
        assert_eq!(cards[0].kind, Kind::Task("late".into()));
        dismissed.insert("task:late".to_string());
        assert_eq!(rank(now(), &[], &[], &tasks, &[], &Extras::default(), &dismissed, 5).len(), 1);
    }

    #[test]
    fn waiting_needs_three_days_and_list_is_capped() {
        let w = |id, days| WaitIn { id, to: "Sam".into(), subject: "Quote".into(), sent: now() - Duration::days(days) };
        assert_eq!(rank(now(), &[], &[], &[], &[w(1, 2)], &Extras::default(), &HashSet::new(), 5).len(), 0);
        assert_eq!(rank(now(), &[], &[], &[], &[w(1, 4), w(2, 5), w(3, 6)], &Extras::default(), &HashSet::new(), 2).len(), 2);
    }

    #[test]
    fn deadline_in_two_days_shows_and_event_in_mail_is_offered() {
        let mut m = mail(1);
        m.deadline = Some(("Invoice".into(), now().date_naive() + Duration::days(2)));
        m.event = Some(("Dinner".into(), now() + Duration::days(1)));
        let cards = rank(now(), &[m], &[], &[], &[], &Extras::default(), &HashSet::new(), 5);
        assert!(cards.iter().any(|c| matches!(c.kind, Kind::Deadline(1))));
        assert!(cards.iter().any(|c| matches!(c.kind, Kind::Found(1))));
    }

    #[test]
    fn cleanup_and_digest_cards_need_enough_mail() {
        let few = Extras { cleanups: vec![CleanIn { id: 1, from: "Deals".into(), count: 3 }], newsletters: 2 };
        assert!(rank(now(), &[], &[], &[], &[], &few, &HashSet::new(), 5).is_empty());
        let many = Extras { cleanups: vec![CleanIn { id: 1, from: "Deals".into(), count: 9 }], newsletters: 7 };
        let cards = rank(now(), &[], &[], &[], &[], &many, &HashSet::new(), 5);
        assert_eq!(cards.len(), 2);
        assert_eq!(cards[0].kind, Kind::Digest);
        assert_eq!(cards[1].kind, Kind::Cleanup(1));
    }

    #[test]
    fn greeting_matches_hour() {
        assert_eq!(greeting(now()), "Good morning");
    }
}
