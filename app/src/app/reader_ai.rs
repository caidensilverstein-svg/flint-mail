// AI extras in the reader: add-to-calendar banner, suggested task, reply chips.
use super::*;
use crate::insights;

fn tinted(ui: &mut Ui, pal: Pal, body: impl FnOnce(&mut Ui)) {
    egui::Frame::none().fill(pal.sel).rounding(Rounding::same(10.0)).inner_margin(egui::Margin::symmetric(14.0, 10.0)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        body(ui);
    });
    ui.add_space(8.0);
}

impl FlintApp {
    pub(super) fn reader_insights(&self, ui: &mut Ui, pal: Pal, m: &Mail, acts: &mut Vec<Act>) {
        let Some(ins) = self.insight_of(m) else { return };
        let id = m.id;
        let dismissed = |k: &str| ins.dismissed.iter().any(|d| d == k);

        if let Some(e) = &ins.event {
            if let Some(start) = insights::parse_when(&e.start) {
                let end = insights::parse_when(&e.end).filter(|x| *x > start).unwrap_or(start + Duration::hours(1));
                let exists = self.event_exists(&e.title, start);
                if exists || !dismissed("found") {
                    tinted(ui, pal, |ui| {
                        ui.label(RichText::new(&e.title).strong().size(14.0).color(pal.text));
                        let place = if e.location.is_empty() { String::new() } else { format!("  -  {}", e.location) };
                        ui.label(RichText::new(format!("{}{}", start.format("%a, %b %-d at %-I:%M %p"), place)).size(12.5).color(pal.sub));
                        if exists {
                            ui.label(RichText::new("On your calendar").size(12.0).color(GREEN));
                        } else {
                            let clash = self.events.iter().find(|x| !x.all_day && x.start < end && x.end > start);
                            if let Some(c) = clash {
                                ui.label(RichText::new(format!("Overlaps with {} at {}", c.title, c.start.format("%-I:%M %p"))).size(12.0).color(ORANGE));
                            }
                            ui.add_space(4.0);
                            ui.horizontal(|ui| {
                                if primary_button(ui, "Add to calendar").clicked() {
                                    acts.push(Act::AddEvent(id));
                                }
                                if ui.button("Not an event").clicked() {
                                    acts.push(Act::DismissInsight(id, "found"));
                                }
                            });
                        }
                    });
                }
            }
        }

        if let Some(t) = &ins.task {
            let have = self.tasks.iter().any(|x| x.title.eq_ignore_ascii_case(&t.title));
            if !have && !dismissed("task") {
                tinted(ui, pal, |ui| {
                    ui.label(RichText::new(format!("Task: {}", t.title)).strong().size(14.0).color(pal.text));
                    if let Some(d) = insights::parse_when(&t.due) {
                        ui.label(RichText::new(format!("Due {}", d.format("%a, %b %-d"))).size(12.5).color(pal.sub));
                    }
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        if primary_button(ui, "Add task").clicked() {
                            acts.push(Act::MakeTask(id));
                        }
                        if ui.button("Dismiss").clicked() {
                            acts.push(Act::DismissInsight(id, "task"));
                        }
                    });
                });
            }
        }

        if !ins.facts.is_empty() {
            ui.label(RichText::new(ins.facts.join("   -   ")).size(12.0).color(pal.sub));
            ui.add_space(6.0);
        }
    }

    pub(super) fn reader_replies(&self, ui: &mut Ui, pal: Pal, m: &Mail, acts: &mut Vec<Act>) {
        let Some(ins) = self.insight_of(m) else { return };
        if ins.replies.is_empty() {
            return;
        }
        ui.label(RichText::new("Suggested replies").size(12.0).color(pal.sub));
        ui.horizontal_wrapped(|ui| {
            for r in &ins.replies {
                let label = crate::menumodel::clip(r, 46);
                if ui.add(egui::Button::new(RichText::new(label).color(ACCENT)).fill(pal.sel)).on_hover_text(r).clicked() {
                    acts.push(Act::ReplyWith(m.id, r.clone()));
                }
            }
        });
        ui.add_space(4.0);
    }

    pub(super) fn make_task_from_mail(&mut self, id: u64) {
        let Some(m) = self.mail(id) else { return };
        let key = mail_key(&m.account, &m.from_addr, m.received);
        let (title, due) = match self.insight_of(m).and_then(|i| i.task.clone()) {
            Some(t) => (t.title, insights::parse_when(&t.due)),
            None => (format!("Follow up: {}", crate::menumodel::clip(&m.subject, 60)), None),
        };
        self.task_sources.insert(title.to_lowercase(), key);
        self.save_task_sources();
        self.add_task(title.clone(), due);
        self.toast(format!("Task added: {}", title));
    }

    pub(super) fn save_task_sources(&self) {
        if let Some(p) = dirs::config_dir().map(|d| d.join("Flint Mail").join("tasks-meta.json")) {
            if let Some(dir) = p.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            if let Ok(s) = serde_json::to_string(&self.task_sources) {
                let _ = std::fs::write(p, s);
            }
        }
    }
}

pub(super) fn load_task_sources() -> HashMap<String, String> {
    dirs::config_dir()
        .map(|d| d.join("Flint Mail").join("tasks-meta.json"))
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}
