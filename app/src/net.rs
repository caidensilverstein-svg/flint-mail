// Real mail accounts: IMAP to read the inbox, SMTP to send.
use chrono::{Local, TimeZone};
use mailparse::{MailHeaderMap, ParsedMail};

use crate::data::{Account, Category};
use crate::mechanics::IMAP_FETCH_LIMIT;

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct RemoteMail {
    pub uid: u32,
    pub account: String,
    pub from_name: String,
    pub from_addr: String,
    pub to: String,
    pub subject: String,
    pub body: String,
    pub received: chrono::DateTime<Local>,
    pub read: bool,
    pub category: Category,
    pub can_unsubscribe: bool,
    pub attachments: Vec<String>,
}

pub enum NetResult {
    Calendar { names: Vec<String>, events: Vec<crate::mac::MacEvent>, warnings: Vec<String> },
    Synced { account: String, mails: Vec<RemoteMail> },
    Tasks(Result<Vec<crate::mac::Task>, String>),
    RefreshPlanner,
    SyncFailed { account: String, error: String },
    Sent { to: String },
    SendFailed { to: String, error: String },
}

const CACHE_VERSION: u32 = 3;

#[derive(Default, serde::Serialize, serde::Deserialize)]
pub struct MailCache {
    #[serde(default)]
    pub version: u32,
    pub uidvalidity: u32,
    pub mails: Vec<RemoteMail>,
}

fn cache_path(email: &str) -> Option<std::path::PathBuf> {
    let safe: String = email.chars().map(|c| if c.is_ascii_alphanumeric() || c == '@' || c == '.' { c } else { '_' }).collect();
    dirs::config_dir().map(|d| d.join("Flint Mail").join(format!("mail-cache-{}.json", safe)))
}

pub fn load_cache(email: &str) -> MailCache {
    cache_path(email)
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| serde_json::from_str::<MailCache>(&s).ok())
        .filter(|c| c.version == CACHE_VERSION)
        .unwrap_or_default()
}

fn save_cache(email: &str, cache: &MailCache) {
    if let Some(p) = cache_path(email) {
        if let Ok(s) = serde_json::to_string(cache) {
            let _ = std::fs::write(p, s);
        }
    }
}

pub fn fetch_inbox(acc: &Account, password: &str) -> Result<Vec<RemoteMail>, String> {
    let tls = native_tls::TlsConnector::builder().build().map_err(|e| e.to_string())?;
    let client = imap::connect((acc.imap_host.as_str(), acc.imap_port), acc.imap_host.as_str(), &tls)
        .map_err(|e| format!("Could not reach {}: {}", acc.imap_host, e))?;
    let mut session = client
        .login(&acc.username, password)
        .map_err(|(e, _)| format!("Sign-in failed: {}", e))?;
    let mailbox = session.select("INBOX").map_err(|e| e.to_string())?;
    let total = mailbox.exists;
    let validity = mailbox.uid_validity.unwrap_or(0);
    let mut cache = load_cache(&acc.email);
    if cache.uidvalidity != validity || cache.version != CACHE_VERSION {
        cache = MailCache { version: CACHE_VERSION, uidvalidity: validity, mails: Vec::new() };
    }
    let mut out: Vec<RemoteMail> = Vec::new();
    if total > 0 {
        let start = total.saturating_sub(IMAP_FETCH_LIMIT - 1).max(1);
        let heads = session
            .fetch(format!("{}:{}", start, total), "(UID FLAGS)")
            .map_err(|e| e.to_string())?;
        let mut seen: std::collections::HashMap<u32, bool> = std::collections::HashMap::new();
        for f in heads.iter() {
            if let Some(uid) = f.uid {
                seen.insert(uid, f.flags().iter().any(|fl| matches!(fl, imap::types::Flag::Seen)));
            }
        }
        for m in &cache.mails {
            if let Some(read) = seen.get(&m.uid) {
                let mut m = m.clone();
                m.read = *read;
                out.push(m);
            }
        }
        let have: std::collections::HashSet<u32> = out.iter().map(|m| m.uid).collect();
        let mut missing: Vec<u32> = seen.keys().copied().filter(|u| !have.contains(u)).collect();
        missing.sort_unstable();
        if !missing.is_empty() {
            let set = missing.iter().map(|u| u.to_string()).collect::<Vec<_>>().join(",");
            let fetches = session.uid_fetch(set, "(UID FLAGS BODY.PEEK[])").map_err(|e| e.to_string())?;
            for f in fetches.iter() {
                let (Some(uid), Some(raw)) = (f.uid, f.body()) else { continue };
                let read = f.flags().iter().any(|fl| matches!(fl, imap::types::Flag::Seen));
                if let Some(m) = parse_message(acc, uid, raw, read) {
                    out.push(m);
                }
            }
        }
    }
    let _ = session.logout();
    out.sort_by(|a, b| b.received.cmp(&a.received));
    save_cache(&acc.email, &MailCache { version: CACHE_VERSION, uidvalidity: validity, mails: out.clone() });
    Ok(out)
}

