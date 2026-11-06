// Background email analysis: queues unanalyzed recent mail, two at a time.
use super::*;
use crate::insights::{self, Insight};

impl FlintApp {
    pub(super) fn insight_of(&self, m: &Mail) -> Option<&Insight> {
        self.insights.get(&mail_key(&m.account, &m.from_addr, m.received))
    }

    pub(super) fn insight_or_guess(&self, m: &Mail) -> Insight {
        self.insight_of(m).cloned().unwrap_or_else(|| insights::heuristic(m))
    }

    pub(super) fn analyze_tick(&mut self, ctx: &egui::Context) {
        if self.insights_dirty && self.last_insights_save.elapsed().as_secs() >= 15 {
            let keys: HashSet<String> = self.mails.iter().map(|m| mail_key(&m.account, &m.from_addr, m.received)).collect();
            self.insights.retain(|k, _| keys.contains(k));
            insights::save(&self.insights);
            self.insights_dirty = false;
            self.last_insights_save = std::time::Instant::now();
        }
        if !self.settings.ai_analyze || self.screenshot.is_some() || !self.engine().is_cloud() {
            return;
        }
        if self.insight_inflight.len() >= 2 || self.last_analyze.elapsed().as_millis() < 700 || self.analyze_pause_until > std::time::Instant::now() {
            return;
        }
        if self.insight_budget == 0 {
            return;
        }
        let cutoff = Local::now() - Duration::days(14);
        let recent = Local::now() - Duration::days(3);
        let today = Local::now().date_naive();
        let next = self
            .mails
            .iter()
            .filter(|m| !m.trashed && !m.done && m.received > cutoff && (!m.read || m.received > recent) && !m.body.is_empty() && !self.insight_tried.contains(&m.id) && self.insight_of(m).is_none())
            .max_by_key(|m| m.received)
            .map(|m| (m.id, insights::build_prompt(m, today)));
        let Some((id, prompt)) = next else { return };
        self.insight_budget -= 1;
        self.insight_tried.insert(id);
        self.insight_inflight.insert(id);
        self.last_analyze = std::time::Instant::now();
        self.run_ai(ctx, insights::SYSTEM, prompt, String::new(), Target::Insight(id));
    }

    pub(super) fn apply_insight(&mut self, id: u64, text: &str, used_model: bool, error: Option<&str>) {
        self.insight_inflight.remove(&id);
        if !used_model {
            if error.map_or(false, |e| e.contains("429")) {
                self.insight_tried.remove(&id);
                self.analyze_pause_until = std::time::Instant::now() + std::time::Duration::from_secs(45);
            }
            return;
        }
        let key = self.mail(id).map(|m| mail_key(&m.account, &m.from_addr, m.received));
        if let (Some(key), Some(ins)) = (key, insights::parse(text)) {
            self.insights.insert(key, ins);
            self.insights_dirty = true;
        }
    }
}
