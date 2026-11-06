use chrono::{DateTime, Duration, Local};
use serde::{Deserialize, Serialize};

use crate::mechanics::{at, round_up_half_hour};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Category {
    People,
    Notification,
    Newsletter,
}

#[derive(Clone, Debug)]
pub struct Comment {
    pub author: String,
    pub text: String,
    pub at: DateTime<Local>,
}

#[derive(Clone, Debug)]
pub struct Mail {
    pub id: u64,
    pub account: String,
    pub from_name: String,
    pub from_addr: String,
    pub to: String,
    pub subject: String,
    pub body: String,
    pub received: DateTime<Local>,
    pub category: Category,
    pub read: bool,
    pub pinned: bool,
    pub priority: bool,
    pub done: bool,
    pub trashed: bool,
    pub set_aside: bool,
    pub snoozed_until: Option<DateTime<Local>>,
    pub someday: bool,
    pub new_sender: bool,
    pub labels: Vec<String>,
    pub attachments: Vec<String>,
    pub comments: Vec<Comment>,
    pub assigned: Option<String>,
    pub shared: bool,
    pub summary: Option<String>,
    pub can_unsubscribe: bool,
    pub uid: u32,
}

impl Mail {
    pub fn snippet(&self) -> String {
        self.body.split_whitespace().collect::<Vec<_>>().join(" ")
    }
}

#[derive(Clone, Debug)]
pub struct SentMail {
    pub id: u64,
    pub account: String,
    pub to: String,
    pub subject: String,
    pub body: String,
    pub at: DateTime<Local>,
    pub read_receipt: bool,
    pub opened: Vec<DateTime<Local>>,
    pub follow_up: Option<DateTime<Local>>,
    pub follow_up_fired: bool,
    pub replied: bool,
}

#[derive(Clone, Debug)]
pub struct Scheduled {
    pub id: u64,
    pub account: String,
    pub to: String,
    pub subject: String,
    pub body: String,
    pub send_at: DateTime<Local>,
    pub read_receipt: bool,
    pub follow_up: Option<DateTime<Local>>,
    pub attachments: Vec<std::path::PathBuf>,
}

#[derive(Clone, Debug)]
pub struct Event {
    pub id: u64,
    pub title: String,
    pub start: DateTime<Local>,
    pub end: DateTime<Local>,
    pub calendar: usize,
    pub location: String,
    pub link: Option<String>,
    pub attendees: Vec<String>,
    pub notes: String,
    pub notified: bool,
    pub all_day: bool,
    pub recurs: bool,
}

#[derive(Clone, Debug)]
pub struct CalendarInfo {
    pub name: String,
    pub visible: bool,
}