pub fn mark_seen(acc: &Account, password: &str, uid: u32) -> Result<(), String> {
    let tls = native_tls::TlsConnector::builder().build().map_err(|e| e.to_string())?;
    let client = imap::connect((acc.imap_host.as_str(), acc.imap_port), acc.imap_host.as_str(), &tls).map_err(|e| e.to_string())?;
    let mut session = client.login(&acc.username, password).map_err(|(e, _)| e.to_string())?;
    session.select("INBOX").map_err(|e| e.to_string())?;
    session.uid_store(uid.to_string(), "+FLAGS (\\Seen)").map_err(|e| e.to_string())?;
    let _ = session.logout();
    Ok(())
}

fn parse_message(acc: &Account, uid: u32, raw: &[u8], read: bool) -> Option<RemoteMail> {
    let parsed = mailparse::parse_mail(raw).ok()?;
    let h = &parsed.headers;
    let from = h.get_first_value("From").unwrap_or_default();
    let (from_name, from_addr) = split_address(&from);
    let subject = h.get_first_value("Subject").unwrap_or_else(|| "(no subject)".into());
    let to = h.get_first_value("To").unwrap_or_default();
    let received = h
        .get_first_value("Date")
        .and_then(|d| mailparse::dateparse(&d).ok())
        .and_then(|ts| Local.timestamp_opt(ts, 0).single())
        .unwrap_or_else(Local::now);
    let unsubscribe = h.get_first_value("List-Unsubscribe").is_some();
    let lower = from_addr.to_lowercase();
    let category = if unsubscribe {
        Category::Newsletter
    } else if ["noreply", "no-reply", "notification", "alerts", "updates", "billing"].iter().any(|k| lower.contains(k)) {
        Category::Notification
    } else {
        Category::People
    };
    let mut attachments = Vec::new();
    collect_attachments(&parsed, &mut attachments);
    Some(RemoteMail {
        uid,
        account: acc.email.clone(),
        from_name,
        from_addr,
        to,
        subject,
        body: plain_text(&parsed).lines().map(decode_entities).collect::<Vec<_>>().join("\n").trim().to_string(),
        received,
        read,
        category,
        can_unsubscribe: unsubscribe,
        attachments,
    })
}

fn split_address(s: &str) -> (String, String) {
    if let (Some(a), Some(b)) = (s.find('<'), s.rfind('>')) {
        let name = s[..a].trim().trim_matches('"').to_string();
        let addr = s[a + 1..b].trim().to_string();
        (if name.is_empty() { addr.clone() } else { name }, addr)
    } else {
        (s.trim().to_string(), s.trim().to_string())
    }
}

fn plain_text(p: &ParsedMail) -> String {
    if p.subparts.is_empty() {
        let body = p.get_body().unwrap_or_default();
        if p.ctype.mimetype == "text/html" {
            return strip_html(&body);
        }
        return body;
    }
    for sp in &p.subparts {
        if sp.ctype.mimetype == "text/plain" {
            return sp.get_body().unwrap_or_default();
        }
    }
    for sp in &p.subparts {
        let t = plain_text(sp);
        if !t.trim().is_empty() {
            return t;
        }
    }
    String::new()
}

fn collect_attachments(p: &ParsedMail, out: &mut Vec<String>) {
    let disp = p.get_content_disposition();
    if let Some(name) = disp.params.get("filename") {
        out.push(name.clone());
    }
    for sp in &p.subparts {
        collect_attachments(sp, out);
    }
}

fn strip_html(s: &str) -> String {
    let mut text = s.to_string();
    for tag in ["style", "script", "head"] {
        loop {
            let lower = text.to_lowercase();
            let Some(start) = lower.find(&format!("<{}", tag)) else { break };
            let end = lower[start..].find(&format!("</{}>", tag)).map(|e| start + e + tag.len() + 3).unwrap_or(text.len());
            text.replace_range(start..end.min(text.len()), " ");
        }
    }
    while let Some(start) = text.find("<!--") {
        let end = text[start..].find("-->").map(|e| start + e + 3).unwrap_or(text.len());
        text.replace_range(start..end, " ");
    }
    let mut out = String::new();
    let mut in_tag = false;
    for ch in text.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => {
                in_tag = false;
                out.push(' ');
            }
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }
    decode_entities(&out)
}

