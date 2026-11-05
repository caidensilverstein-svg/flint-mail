// AI writing and reading tools. With an API key the requests go to the Claude
// Messages API; otherwise they run on the local Ollama model; if that is
// missing, simple on-device fallbacks keep every button useful.
use std::sync::mpsc::Sender;
use std::time::Duration;

use crate::data::Mail;
use crate::mechanics::AI_MAX_TOKENS;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tone {
    Friendly,
    Neutral,
    Formal,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Target {
    Summary(u64),
    Composer,
    Copilot,
    EventNotes(u64),
    Labels,
}

pub struct AiResult {
    pub target: Target,
    pub text: String,
    pub used_model: bool,
    pub error: Option<String>,
}

pub fn spawn(
    key: Option<String>,
    model: String,
    system: String,
    prompt: String,
    fallback: String,
    target: Target,
    tx: Sender<AiResult>,
    ctx: eframe::egui::Context,
) {
    let key = key.filter(|k| !k.trim().is_empty());
    if key.is_none() && !local_model_ready() {
        let _ = tx.send(AiResult { target, text: fallback, used_model: false, error: None });
        ctx.request_repaint();
        return;
    }
    std::thread::spawn(move || {
        let out = match &key {
            Some(k) => call_claude(k, &model, &system, &prompt),
            None => call_local(&system, &prompt),
        };
        let res = match out {
            Ok(text) => AiResult { target, text, used_model: true, error: None },
            Err(e) => AiResult { target, text: fallback, used_model: false, error: Some(e) },
        };
        let _ = tx.send(res);
        ctx.request_repaint();
    });
}

const LOCAL_MODEL: &str = "qwen3.5:9b";
const LOCAL_URL: &str = "http://127.0.0.1:11434";

pub fn local_model_ready() -> bool {
    ureq::get(&format!("{}/api/tags", LOCAL_URL)).timeout(Duration::from_millis(600)).call().is_ok()
}

fn call_local(system: &str, prompt: &str) -> Result<String, String> {
    let body = serde_json::json!({
        "model": LOCAL_MODEL,
        "stream": false,
        "think": false,
        "keep_alive": "30m",
        "options": { "num_predict": AI_MAX_TOKENS, "num_ctx": 8192 },
        "messages": [
            { "role": "system", "content": system },
            { "role": "user", "content": prompt },
        ],
    });
    let v: serde_json::Value = ureq::post(&format!("{}/api/chat", LOCAL_URL))
        .timeout(Duration::from_secs(300))
        .send_json(body)
        .map_err(|e| format!("Local AI failed: {}", e))?
        .into_json()
        .map_err(|e| e.to_string())?;
    let text = v["message"]["content"].as_str().unwrap_or("").trim().to_string();
    if text.is_empty() {
        Err("The local AI returned an empty answer".into())
    } else {
        Ok(text)
    }
}

fn call_claude(key: &str, model: &str, system: &str, prompt: &str) -> Result<String, String> {
    let body = serde_json::json!({
        "model": model,
        "max_tokens": AI_MAX_TOKENS,
        "system": system,
        "messages": [{ "role": "user", "content": prompt }],
    });
    let resp = ureq::post("https://api.anthropic.com/v1/messages")
        .set("x-api-key", key.trim())
        .set("anthropic-version", "2023-06-01")
        .set("content-type", "application/json")
        .timeout(Duration::from_secs(90))
        .send_json(body);
    match resp {
        Ok(r) => {
            let v: serde_json::Value = r.into_json().map_err(|e| e.to_string())?;
            let text: String = v["content"]
                .as_array()
                .map(|parts| {
                    parts
                        .iter()
                        .filter_map(|p| p["text"].as_str())
                        .collect::<Vec<_>>()
                        .join("")
                })
                .unwrap_or_default();
            if text.trim().is_empty() {
                Err("The AI returned an empty answer".into())
            } else {
                Ok(text.trim().to_string())
            }
        }
        Err(ureq::Error::Status(code, r)) => {
            let detail = r.into_string().unwrap_or_default();
            Err(format!("AI request failed ({}): {}", code, detail.chars().take(160).collect::<String>()))
        }
        Err(e) => Err(format!("AI request failed: {}", e)),
    }
}

// ---------- on-device fallbacks ----------

pub fn sentences(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for ch in text.replace('\n', " ").chars() {
        cur.push(ch);
        if matches!(ch, '.' | '?' | '!') {
            let s = cur.trim().to_string();
            if s.len() > 1 {
                out.push(s);
            }
            cur.clear();
        }
    }
    let s = cur.trim().to_string();
    if !s.is_empty() {
        out.push(s);
    }
    out.into_iter()
        .map(|s| s.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|s| !s.is_empty())
        .collect()
}

fn body_without_greeting(body: &str) -> String {
    body.lines()
        .filter(|l| {
            let t = l.trim().to_lowercase();
            !(t.starts_with("hi ") || t.starts_with("hey") || t.starts_with("hello") || t == "thanks," || t == "best," || t.ends_with(',') && t.split_whitespace().count() <= 2)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn summarize(m: &Mail) -> String {
    let core = body_without_greeting(&m.body);
    let sents = sentences(&core);
    let mut out = String::new();
    if let Some(first) = sents.first() {
        out.push_str(first);
    }
    let asks: Vec<&String> = sents
        .iter()
        .skip(1)
        .filter(|s| {
            let l = s.to_lowercase();
            s.ends_with('?') || l.contains("please") || l.contains("could you") || l.contains("can you") || l.contains("by ")
        })
        .take(3)
        .collect();
    if !asks.is_empty() {
        out.push_str("\n\nAsks for you:");
        for a in asks {
            out.push_str("\n- ");
            out.push_str(a);
        }
    } else if let Some(second) = sents.get(1) {
        out.push(' ');
        out.push_str(second);
    }
    out
}

pub fn apply_tone(text: &str, tone: Tone, name: &str) -> String {
    let core = body_without_greeting(text);
    let core = core.trim();
    let first = name.split_whitespace().next().unwrap_or("there");
    match tone {
        Tone::Friendly => format!("Hi {},\n\n{}\n\nThanks so much!", first, core),
        Tone::Neutral => format!("Hi {},\n\n{}\n\nThanks,", first, core),
        Tone::Formal => {
            let c = core
                .replace("can't", "cannot")
                .replace("don't", "do not")
                .replace("won't", "will not")
                .replace("I'm", "I am")
                .replace("it's", "it is")
                .replace("Thanks", "Thank you");
            format!("Dear {},\n\n{}\n\nKind regards,", name, c)
        }
    }
}

pub fn shorten(text: &str) -> String {
    let s = sentences(text);
    let keep = (s.len() + 1) / 2;
    s.into_iter().take(keep.max(1)).collect::<Vec<_>>().join(" ")
}

pub fn expand(text: &str) -> String {
    format!(
        "{}\n\nTo give a bit more context: I want to make sure we're aligned on next steps and timing. If anything here is unclear or you'd like to discuss it live, I'm happy to set up a quick call.",
        text.trim()
    )
}

pub fn proofread(text: &str) -> String {
    let mut out = String::new();
    let mut cap = true;
    let mut prev_space = false;
    for ch in text.chars() {
        if ch == ' ' && prev_space {
            continue;
        }
        prev_space = ch == ' ';
        if cap && ch.is_alphabetic() {
            out.extend(ch.to_uppercase());
            cap = false;
            continue;
        }
        if matches!(ch, '.' | '?' | '!' | '\n') {
            cap = true;
        }
        out.push(ch);
    }
    out.replace(" i ", " I ").replace(" i'", " I'").replace(" ,", ",").replace(" .", ".")
}

pub fn paragraphs(text: &str) -> String {
    sentences(text)
        .chunks(2)
        .map(|c| c.join(" "))
        .collect::<Vec<_>>()
        .join("\n\n")
}

pub fn draft_from_prompt(prompt: &str, to_name: &str) -> String {
    let first = to_name.split_whitespace().next().unwrap_or("there");
    let p = prompt.trim().trim_end_matches('.');
    format!(
        "Hi {},\n\nI wanted to reach out about {}. Let me know your thoughts when you have a moment, and if it helps we can find a time to talk it through.\n\nThanks,",
        first, p
    )
}

pub fn quick_reply(m: &Mail, intent: &str) -> String {
    let first = m.from_name.split_whitespace().next().unwrap_or("there");
    match intent {
        "interested" => format!("Hi {},\n\nThanks for this, sounds good to me. Let's move ahead. What would you need from me for next steps?", first),
        "not_interested" => format!("Hi {},\n\nThanks for thinking of me. I'll pass on this one for now, but I appreciate you reaching out.", first),
        _ => format!("Hi {},\n\nThank you, really appreciate it!", first),
    }
}

pub fn suggest_label(m: &Mail) -> Option<&'static str> {
    let t = format!("{} {}", m.subject, m.body).to_lowercase();
    if ["invoice", "payment", "card", "budget", "bank"].iter().any(|k| t.contains(k)) {
        Some("Finance")
    } else if ["flight", "trip", "offsite", "hotel", "travel"].iter().any(|k| t.contains(k)) {
        Some("Travel")
    } else if ["package", "order", "receipt"].iter().any(|k| t.contains(k)) {
        Some("Receipts")
    } else if ["hike", "dinner", "cat", "weekend", "coffee"].iter().any(|k| t.contains(k)) {
        Some("Personal")
    } else if ["launch", "contract", "api", "build", "hiring", "review", "design"].iter().any(|k| t.contains(k)) {
        Some("Work")
    } else {
        None
    }
}

pub fn copilot_offline(question: &str, mails: &[&Mail]) -> String {
    let words: Vec<String> = question
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.len() > 3 && !["what", "which", "about", "from", "with", "have", "does", "emails", "email", "there", "inbox", "anything", "need"].contains(w))
        .map(|w| w.to_string())
        .collect();
    let q = question.to_lowercase();
    let mut hits: Vec<&&Mail> = if q.contains("unread") || q.contains("need") || q.contains("reply") || q.contains("todo") || q.contains("to do") {
        mails.iter().filter(|m| !m.read && m.category == crate::data::Category::People).collect()
    } else {
        mails
            .iter()
            .filter(|m| {
                let hay = format!("{} {} {}", m.from_name, m.subject, m.body).to_lowercase();
                words.iter().any(|w| hay.contains(w.as_str()))
            })
            .collect()
    };
    hits.truncate(5);
    if hits.is_empty() {
        return "I couldn't find emails about that. Try other words, or add an AI key in Settings for full answers.".into();
    }
    let mut out = format!("I found {} email{}:\n", hits.len(), if hits.len() == 1 { "" } else { "s" });
    for m in hits {
        out.push_str(&format!("\n- {}: \"{}\" - {}", m.from_name, m.subject, sentences(&body_without_greeting(&m.body)).first().cloned().unwrap_or_default()));
    }
    out
}

pub fn mail_context(mails: &[&Mail]) -> String {
    mails
        .iter()
        .take(30)
        .map(|m| {
            format!(
                "From: {} <{}>\nDate: {}\nSubject: {}\n{}\n",
                m.from_name,
                m.from_addr,
                m.received.format("%a %b %-d %-I:%M %p"),
                m.subject,
                m.body.chars().take(400).collect::<String>()
            )
        })
        .collect::<Vec<_>>()
        .join("\n---\n")
}


pub struct CapturedTask {
    pub title: String,
    pub due: Option<String>,
}

pub fn extract_tasks(path: &std::path::Path, today: chrono::NaiveDate) -> Result<Vec<CapturedTask>, String> {
    if !local_model_ready() {
        return Err("The local AI (Ollama) isn't running. Open Ollama and press Cmd+Shift+B again, or add the tasks by hand below.".into());
    }
    let img = crate::snap::encode_for_vision(path)?;
    let prompt = format!(
        "This is a screenshot of my work planner or to-do list. Today is {} ({}). List every task, to-do or work item you can read, one entry each, with a short clear title. \
         If an item shows a due date, deadline, day or time, give it as YYYY-MM-DD or YYYY-MM-DD HH:MM using today's date to resolve words like Monday or tomorrow; otherwise use null. \
         Skip window chrome, menus and anything that is not a task. Answer with JSON only: {{\"tasks\":[{{\"title\":\"...\",\"due\":null}}]}}",
        today.format("%Y-%m-%d"),
        today.format("%A")
    );
    let body = serde_json::json!({
        "model": LOCAL_MODEL,
        "stream": false,
        "think": false,
        "format": "json",
        "keep_alive": "30m",
        "options": { "num_predict": 1500, "num_ctx": 8192, "temperature": 0.1 },
        "messages": [{ "role": "user", "content": prompt, "images": [img] }],
    });
    let v: serde_json::Value = ureq::post(&format!("{}/api/chat", LOCAL_URL))
        .timeout(Duration::from_secs(420))
        .send_json(body)
        .map_err(|e| format!("Local AI failed: {}", e))?
        .into_json()
        .map_err(|e| e.to_string())?;
    let text = v["message"]["content"].as_str().unwrap_or("");
    parse_tasks(text)
}

pub fn parse_tasks(text: &str) -> Result<Vec<CapturedTask>, String> {
    let start = text.find(|c| c == '{' || c == '[').ok_or("The AI did not return any tasks")?;
    let end = text.rfind(|c| c == '}' || c == ']').ok_or("The AI did not return any tasks")?;
    let v: serde_json::Value = serde_json::from_str(&text[start..=end]).map_err(|e| format!("Could not read the AI answer: {}", e))?;
    let arr = if v.is_array() { v.as_array().cloned() } else { v["tasks"].as_array().cloned() }.unwrap_or_default();
    let mut out = Vec::new();
    for t in arr {
        let title = t["title"].as_str().or_else(|| t.as_str()).unwrap_or("").trim().to_string();
        if title.is_empty() || out.iter().any(|o: &CapturedTask| o.title.eq_ignore_ascii_case(&title)) {
            continue;
        }
        let due = t["due"].as_str().map(|s| s.trim().to_string()).filter(|s| !s.is_empty() && !s.eq_ignore_ascii_case("null"));
        out.push(CapturedTask { title, due });
    }
    Ok(out)
}

#[cfg(test)]
mod capture_tests {
    use super::*;

    #[test]
    fn parses_task_json() {
        let t = parse_tasks("```json\n{\"tasks\":[{\"title\":\"Write report\",\"due\":\"2026-11-06\"},{\"title\":\"Call Sam\",\"due\":null},{\"title\":\"write report\"}]}\n```").unwrap();
        assert_eq!(t.len(), 2);
        assert_eq!(t[0].due.as_deref(), Some("2026-11-06"));
        assert!(t[1].due.is_none());
        assert!(parse_tasks("no json here").is_err());
    }
}
