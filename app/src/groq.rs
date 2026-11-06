// Groq provider (OpenAI-compatible chat completions), plain and streaming.
use std::io::{BufRead, BufReader};
use std::time::Duration;

const URL: &str = "https://api.groq.com/openai/v1/chat/completions";

pub const DEFAULT_MODEL: &str = "llama-3.3-70b-versatile";
pub const DEFAULT_FAST_MODEL: &str = "llama-3.1-8b-instant";

#[derive(Debug, PartialEq)]
pub enum Sse {
    Chunk(String),
    Done,
    Skip,
}

pub fn parse_sse_line(line: &str) -> Sse {
    let Some(data) = line.trim().strip_prefix("data:") else { return Sse::Skip };
    let data = data.trim();
    if data == "[DONE]" {
        return Sse::Done;
    }
    match serde_json::from_str::<serde_json::Value>(data) {
        Ok(v) => match v["choices"][0]["delta"]["content"].as_str() {
            Some(t) if !t.is_empty() => Sse::Chunk(t.to_string()),
            _ => Sse::Skip,
        },
        Err(_) => Sse::Skip,
    }
}

pub fn build_body(model: &str, system: &str, prompt: &str, max_tokens: u32, stream: bool, json: bool) -> serde_json::Value {
    let mut body = serde_json::json!({
        "model": model,
        "max_tokens": max_tokens,
        "stream": stream,
        "temperature": if json { 0.1 } else { 0.5 },
        "messages": [
            { "role": "system", "content": system },
            { "role": "user", "content": prompt },
        ],
    });
    if json {
        body["response_format"] = serde_json::json!({ "type": "json_object" });
    }
    body
}

fn describe(e: ureq::Error) -> String {
    match e {
        ureq::Error::Status(code, r) => {
            let detail = r.into_string().unwrap_or_default();
            let msg = serde_json::from_str::<serde_json::Value>(&detail)
                .ok()
                .and_then(|v| v["error"]["message"].as_str().map(|s| s.to_string()))
                .unwrap_or_else(|| detail.chars().take(160).collect());
            format!("Groq request failed ({}): {}", code, msg)
        }
        other => format!("Groq request failed: {}", other),
    }
}

pub fn chat(key: &str, model: &str, system: &str, prompt: &str, max_tokens: u32, json: bool) -> Result<String, String> {
    let body = build_body(model, system, prompt, max_tokens, false, json);
    let resp = ureq::post(URL)
        .set("authorization", &format!("Bearer {}", key.trim()))
        .timeout(Duration::from_secs(40))
        .send_json(body)
        .map_err(describe)?;
    let v: serde_json::Value = resp.into_json().map_err(|e| e.to_string())?;
    let text = v["choices"][0]["message"]["content"].as_str().unwrap_or("").trim().to_string();
    if text.is_empty() {
        Err("Groq returned an empty answer".into())
    } else {
        Ok(text)
    }
}

// Calls on_chunk with each new piece; returns the full text. On a mid-stream
// failure the text received so far is returned in the Err so callers can keep it.
pub fn stream(key: &str, model: &str, system: &str, prompt: &str, max_tokens: u32, mut on_chunk: impl FnMut(&str)) -> Result<String, (String, String)> {
    let body = build_body(model, system, prompt, max_tokens, true, false);
    let resp = ureq::post(URL)
        .set("authorization", &format!("Bearer {}", key.trim()))
        .timeout(Duration::from_secs(90))
        .send_json(body)
        .map_err(|e| (describe(e), String::new()))?;
    let reader = BufReader::new(resp.into_reader());
    let mut full = String::new();
    for line in reader.lines() {
        let line = match line {
            Ok(l) => l,
            Err(e) => return Err((format!("Groq stream interrupted: {}", e), full)),
        };
        match parse_sse_line(&line) {
            Sse::Chunk(t) => {
                full.push_str(&t);
                on_chunk(&full);
            }
            Sse::Done => break,
            Sse::Skip => {}
        }
    }
    if full.trim().is_empty() {
        Err(("Groq returned an empty answer".into(), String::new()))
    } else {
        Ok(full.trim().to_string())
    }
}

pub fn test(key: &str, model: &str) -> Result<String, String> {
    chat(key, model, "Reply with the single word ok.", "ping", 8, false).map(|_| format!("Connected. Model {} responded.", model))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_chunks_done_and_noise() {
        assert_eq!(parse_sse_line(r#"data: {"choices":[{"delta":{"content":"Hel"}}]}"#), Sse::Chunk("Hel".into()));
        assert_eq!(parse_sse_line("data: [DONE]"), Sse::Done);
        assert_eq!(parse_sse_line(""), Sse::Skip);
        assert_eq!(parse_sse_line(": keep-alive"), Sse::Skip);
        assert_eq!(parse_sse_line(r#"data: {"choices":[{"delta":{"role":"assistant"}}]}"#), Sse::Skip);
        assert_eq!(parse_sse_line("data: {not json"), Sse::Skip);
    }

    #[test]
    fn body_has_json_mode_only_when_asked() {
        let a = build_body("m", "s", "p", 100, false, true);
        assert_eq!(a["response_format"]["type"], "json_object");
        let b = build_body("m", "s", "p", 100, true, false);
        assert!(b.get("response_format").is_none());
        assert_eq!(b["stream"], true);
    }
}