fn decode_entities(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        let tail = &rest[i..];
        if let Some(end) = tail[..tail.len().min(12)].find(';') {
            let ent = &tail[1..end];
            let rep: Option<String> = match ent {
                "nbsp" => Some(" ".into()),
                "amp" => Some("&".into()),
                "lt" => Some("<".into()),
                "gt" => Some(">".into()),
                "quot" => Some("\"".into()),
                "apos" | "#39" => Some("'".into()),
                "zwnj" | "zwj" | "shy" | "zwsp" => Some(String::new()),
                "rsquo" | "lsquo" => Some("'".into()),
                "rdquo" | "ldquo" => Some("\"".into()),
                "ndash" | "mdash" => Some("-".into()),
                "hellip" => Some("...".into()),
                _ if ent.starts_with('#') => {
                    let n = if let Some(h) = ent.strip_prefix("#x").or_else(|| ent.strip_prefix("#X")) { u32::from_str_radix(h, 16).ok() } else { ent[1..].parse::<u32>().ok() };
                    n.and_then(char::from_u32).map(|c| c.to_string())
                }
                _ => None,
            };
            if let Some(r) = rep {
                out.push_str(&r);
                rest = &tail[end + 1..];
                continue;
            }
        }
        out.push('&');
        rest = &tail[1..];
    }
    out.push_str(rest);
    out.chars()
        .filter(|c| !matches!(*c, '\u{200b}' | '\u{200c}' | '\u{200d}' | '\u{00ad}' | '\u{feff}' | '\u{2060}' | '\u{034f}'))
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn send(acc: &Account, password: &str, to: &str, subject: &str, body: &str, files: &[std::path::PathBuf]) -> Result<(), String> {
    use lettre::transport::smtp::authentication::Credentials;
    use lettre::{Message, SmtpTransport, Transport};

    let from = format!("{} <{}>", acc.name, acc.email);
    let mut builder = Message::builder()
        .from(from.parse().map_err(|e| format!("From address: {}", e))?)
        .subject(subject);
    for addr in to.split([',', ';']).map(|a| a.trim()).filter(|a| !a.is_empty()) {
        builder = builder.to(addr.parse().map_err(|e| format!("To address {}: {}", addr, e))?);
    }
    let email = if files.is_empty() {
        builder.body(body.to_string()).map_err(|e| e.to_string())?
    } else {
        use lettre::message::{header::ContentType, Attachment, MultiPart, SinglePart};
        let mut mp = MultiPart::mixed().singlepart(SinglePart::plain(body.to_string()));
        for f in files {
            let bytes = std::fs::read(f).map_err(|e| format!("Could not read {}: {}", f.display(), e))?;
            let name = f.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "attachment".into());
            let ct = if name.ends_with(".png") { ContentType::parse("image/png") } else { ContentType::parse("application/octet-stream") }.map_err(|e| e.to_string())?;
            mp = mp.singlepart(Attachment::new(name).body(bytes, ct));
        }
        builder.multipart(mp).map_err(|e| e.to_string())?
    };
    let creds = Credentials::new(acc.username.clone(), password.to_string());
    let transport = if acc.smtp_port == 465 {
        SmtpTransport::relay(&acc.smtp_host)
    } else {
        SmtpTransport::starttls_relay(&acc.smtp_host)
    }
    .map_err(|e| e.to_string())?
    .port(acc.smtp_port)
    .credentials(creds)
    .build();
    transport.send(&email).map_err(|e| e.to_string())?;
    Ok(())
}

pub struct Provider {
    pub name: &'static str,
    pub imap: (&'static str, u16),
    pub smtp: (&'static str, u16),
}

pub const PROVIDERS: [Provider; 5] = [
    Provider { name: "Gmail", imap: ("imap.gmail.com", 993), smtp: ("smtp.gmail.com", 465) },
    Provider { name: "iCloud", imap: ("imap.mail.me.com", 993), smtp: ("smtp.mail.me.com", 587) },
    Provider { name: "Outlook", imap: ("outlook.office365.com", 993), smtp: ("smtp.office365.com", 587) },
    Provider { name: "Yahoo", imap: ("imap.mail.yahoo.com", 993), smtp: ("smtp.mail.yahoo.com", 465) },
    Provider { name: "Fastmail", imap: ("imap.fastmail.com", 993), smtp: ("smtp.fastmail.com", 465) },
];

#[cfg(test)]
mod live_tests {
    use super::*;

    #[test]
    #[ignore]
    fn mark_seen_on_an_already_read_message_is_a_noop() {
        let dir = dirs::config_dir().unwrap().join("Flint Mail");
        let settings: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(dir.join("settings.json")).unwrap()).unwrap();
        let acc: Account = serde_json::from_value(settings["accounts"][0].clone()).unwrap();
        let cache = load_cache(&acc.email);
        let m = cache.mails.iter().find(|m| m.read).expect("a read message");
        let pw = crate::mac::keychain_get(&acc.email).expect("password in keychain");
        mark_seen(&acc, &pw, m.uid).unwrap();
    }
}