#[derive(Clone, Serialize, Deserialize, Default, Debug)]
pub struct Account {
    pub name: String,
    pub email: String,
    pub imap_host: String,
    pub imap_port: u16,
    pub smtp_host: String,
    pub smtp_port: u16,
    pub username: String,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct Template {
    pub name: String,
    pub subject: String,
    pub body: String,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct Settings {
    pub dark: bool,
    pub notifications: bool,
    pub undo_send_secs: i64,
    pub close_to_tray: bool,
    pub menu_bar: bool,
    pub menu_bar_unread: bool,
    pub menu_bar_countdown: bool,
    pub ai_model: String,
    pub ai_auto_summary: bool,
    pub ai_auto_labels: bool,
    pub gatekeeper: bool,
    pub signature: String,
    pub templates: Vec<Template>,
    pub accounts: Vec<Account>,
    pub blocked: Vec<String>,
    #[serde(default = "default_cal_view")]
    pub cal_view: String,
    #[serde(default)]
    pub hotkey_migrated: bool,
    #[serde(default = "default_zoom")]
    pub zoom: f32,
    #[serde(default)]
    pub sync_read_to_gmail: bool,
    #[serde(default = "default_groq_model")]
    pub groq_model: String,
    #[serde(default = "default_groq_fast_model")]
    pub groq_fast_model: String,
    #[serde(default = "default_true")]
    pub ai_analyze: bool,
    #[serde(default = "default_true")]
    pub start_on_home: bool,
    #[serde(default)]
    pub vip: Vec<String>,
    #[serde(default)]
    pub menu_title: String,
    #[serde(default = "default_true")]
    pub menu_needs: bool,
    #[serde(default = "default_true")]
    pub menu_today: bool,
    #[serde(default = "default_true")]
    pub menu_unread: bool,
    #[serde(default = "default_true")]
    pub menu_quick: bool,
    #[serde(default = "default_true")]
    pub menu_clipboard: bool,
}

fn default_true() -> bool {
    true
}

fn default_groq_model() -> String {
    crate::groq::DEFAULT_MODEL.into()
}

fn default_groq_fast_model() -> String {
    crate::groq::DEFAULT_FAST_MODEL.into()
}

fn default_zoom() -> f32 {
    1.0
}

fn default_cal_view() -> String {
    "week".into()
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            dark: false,
            notifications: true,
            undo_send_secs: crate::mechanics::UNDO_SEND_SECS,
            close_to_tray: true,
            menu_bar: true,
            menu_bar_unread: true,
            menu_bar_countdown: true,
            ai_model: crate::mechanics::AI_DEFAULT_MODEL.into(),
            ai_auto_summary: false,
            ai_auto_labels: true,
            gatekeeper: true,
            signature: "Best,\nAlex".into(),
            templates: vec![
                Template {
                    name: "Meeting follow-up".into(),
                    subject: "Notes from today".into(),
                    body: "Hi {name},\n\nThanks for the time today. Here is a quick recap of what we agreed:\n\n- \n- \n\nLet me know if I missed anything.".into(),
                },
                Template {
                    name: "Not right now".into(),
                    subject: "".into(),
                    body: "Hi {name},\n\nThanks for reaching out. This isn't a fit for us right now, but I'll keep your note on file.".into(),
                },
                Template {
                    name: "Intro".into(),
                    subject: "Introduction".into(),
                    body: "Hi both,\n\nConnecting you two since you're working on similar problems. I'll let you take it from here.".into(),
                },
            ],
            accounts: Vec::new(),
            blocked: Vec::new(),
            cal_view: default_cal_view(),
            hotkey_migrated: true,
            zoom: default_zoom(),
            sync_read_to_gmail: false,
            groq_model: default_groq_model(),
            groq_fast_model: default_groq_fast_model(),
            ai_analyze: true,
            start_on_home: true,
            vip: Vec::new(),
            menu_title: String::new(),
            menu_needs: true,
            menu_today: true,
            menu_unread: true,
            menu_quick: true,
            menu_clipboard: true,
        }
    }
}

pub const SAMPLE_ACCOUNTS: [&str; 2] = ["alex@flint.example", "alex@studio.example"];
pub const TEAMMATES: [&str; 4] = ["Me", "Maya Chen", "Jordan Ellis", "Priya Raman"];
pub const LABELS: [&str; 6] = ["Work", "Finance", "Travel", "Personal", "Receipts", "Follow up"];

struct Seed {
    name: &'static str,
    addr: &'static str,
    subject: &'static str,
    body: &'static str,
    mins_ago: i64,
    cat: Category,
    read: bool,
    pinned: bool,
    priority: bool,
    new_sender: bool,
    account: usize,
    attachments: &'static [&'static str],
}

