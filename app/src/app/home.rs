// Needs You: the screen Flint opens on, plus the card actions shared with the menu bar.
use super::*;
use crate::home::{self as hm, Card, EventIn, Kind, MailIn, TaskIn, WaitIn};
use crate::insights;

pub(super) fn task_key(t: &mac::Task) -> String {
    if t.id.is_empty() {
        format!("local:{}", t.title)
    } else {
        t.id.clone()
    }
}

impl FlintApp {
    pub(super) fn home_cards(&self, now: DateTime<Local>, max: usize) -> Vec<Card> {
        let cutoff = now - Duration::days(14);
        let mut recent: Vec<&Mail> = self.mails.iter().filter(|m| self.active(m) && !m.done && !m.set_aside && m.received > cutoff && !(self.settings.gatekeeper && m.new_sender)).collect();
        recent.sort_by(|a, b| b.received.cmp(&a.received));
        recent.truncate(150);
        let mails: Vec<MailIn> = recent
            .into_iter()
            .map(|m| {
                let ins = self.insight_or_guess(m);
                let dismissed = |k: &str| ins.dismissed.iter().any(|d| d == k);
                let addr = m.from_addr.to_lowercase();
                let answered = self.sent.iter().any(|s| s.at > m.received && s.to.to_lowercase().contains(&addr));
                let event = ins
                    .event
                    .as_ref()
                    .filter(|_| !dismissed("found"))
                    .and_then(|e| insights::parse_when(&e.start).map(|s| (e.title.clone(), s)))
                    .filter(|(t, s)| !self.event_exists(t, *s));
                let deadline = (!ins.deadline_what.is_empty() && !dismissed("deadline")).then(|| insights::parse_when(&ins.deadline_date).map(|d| (ins.deadline_what.clone(), d.date_naive()))).flatten();
                MailIn {
                    id: m.id,
                    from: m.from_name.clone(),
                    subject: m.subject.clone(),
                    summary: ins.summary.clone(),
                    received: m.received,
                    unread: !m.read,
                    needs_reply: ins.needs_reply && !dismissed("reply") && !answered,
                    urgency: ins.urgency,
                    reply_by: insights::reply_by_date(&ins),
                    vip: self.settings.vip.iter().any(|v| v.eq_ignore_ascii_case(&m.from_addr)),
                    priority: m.priority,
                    event,
                    deadline,
                }
            })
            .collect();
        let events: Vec<EventIn> = self
            .upcoming_events(now)
            .into_iter()
            .map(|e| EventIn { id: e.id, title: e.title.clone(), start: e.start, end: e.end, has_link: e.link.is_some(), attendees: e.attendees.len() })
            .collect();
        let tasks: Vec<TaskIn> = self.tasks.iter().map(|t| TaskIn { key: task_key(t), title: t.title.clone(), due: t.due }).collect();
        let waits: Vec<WaitIn> = self.sent.iter().filter(|s| !s.replied).map(|s| WaitIn { id: s.id, to: s.to.clone(), subject: s.subject.clone(), sent: s.at }).collect();
        hm::rank(now, &mails, &events, &tasks, &waits, &self.home_dismissed, max)
    }

    fn home_cards_cached(&mut self, now: DateTime<Local>) -> Vec<Card> {
        if let Some((at, cards)) = &self.home_cache {
            if at.elapsed().as_millis() < 1500 {
                return cards.clone();
            }
        }
        let cards = self.home_cards(now, 5);
        self.home_cache = Some((std::time::Instant::now(), cards.clone()));
        cards
    }

    pub(super) fn event_exists(&self, title: &str, start: DateTime<Local>) -> bool {
        let t = title.to_lowercase();
        self.events.iter().any(|e| e.start.date_naive() == start.date_naive() && (e.start - start).num_minutes().abs() < 180 && {
            let et = e.title.to_lowercase();
            et.contains(&t) || t.contains(&et)
        })
    }

