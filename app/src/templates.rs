// Reusable email text: starter library, variable expansion, ranking, and /slash matching.
use chrono::NaiveDate;

use crate::data::Template;

fn t(name: &str, subject: &str, body: &str) -> Template {
    Template { name: name.into(), subject: subject.into(), body: body.into(), uses: 0 }
}

pub fn starters() -> Vec<Template> {
    vec![
        t("Quick yes", "", "Hi {name},\n\nYes, that works for me. Thanks!"),
        t("Quick no", "", "Hi {name},\n\nThanks for thinking of me. I'm going to pass this time, but I appreciate you asking."),
        t("Thank you", "", "Hi {name},\n\nThank you, I really appreciate it."),
        t("Let me check", "", "Hi {name},\n\nLet me check on this and get back to you by end of day tomorrow."),
        t("Follow up", "Following up", "Hi {name},\n\nJust following up on my note below. Do you have any thoughts when you get a chance?"),
        t("Gentle nudge", "", "Hi {name},\n\nJust bumping this to the top of your inbox. No rush, but I wanted to make sure it didn't get buried."),
        t("Reschedule", "Can we move our meeting?", "Hi {name},\n\nSomething came up and I need to move our meeting. Could we try {date} or another time that suits you?"),
        t("Decline politely", "", "Hi {name},\n\nThank you for the invitation. Unfortunately I can't make it work, but I hope it goes well."),
        t("Accept invitation", "", "Hi {name},\n\nThanks for the invite, I'd be glad to join. See you then."),
        t("Meeting recap", "Notes from today", "Hi {name},\n\nThanks for the time today. Here is a quick recap of what we agreed:\n\n- \n- \n\nLet me know if I missed anything."),
        t("Ask for an update", "Quick check-in", "Hi {name},\n\nCould you give me a quick update on where this stands? Even a rough timeline helps."),
        t("Chase invoice", "Invoice reminder", "Hi {name},\n\nA friendly reminder that the attached invoice is still open. Could you let me know when I can expect payment?"),
        t("Introduce myself", "Nice to meet you", "Hi {name},\n\nI'm {my_name}. I wanted to introduce myself and see if we could find a time to connect."),
        t("Intro two people", "Introduction", "Hi both,\n\nConnecting you two since you're working on similar problems. I'll let you take it from here."),
        t("Ask for an intro", "Would you be open to an intro?", "Hi {name},\n\nWould you be open to introducing me to someone on your team? Happy to send a short blurb that makes it easy to forward."),
        t("Out of office", "Out of office", "Hi {name},\n\nI'm away and will be back on {date}. I'll reply when I'm back."),
        t("Apology", "", "Hi {name},\n\nSorry about the delay and for any confusion. Here is where things stand:"),
        t("Referral thanks", "Thank you for the referral", "Hi {name},\n\nThank you so much for the referral. It means a lot, and I'll keep you posted on how it goes."),
        t("Close the loop", "", "Hi {name},\n\nThanks for the help with this. I'm going to close the loop on my side. Let me know if anything else comes up."),
        t("Not right now", "", "Hi {name},\n\nThanks for reaching out. This isn't a fit for us right now, but I'll keep your note on file."),
    ]
}

pub fn expand(body: &str, name: &str, my_name: &str, today: NaiveDate) -> String {
    let first = name.split_whitespace().next().filter(|n| !n.is_empty()).unwrap_or("there");
    body.replace("{name}", first)
        .replace("{first}", first)
        .replace("{my_name}", my_name)
        .replace("{date}", &today.format("%A, %B %-d").to_string())
        .replace("{day}", &today.format("%A").to_string())
}

// Most used first, then alphabetical; an empty query keeps everything.
pub fn rank(templates: &[Template], query: &str) -> Vec<usize> {
    let q = query.trim().to_lowercase();
    let mut idx: Vec<usize> = (0..templates.len()).filter(|&i| q.is_empty() || templates[i].name.to_lowercase().contains(&q) || templates[i].body.to_lowercase().contains(&q)).collect();
    idx.sort_by(|&a, &b| templates[b].uses.cmp(&templates[a].uses).then_with(|| templates[a].name.to_lowercase().cmp(&templates[b].name.to_lowercase())));
    idx
}

// The trailing "/word" the user is typing, without the slash.
pub fn slash_query(body: &str) -> Option<&str> {
    let last = body.rsplit(|c: char| c.is_whitespace()).next()?;
    let q = last.strip_prefix('/')?;
    (q.chars().all(|c| c.is_alphanumeric() || c == '-')).then_some(q)
}

pub fn replace_slash(body: &str, text: &str) -> String {
    match slash_query(body) {
        Some(q) => format!("{}{}", &body[..body.len() - q.len() - 1], text),
        None => format!("{}{}", body, text),
    }
}

pub fn slug(name: &str) -> String {
    name.to_lowercase().split_whitespace().collect::<Vec<_>>().join("-")
}

// Adds starters whose names are missing; returns how many were added.
pub fn merge_starters(existing: &mut Vec<Template>) -> usize {
    let mut added = 0;
    for s in starters() {
        if !existing.iter().any(|e| e.name.eq_ignore_ascii_case(&s.name)) {
            existing.push(s);
            added += 1;
        }
    }
    added
}

#[cfg(test)]
mod tests {
    use super::*;

    fn day() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 11, 6).unwrap()
    }

    #[test]
    fn expands_variables_and_defaults_the_name() {
        assert_eq!(expand("Hi {name}, from {my_name} on {date}", "Maya Chen", "Caiden", day()), "Hi Maya, from Caiden on Friday, November 6");
        assert_eq!(expand("Hi {name}", "", "C", day()), "Hi there");
    }

    #[test]
    fn ranks_by_use_then_name_and_filters() {
        let mut ts = vec![t("Zeta", "", "a"), t("Alpha", "", "b"), t("Mid", "", "needle")];
        ts[0].uses = 5;
        assert_eq!(rank(&ts, ""), vec![0, 1, 2]);
        assert_eq!(rank(&ts, "needle"), vec![2]);
        assert_eq!(rank(&ts, "AL"), vec![1]);
    }

    #[test]
    fn slash_query_only_reads_the_last_word() {
        assert_eq!(slash_query("Hello\n/than"), Some("than"));
        assert_eq!(slash_query("see a/b"), None);
        assert_eq!(slash_query("done "), None);
        assert_eq!(slash_query("/"), Some(""));
        assert_eq!(replace_slash("Hi\n/than", "Thank you."), "Hi\nThank you.");
    }

    #[test]
    fn starter_library_is_large_unique_and_merges_once() {
        let s = starters();
        assert!(s.len() >= 20);
        let mut names: Vec<_> = s.iter().map(|x| x.name.to_lowercase()).collect();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), s.len());
        let mut mine = vec![t("Intro two people", "x", "custom")];
        let added = merge_starters(&mut mine);
        assert_eq!(added, s.len() - 1);
        assert_eq!(merge_starters(&mut mine), 0);
        assert_eq!(mine[0].body, "custom");
    }
}