const SEEDS: &[Seed] = &[
    Seed { name: "Maya Chen", addr: "maya@studio.example", subject: "Launch plan for Thursday", body: "Hi Alex,\n\nI put together the launch plan for Thursday. Could you review the timeline by Wednesday noon? The main open question is whether we ship the onboarding changes in the same release or hold them for next week.\n\nAlso, can you confirm the budget for the ads test is still 2,000? Finance asked me this morning.\n\nThanks,\nMaya", mins_ago: 18, cat: Category::People, read: false, pinned: false, priority: true, new_sender: false, account: 1, attachments: &["launch-plan.pdf"] },
    Seed { name: "Jordan Ellis", addr: "jordan@studio.example", subject: "Re: Hiring loop feedback", body: "Thanks for the notes. I agree on the second candidate, strong on systems design but we should check references. Can we sync for 15 minutes tomorrow afternoon?\n\nJordan", mins_ago: 52, cat: Category::People, read: false, pinned: false, priority: false, new_sender: false, account: 1, attachments: &[] },
    Seed { name: "Priya Raman", addr: "priya@flint.example", subject: "Weekend hike?", body: "Hey! A few of us are going up the ridge trail on Saturday morning, leaving around 8. Want to come? We'll grab lunch after.\n\nPriya", mins_ago: 95, cat: Category::People, read: false, pinned: false, priority: false, new_sender: false, account: 0, attachments: &[] },
    Seed { name: "Northwind Billing", addr: "billing@northwind.example", subject: "Your invoice for November is ready", body: "Your invoice INV-20431 for 49.00 is ready. Payment will be taken automatically on Nov 12 from the card ending 4242. No action is needed.", mins_ago: 130, cat: Category::Notification, read: false, pinned: false, priority: false, new_sender: false, account: 0, attachments: &["INV-20431.pdf"] },
    Seed { name: "Parcel Tracker", addr: "updates@parcel.example", subject: "Your package is out for delivery", body: "Good news, your order 88-1192 is out for delivery and should arrive today between 2 PM and 6 PM.", mins_ago: 170, cat: Category::Notification, read: false, pinned: false, priority: false, new_sender: false, account: 0, attachments: &[] },
    Seed { name: "Build Bot", addr: "ci@builds.example", subject: "Build #4821 passed on main", body: "All 312 checks passed for commit 9f2c1ab on main. Deploy to staging finished in 4m 12s.", mins_ago: 210, cat: Category::Notification, read: false, pinned: false, priority: false, new_sender: false, account: 1, attachments: &[] },
    Seed { name: "Calendar", addr: "calendar@flint.example", subject: "Invitation: Design review (Thu 3:30 PM)", body: "Jordan Ellis invited you to Design review on Thursday at 3:30 PM. Join with the video link in the event.", mins_ago: 260, cat: Category::Notification, read: true, pinned: false, priority: false, new_sender: false, account: 1, attachments: &[] },
    Seed { name: "The Morning Ledger", addr: "news@ledger.example", subject: "Rates, ruins and a quiet rally", body: "Today: markets drift higher as rate expectations settle. Plus, why small caps are back in the conversation, and three charts worth your time this week.", mins_ago: 300, cat: Category::Newsletter, read: false, pinned: false, priority: false, new_sender: false, account: 0, attachments: &[] },
    Seed { name: "Design Weekly", addr: "hello@designweekly.example", subject: "Issue 212: Calm interfaces", body: "This week we look at calm interfaces, a new type foundry, and a long read about building tools that get out of the way.", mins_ago: 420, cat: Category::Newsletter, read: false, pinned: false, priority: false, new_sender: false, account: 0, attachments: &[] },
    Seed { name: "Trail Club", addr: "news@trailclub.example", subject: "November trail conditions", body: "Upper trails are icy in the mornings. Bring layers and traction. Volunteer day is the 16th.", mins_ago: 1500, cat: Category::Newsletter, read: true, pinned: false, priority: false, new_sender: false, account: 0, attachments: &[] },
    Seed { name: "Sam Okafor", addr: "sam@clientco.example", subject: "Contract redlines", body: "Hi Alex,\n\nAttached are our redlines on sections 4 and 7. The payment terms are the main change: we'd like net 45 instead of net 30. Happy to talk it through on a call this week.\n\nBest,\nSam", mins_ago: 1440, cat: Category::People, read: true, pinned: true, priority: false, new_sender: false, account: 1, attachments: &["contract-v3-redlines.docx"] },
    Seed { name: "Lena Fischer", addr: "lena@flint.example", subject: "Flights for the offsite", body: "I found two options for the offsite flights. Option A leaves Tuesday at 7:10 AM, option B on Monday evening. Which do you prefer? Prices are about the same.", mins_ago: 1600, cat: Category::People, read: true, pinned: true, priority: false, new_sender: false, account: 0, attachments: &[] },
    Seed { name: "Diego Morales", addr: "diego@studio.example", subject: "Quick question on the API docs", body: "Is the rate limit 100 or 1000 requests per minute? The docs say both in different places.", mins_ago: 2000, cat: Category::People, read: true, pinned: false, priority: false, new_sender: false, account: 1, attachments: &[] },
    Seed { name: "Maya Chen", addr: "maya@studio.example", subject: "Coffee next week?", body: "Would love to catch up properly. Any morning next week works for me.", mins_ago: 2900, cat: Category::People, read: true, pinned: false, priority: false, new_sender: false, account: 1, attachments: &[] },
    Seed { name: "Market Notes", addr: "digest@marketnotes.example", subject: "Your weekly digest", body: "The five stories you missed this week, in five minutes.", mins_ago: 3100, cat: Category::Newsletter, read: true, pinned: false, priority: false, new_sender: false, account: 0, attachments: &[] },
    Seed { name: "Bank Alerts", addr: "alerts@bank.example", subject: "Card payment of 12.40", body: "A payment of 12.40 was made at Corner Cafe with your card ending 4242.", mins_ago: 3300, cat: Category::Notification, read: true, pinned: false, priority: false, new_sender: false, account: 0, attachments: &[] },
    Seed { name: "Riley Park", addr: "riley@talentfirm.example", subject: "Opportunity you might like", body: "Hi Alex, I'm reaching out about a senior role at a fast-growing company. Would you be open to a quick chat?", mins_ago: 80, cat: Category::People, read: false, pinned: false, priority: false, new_sender: true, account: 1, attachments: &[] },
    Seed { name: "Casey Wu", addr: "casey@neighbors.example", subject: "Lost cat on Elm Street", body: "Hi neighbor, our grey cat Pepper slipped out last night. If you see her please let us know!", mins_ago: 240, cat: Category::People, read: false, pinned: false, priority: false, new_sender: true, account: 0, attachments: &[] },
    Seed { name: "Promo Deals", addr: "offers@deals.example", subject: "48 hours only: 60% off everything", body: "Our biggest sale of the season is here. Don't miss out!", mins_ago: 400, cat: Category::Newsletter, read: false, pinned: false, priority: false, new_sender: true, account: 0, attachments: &[] },
];