    pub(super) fn complete_task_key(&mut self, key: &str) {
        if let Some(i) = self.tasks.iter().position(|t| task_key(t) == key) {
            let t = self.tasks.remove(i);
            if !t.id.is_empty() {
                let id = t.id.clone();
                std::thread::spawn(move || mac::complete_reminder(&id));
            }
            self.toast(format!("Done: {}", t.title));
        }
        self.home_cache = None;
        self.last_tray_refresh = Local::now() - Duration::days(1);
    }

    fn dismiss_insight(&mut self, mail_id: u64, kind: &str) {
        let Some(m) = self.mail(mail_id) else { return };
        let key = mail_key(&m.account, &m.from_addr, m.received);
        let mut ins = self.insight_or_guess(m);
        if !ins.dismissed.iter().any(|d| d == kind) {
            ins.dismissed.push(kind.to_string());
        }
        self.insights.insert(key, ins);
        self.insights_dirty = true;
    }

    pub(super) fn event_from_mail(&mut self, mail_id: u64) {
        let Some(m) = self.mail(mail_id) else { return };
        let Some(e) = self.insight_of(m).and_then(|i| i.event.clone()) else { return };
        let Some(start) = insights::parse_when(&e.start) else { return };
        let end = insights::parse_when(&e.end).filter(|x| *x > start).unwrap_or(start + Duration::hours(1));
        let note = format!("From {}: {}", m.from_name, m.subject);
        let who = m.from_addr.clone();
        let mut f = self.new_event_form(Some(start));
        f.title = e.title;
        f.location = e.location;
        f.link = e.link;
        f.end_date = end.format("%Y-%m-%d").to_string();
        f.end_time = end.format("%H:%M").to_string();
        f.notes = note;
        f.attendees = who;
        self.event_form = Some(f);
    }

    pub(super) fn open_mail(&mut self, id: u64, ctx: &egui::Context) {
        self.mode = Mode::Mail;
        self.apply(vec![Act::Select(id)], ctx);
    }

    pub(super) fn home_primary(&mut self, card: &Card, ctx: &egui::Context) {
        match &card.kind {
            Kind::Reply(id) => {
                self.mode = Mode::Mail;
                self.apply(vec![Act::Select(*id), Act::Reply(*id, false)], ctx);
            }
            Kind::Meeting(id) => {
                let link = self.events.iter().find(|e| e.id == *id).and_then(|e| e.link.clone());
                match link {
                    Some(l) => {
                        let _ = mac::open_url(&l);
                    }
                    None => {
                        self.mode = Mode::Calendar;
                        self.selected_event = Some(*id);
                    }
                }
            }
            Kind::Task(key) => self.complete_task_key(key),
            Kind::Found(id) => self.event_from_mail(*id),
            Kind::Deadline(id) => self.open_mail(*id, ctx),
            Kind::Waiting(id) => {
                if let Some(s) = self.sent.iter().find(|s| s.id == *id).cloned() {
                    let mut c = Composer::blank();
                    c.to = s.to.clone();
                    c.subject = format!("Re: {}", s.subject);
                    c.body = format!("Hi,\n\nJust bumping this to the top of your inbox. Any thoughts?\n\n{}", self.settings.signature);
                    self.composer = Some(c);
                }
            }
        }
        self.home_cache = None;
    }

    pub(super) fn home_secondary(&mut self, card: &Card) {
        match &card.kind {
            Kind::Reply(id) => self.dismiss_insight(*id, "reply"),
            Kind::Found(id) => self.dismiss_insight(*id, "found"),
            Kind::Deadline(id) => self.dismiss_insight(*id, "deadline"),
            _ => {
                self.home_dismissed.insert(card.id.clone());
            }
        }
        self.home_cache = None;
        self.last_tray_refresh = Local::now() - Duration::days(1);
    }