pub fn sample(now: DateTime<Local>) -> (Vec<Mail>, Vec<SentMail>, Vec<Event>) {
    let mut mails = Vec::new();
    for (i, s) in SEEDS.iter().enumerate() {
        mails.push(Mail {
            id: i as u64 + 1,
            account: SAMPLE_ACCOUNTS[s.account].into(),
            from_name: s.name.into(),
            from_addr: s.addr.into(),
            to: SAMPLE_ACCOUNTS[s.account].into(),
            subject: s.subject.into(),
            body: s.body.into(),
            received: now - Duration::minutes(s.mins_ago),
            category: s.cat,
            read: s.read,
            pinned: s.pinned,
            priority: s.priority,
            done: false,
            trashed: false,
            set_aside: false,
            snoozed_until: None,
            someday: false,
            new_sender: s.new_sender,
            labels: Vec::new(),
            attachments: s.attachments.iter().map(|a| a.to_string()).collect(),
            comments: Vec::new(),
            assigned: None,
            shared: false,
            summary: None,
            can_unsubscribe: s.cat == Category::Newsletter,
            uid: 0,
        });
    }
    if let Some(m) = mails.iter_mut().find(|m| m.subject == "Contract redlines") {
        m.shared = true;
        m.assigned = Some("Me".into());
        m.comments.push(Comment {
            author: "Jordan Ellis".into(),
            text: "Net 45 is fine if they sign this month. Can you reply today?".into(),
            at: now - Duration::minutes(600),
        });
    }

    let sent = vec![
        SentMail {
            id: 900,
            account: SAMPLE_ACCOUNTS[1].into(),
            to: "sam@clientco.example".into(),
            subject: "Proposal for Q1".into(),
            body: "Hi Sam, attached is the proposal we discussed. Let me know what you think.".into(),
            at: now - Duration::days(3),
            read_receipt: true,
            opened: vec![now - Duration::days(2), now - Duration::hours(5)],
            follow_up: Some(now - Duration::hours(1)),
            follow_up_fired: true,
            replied: false,
        },
        SentMail {
            id: 901,
            account: SAMPLE_ACCOUNTS[0].into(),
            to: "lena@flint.example".into(),
            subject: "Offsite dates".into(),
            body: "Does the week of the 18th still work for everyone?".into(),
            at: now - Duration::days(1),
            read_receipt: true,
            opened: vec![],
            follow_up: None,
            follow_up_fired: false,
            replied: false,
        },
    ];

    let today = now.date_naive();
    let soon = round_up_half_hour(now);
    let mut events = Vec::new();
    let mut add = |id: u64, title: &str, start: DateTime<Local>, mins: i64, cal: usize, loc: &str, link: Option<&str>, who: &[&str]| {
        events.push(Event {
            id,
            title: title.into(),
            start,
            end: start + Duration::minutes(mins),
            calendar: cal,
            location: loc.into(),
            link: link.map(|s| s.into()),
            attendees: who.iter().map(|s| s.to_string()).collect(),
            notes: String::new(),
            // sample meetings never raise real Mac notifications
            notified: true,
            all_day: false,
            recurs: false,
        });
    };
    let d = |n: i64| today + Duration::days(n);
    add(1, "Team standup", at(d(0), 9, 30), 15, 0, "", Some("https://meet.example.com/standup"), &["Maya Chen", "Jordan Ellis"]);
    add(2, "Product sync", soon, 30, 0, "Room 4B", Some("https://meet.example.com/product-sync"), &["Maya Chen", "Diego Morales"]);
    add(3, "Lunch with Lena", at(d(0), 12, 30), 60, 1, "Corner Cafe", None, &["Lena Fischer"]);
    add(4, "Design review", at(d(1), 15, 30), 60, 0, "", Some("https://meet.example.com/design-review"), &["Jordan Ellis", "Maya Chen"]);
    add(5, "1:1 Jordan", at(d(1), 10, 0), 30, 0, "", Some("https://meet.example.com/one-on-one"), &["Jordan Ellis"]);
    add(6, "Dentist", at(d(2), 8, 0), 45, 1, "Main St Clinic", None, &[]);
    add(7, "Client call: contract", at(d(2), 14, 0), 45, 0, "", Some("https://meet.example.com/client"), &["Sam Okafor"]);
    add(8, "Gym", at(d(3), 7, 0), 60, 1, "Downtown gym", None, &[]);
    add(9, "Ridge trail hike", at(d(4), 8, 0), 240, 2, "Ridge trailhead", None, &["Priya Raman"]);
    add(10, "Planning offsite prep", at(d(-1), 13, 0), 90, 0, "", None, &["Lena Fischer"]);
    add(11, "Family dinner", at(d(5), 18, 30), 120, 2, "Home", None, &[]);
    add(12, "Quarterly review", at(d(8), 11, 0), 60, 0, "", Some("https://meet.example.com/qbr"), &["Maya Chen", "Jordan Ellis", "Priya Raman"]);
    add(13, "Book club", at(d(10), 19, 0), 90, 1, "Library", None, &[]);

    (mails, sent, events)
}

pub fn default_calendars() -> Vec<CalendarInfo> {
    ["Work", "Personal", "Family", "Holidays"]
        .iter()
        .map(|n| CalendarInfo { name: n.to_string(), visible: true })
        .collect()
}