    pub(super) fn home_screen(&mut self, ui: &mut Ui, pal: Pal) {
        let now = Local::now();
        let ctx = ui.ctx().clone();
        let cards = self.home_cards_cached(now);
        let mut primary: Option<Card> = None;
        let mut secondary: Option<Card> = None;
        let mut goto: Option<u8> = None;
        let first_name = self.settings.accounts.first().and_then(|a| a.name.split_whitespace().next().map(|s| s.to_string())).unwrap_or_default();
        let next_event = self.upcoming_events(now).into_iter().find(|e| e.start.date_naive() == now.date_naive()).map(|e| format!("Next: {} at {}", e.title, e.start.format("%-I:%M %p")));
        let unread = self.unread_count();
        let tasks_today = self.tasks.iter().filter(|t| t.due.map_or(false, |d| d.date_naive() <= now.date_naive())).count();
        let events_today = self.upcoming_events(now).iter().filter(|e| e.start.date_naive() == now.date_naive()).count();
        let setup_needed = self.settings.accounts.is_empty();

        egui::ScrollArea::vertical().id_salt("home-scroll").auto_shrink([false, false]).show(ui, |ui| {
            let avail = ui.available_width();
            let w = (avail - 40.0).clamp(280.0, 700.0);
            let pad = ((avail - w) / 2.0).max(16.0);
            ui.horizontal_top(|ui| {
                ui.add_space(pad);
                ui.vertical(|ui| {
                    ui.set_width(w);
                    ui.add_space(44.0);
                    ui.label(RichText::new(now.format("%A, %B %-d").to_string()).size(13.0).color(pal.sub));
                    ui.add_space(2.0);
                    let hello = if first_name.is_empty() { hm::greeting(now).to_string() } else { format!("{}, {}", hm::greeting(now), first_name) };
                    ui.label(RichText::new(hello).size(30.0).strong().color(pal.text));
                    ui.add_space(4.0);
                    let sub = if setup_needed {
                        "Add your mail account to get started.".to_string()
                    } else if cards.is_empty() {
                        "You are clear. Nothing needs you right now.".to_string()
                    } else if cards.len() == 1 {
                        "One thing needs you.".to_string()
                    } else {
                        format!("{} things need you.", cards.len())
                    };
                    ui.label(RichText::new(sub).size(15.0).color(pal.sub));
                    ui.add_space(22.0);
                    if setup_needed && ui.add(egui::Button::new(RichText::new("Add account").color(Color32::WHITE).strong()).fill(ACCENT).rounding(Rounding::same(8.0))).clicked() {
                        self.settings_open = true;
                        self.settings_tab = SettingsTab::Accounts;
                    }
                    for c in &cards {
                        egui::Frame::none().fill(pal.card).stroke(Stroke::new(1.0_f32, pal.border)).rounding(Rounding::same(12.0)).inner_margin(egui::Margin::symmetric(16.0, 14.0)).show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.label(RichText::new(&c.title).size(15.0).strong().color(pal.text));
                            if !c.detail.is_empty() {
                                ui.label(RichText::new(&c.detail).size(12.5).color(pal.sub));
                            }
                            ui.add_space(8.0);
                            ui.horizontal(|ui| {
                                if primary_button(ui, c.primary).clicked() {
                                    primary = Some(c.clone());
                                }
                                if ui.button(c.secondary).clicked() {
                                    secondary = Some(c.clone());
                                }
                            });
                        });
                        ui.add_space(10.0);
                    }
                    if cards.is_empty() && !setup_needed {
                        if let Some(n) = &next_event {
                            ui.label(RichText::new(n).size(13.5).color(pal.text));
                            ui.add_space(8.0);
                        }
                    }
                    ui.add_space(14.0);
                    ui.horizontal_wrapped(|ui| {
                        for (i, label) in [format!("{} unread", unread), format!("{} task{} due", tasks_today, if tasks_today == 1 { "" } else { "s" }), format!("{} event{} today", events_today, if events_today == 1 { "" } else { "s" })].iter().enumerate() {
                            if ui.add(egui::Button::new(RichText::new(label).size(12.5).color(pal.sub)).frame(false)).clicked() {
                                goto = Some(i as u8);
                            }
                        }
                    });
                    ui.add_space(30.0);
                });
            });
        });

        if let Some(c) = primary {
            self.home_primary(&c, &ctx);
        }
        if let Some(c) = secondary {
            self.home_secondary(&c);
        }
        match goto {
            Some(0) => {
                self.mode = Mode::Mail;
                self.folder = Folder::SmartInbox;
                self.unread_only = true;
            }
            Some(_) => self.mode = Mode::Calendar,
            None => {}
        }
    }
}
