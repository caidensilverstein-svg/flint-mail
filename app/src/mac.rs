// Mac desktop integrations: notifications, Keychain, Reminders, Notes and
// apps that accept links (Things, OmniFocus, Todoist, Bear).
use std::process::Command;

static OSA_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

const KEYCHAIN_SERVICE: &str = "Flint Mail";

pub fn encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}

fn applescript_quote(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

pub fn open_url(url: &str) -> Result<(), String> {
    let status = Command::new("open").arg(url).status().map_err(|e| e.to_string())?;
    if status.success() { Ok(()) } else { Err(format!("Could not open {}", url.split(':').next().unwrap_or(url))) }
}

fn osascript(script: &str) -> Result<(), String> {
    let out = Command::new("osascript").arg("-e").arg(script).output().map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

pub fn notify(title: &str, body: &str) {
    let script = format!(
        "display notification {} with title {}",
        applescript_quote(body),
        applescript_quote(title)
    );
    std::thread::spawn(move || {
        let _ = osascript(&script);
    });
}

pub fn keychain_set(account: &str, secret: &str) -> Result<(), String> {
    let out = Command::new("security")
        .args(["add-generic-password", "-U", "-a", account, "-s", KEYCHAIN_SERVICE, "-w", secret])
        .output()
        .map_err(|e| e.to_string())?;
    if out.status.success() { Ok(()) } else { Err(String::from_utf8_lossy(&out.stderr).trim().to_string()) }
}

pub fn keychain_get(account: &str) -> Option<String> {
    let out = Command::new("security")
        .args(["find-generic-password", "-a", account, "-s", KEYCHAIN_SERVICE, "-w"])
        .output()
        .ok()?;
    if out.status.success() {
        let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
        (!s.is_empty()).then_some(s)
    } else {
        None
    }
}

pub fn keychain_delete(account: &str) {
    let _ = Command::new("security")
        .args(["delete-generic-password", "-a", account, "-s", KEYCHAIN_SERVICE])
        .output();
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Integration {
    Reminders,
    Notes,
    Calendar,
    Things,
    OmniFocus,
    Todoist,
    Bear,
    TickTick,
    Notion,
    Trello,
    Asana,
    Evernote,
    Slack,
    Teams,
    Zoom,
    OneNote,
    GoogleDrive,
    Dropbox,
    HubSpot,
    MeisterTask,
    Agenda,
    MicrosoftToDo,
}

impl Integration {
    pub const ALL: [Integration; 22] = [
        Integration::Reminders,
        Integration::Notes,
        Integration::Calendar,
        Integration::Things,
        Integration::OmniFocus,
        Integration::Todoist,
        Integration::Bear,
        Integration::Agenda,
        Integration::TickTick,
        Integration::Notion,
        Integration::Trello,
        Integration::Asana,
        Integration::Evernote,
        Integration::MicrosoftToDo,
        Integration::OneNote,
        Integration::MeisterTask,
        Integration::Slack,
        Integration::Teams,
        Integration::Zoom,
        Integration::GoogleDrive,
        Integration::Dropbox,
        Integration::HubSpot,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Integration::Reminders => "Reminders",
            Integration::Notes => "Notes",
            Integration::Calendar => "Calendar (Mac)",
            Integration::Things => "Things",
            Integration::OmniFocus => "OmniFocus",
            Integration::Todoist => "Todoist",
            Integration::Bear => "Bear",
            Integration::TickTick => "TickTick",
            Integration::Notion => "Notion",
            Integration::Trello => "Trello",
            Integration::Asana => "Asana",
            Integration::Evernote => "Evernote",
            Integration::Slack => "Slack",
            Integration::Teams => "Microsoft Teams",
            Integration::Zoom => "Zoom",
            Integration::OneNote => "OneNote",
            Integration::GoogleDrive => "Google Drive",
            Integration::Dropbox => "Dropbox",
            Integration::HubSpot => "HubSpot",
            Integration::MeisterTask => "MeisterTask",
            Integration::Agenda => "Agenda",
            Integration::MicrosoftToDo => "Microsoft To Do",
        }
    }

    /// True when the export goes straight into an app on this Mac.
    pub fn native(self) -> bool {
        matches!(
            self,
            Integration::Reminders
                | Integration::Notes
                | Integration::Calendar
                | Integration::Things
                | Integration::OmniFocus
                | Integration::Todoist
                | Integration::Bear
                | Integration::Agenda
        )
    }

    pub fn how(self) -> &'static str {
        if self.native() {
            "Sends straight to the app on this Mac"
        } else {
            "Copies the email and opens the service in your browser"
        }
    }

    fn web(self) -> &'static str {
        match self {
            Integration::TickTick => "https://ticktick.com/webapp",
            Integration::Notion => "https://www.notion.so",
            Integration::Trello => "https://trello.com",
            Integration::Asana => "https://app.asana.com",
            Integration::Evernote => "https://www.evernote.com/client/web",
            Integration::Slack => "https://app.slack.com",
            Integration::Teams => "https://teams.microsoft.com",
            Integration::Zoom => "https://zoom.us/meeting/schedule",
            Integration::OneNote => "https://www.onenote.com/notebooks",
            Integration::GoogleDrive => "https://drive.google.com",
            Integration::Dropbox => "https://www.dropbox.com/home",
            Integration::HubSpot => "https://app.hubspot.com",
            Integration::MeisterTask => "https://www.meistertask.com/app",
            Integration::MicrosoftToDo => "https://to-do.office.com",
            _ => "",
        }
    }

    /// Sends a title and text. Returns a short status line, and whether the
    /// caller should also put the text on the clipboard.
    pub fn export(self, title: &str, text: &str) -> Result<(String, bool), String> {
        let short: String = text.chars().take(1500).collect();
        match self {
            Integration::Reminders => {
                osascript(&format!(
                    "tell application \"Reminders\" to make new reminder with properties {{name:{}, body:{}}}",
                    applescript_quote(title),
                    applescript_quote(&short)
                ))?;
                Ok(("Added to Reminders".into(), false))
            }
            Integration::Notes => {
                let html = format!("<h1>{}</h1><p>{}</p>", title, short.replace('\n', "<br>"));
                osascript(&format!(
                    "tell application \"Notes\" to make new note with properties {{body:{}}}",
                    applescript_quote(&html)
                ))?;
                Ok(("Saved to Notes".into(), false))
            }
            Integration::Calendar => {
                osascript(&format!(
                    "tell application \"Calendar\"\n set d to (current date) + 3600\n tell (first calendar whose writable is true) to make new event with properties {{summary:{}, start date:d, end date:d + 1800, description:{}}}\nend tell",
                    applescript_quote(title),
                    applescript_quote(&short)
                ))?;
                Ok(("Added to Calendar in one hour".into(), false))
            }
            Integration::Things => {
                open_url(&format!("things:///add?title={}&notes={}", encode(title), encode(&short)))?;
                Ok(("Sent to Things".into(), false))
            }
            Integration::OmniFocus => {
                open_url(&format!("omnifocus:///add?name={}&note={}", encode(title), encode(&short)))?;
                Ok(("Sent to OmniFocus".into(), false))
            }
            Integration::Todoist => {
                open_url(&format!("todoist://addtask?content={}&description={}", encode(title), encode(&short)))?;
                Ok(("Sent to Todoist".into(), false))
            }
            Integration::Bear => {
                open_url(&format!("bear://x-callback-url/create?title={}&text={}", encode(title), encode(&short)))?;
                Ok(("Sent to Bear".into(), false))
            }
            Integration::Agenda => {
                open_url(&format!("agenda://x-callback-url/create-note?title={}&text={}", encode(title), encode(&short)))?;
                Ok(("Sent to Agenda".into(), false))
            }
            other => {
                open_url(other.web())?;
                Ok((format!("Copied the email and opened {}", other.name()), true))
            }
        }
    }
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct MacEvent {
    pub calendar: String,
    pub title: String,
    pub start: chrono::DateTime<chrono::Local>,
    pub end: chrono::DateTime<chrono::Local>,
    pub location: String,
    pub all_day: bool,
}

// Reads the Mac Calendar app (which includes Google/iCloud calendars added in
// System Settings > Internet Accounts). Repeating events show only their first date.
pub fn calendar_events() -> Result<(Vec<String>, Vec<MacEvent>), String> {
    use chrono::{Duration, Local, TimeZone};
    let names_out = Command::new("osascript")
        .arg("-e")
        .arg("set fs to ASCII character 31\nset out to \"\"\ntell application \"Calendar\"\nrepeat with c in calendars\nset out to out & (name of c) & fs\nend repeat\nend tell\nreturn out")
        .output()
        .map_err(|e| e.to_string())?;
    if !names_out.status.success() {
        return Err(String::from_utf8_lossy(&names_out.stderr).trim().chars().take(200).collect());
    }
    let names: Vec<String> = String::from_utf8_lossy(&names_out.stdout)
        .trim()
        .split('\u{1f}')
        .map(|n| n.trim().to_string())
        .filter(|n| !n.is_empty())
        .collect();
    let script = r#"
on run argv
	set cn to item 1 of argv
	set d1 to (current date)
	set time of d1 to 0
	set d1 to d1 - 7 * days
	set d2 to d1 + 45 * days
	set fs to ASCII character 31
	set rs to ASCII character 30
	set out to ""
	with timeout of 100 seconds
		tell application "Calendar"
			set evs to (every event of calendar cn whose start date is greater than or equal to d1 and start date is less than d2)
			set n to count of evs
			if n > 0 then
				set ts to summary of evs
				set ss to start date of evs
				set es to end date of evs
				set ls to location of evs
				set ads to allday event of evs
				repeat with i from 1 to n
					set s to item i of ss
					set en to item i of es
					set loc to item i of ls
					if loc is missing value then set loc to ""
					set ad to "0"
					if item i of ads then set ad to "1"
					set out to out & "E" & fs & cn & fs & (item i of ts) & fs & (year of s) & "-" & ((month of s) as integer) & "-" & (day of s) & "-" & (time of s) & fs & (year of en) & "-" & ((month of en) as integer) & "-" & (day of en) & "-" & (time of en) & fs & loc & fs & ad & rs
				end repeat
			end if
		end tell
	end timeout
	return out
end run
"#;
    let parse = |s: &str| -> Option<chrono::DateTime<Local>> {
        let p: Vec<i64> = s.trim().split('-').filter_map(|x| x.trim().parse().ok()).collect();
        if p.len() != 4 {
            return None;
        }
        Local.with_ymd_and_hms(p[0] as i32, p[1] as u32, p[2] as u32, 0, 0, 0).single().map(|d| d + Duration::seconds(p[3]))
    };
    let handles: Vec<_> = names
        .iter()
        .map(|n| {
            let n = n.clone();
            std::thread::spawn(move || Command::new("osascript").arg("-e").arg(script).arg(&n).output())
        })
        .collect();
    let mut events = Vec::new();
    for h in handles {
        let Ok(Ok(o)) = h.join() else { continue };
        if !o.status.success() {
            continue;
        }
        let text = String::from_utf8_lossy(&o.stdout).to_string();
        for rec in text.split('\u{1e}') {
            let f: Vec<&str> = rec.trim_matches(|c| c == '\n' || c == '\r').split('\u{1f}').collect();
            if f.first() == Some(&"E") && f.len() >= 6 {
                if let (Some(start), Some(end)) = (parse(f[3]), parse(f[4])) {
                    events.push(MacEvent { calendar: f[1].into(), title: f[2].into(), start, end, location: f[5].into(), all_day: f.get(6) == Some(&"1") });
                }
            }
        }
    }
    Ok((names, events))
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct Task {
    pub id: String,
    pub list: String,
    pub title: String,
    pub due: Option<chrono::DateTime<chrono::Local>>,
    pub flagged: bool,
}

pub fn reminders() -> Result<Vec<Task>, String> {
    use chrono::{Duration, Local, TimeZone};
    let script = r#"
set fs to ASCII character 31
set rs to ASCII character 30
set out to ""
with timeout of 90 seconds
tell application "Reminders"
	repeat with l in lists
		set ln to name of l
		try
			repeat with r in (reminders of l whose completed is false)
				set t to name of r
				set i to id of r
				set dd to ""
				try
					set d to due date of r
					if d is not missing value then set dd to (year of d) & "-" & ((month of d) as integer) & "-" & (day of d) & "-" & (time of d)
				end try
				set fl to "0"
				try
					if flagged of r then set fl to "1"
				end try
				set out to out & i & fs & ln & fs & t & fs & dd & fs & fl & rs
			end repeat
		end try
	end repeat
end tell
end timeout
return out
"#;
    let _guard = OSA_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let o = Command::new("osascript").arg("-e").arg(script).output().map_err(|e| e.to_string())?;
    if !o.status.success() {
        return Err(String::from_utf8_lossy(&o.stderr).trim().chars().take(200).collect());
    }
    let text = String::from_utf8_lossy(&o.stdout).to_string();
    let parse = |s: &str| -> Option<chrono::DateTime<Local>> {
        let p: Vec<i64> = s.trim().split('-').filter_map(|x| x.trim().parse().ok()).collect();
        if p.len() != 4 {
            return None;
        }
        Local.with_ymd_and_hms(p[0] as i32, p[1] as u32, p[2] as u32, 0, 0, 0).single().map(|d| d + Duration::seconds(p[3]))
    };
    let mut out = Vec::new();
    for rec in text.split('\u{1e}') {
        let f: Vec<&str> = rec.trim_matches(|c| c == '\n' || c == '\r').split('\u{1f}').collect();
        if f.len() >= 5 && !f[0].is_empty() {
            out.push(Task { id: f[0].into(), list: f[1].into(), title: f[2].into(), due: parse(f[3]), flagged: f[4] == "1" });
        }
    }
    Ok(out)
}

fn run_helper(args: &[String]) -> bool {
    if let Some(app) = helper_app() {
        return Command::new("open")
            .args(["-W", "-g", "-j", "-a"])
            .arg(&app)
            .arg("--args")
            .args(args)
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
    }
    match helper_path() {
        Some(p) => Command::new(p).args(args).output().map(|o| o.status.success()).unwrap_or(false),
        None => false,
    }
}

pub fn add_event(calendar: &str, title: &str, start: chrono::DateTime<chrono::Local>, end: chrono::DateTime<chrono::Local>, location: &str, link: &str) -> bool {
    run_helper(&["addevent".to_string(), calendar.to_string(), title.to_string(), start.timestamp().to_string(), end.timestamp().to_string(), location.to_string(), link.to_string()])
}

pub fn update_event(calendar: &str, old_title: &str, old_start: chrono::DateTime<chrono::Local>, title: &str, start: chrono::DateTime<chrono::Local>, end: chrono::DateTime<chrono::Local>, location: &str, link: &str) -> bool {
    run_helper(&[
        "updateevent".to_string(),
        calendar.to_string(),
        old_title.to_string(),
        old_start.timestamp().to_string(),
        title.to_string(),
        start.timestamp().to_string(),
        end.timestamp().to_string(),
        location.to_string(),
        link.to_string(),
    ])
}

pub fn delete_event(calendar: &str, title: &str, start: chrono::DateTime<chrono::Local>) -> bool {
    run_helper(&["deleteevent".to_string(), calendar.to_string(), title.to_string(), start.timestamp().to_string()])
}

pub fn complete_reminder(id: &str) {
    if has_helper() {
        run_helper(&["complete".to_string(), id.to_string()]);
        return;
    }
    let script = format!("tell application \"Reminders\" to set completed of (first reminder whose id is \"{}\") to true", id.replace('"', ""));
    let _ = Command::new("osascript").arg("-e").arg(script).output();
}

#[derive(Default, serde::Serialize, serde::Deserialize)]
pub struct PlannerCache {
    pub names: Vec<String>,
    pub events: Vec<MacEvent>,
    pub tasks: Vec<Task>,
}

fn planner_cache_path() -> Option<std::path::PathBuf> {
    dirs::config_dir().map(|d| d.join("Flint Mail").join("planner-cache.json"))
}

pub fn load_planner_cache() -> PlannerCache {
    planner_cache_path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save_planner_cache(c: &PlannerCache) {
    if let Some(p) = planner_cache_path() {
        if let Ok(s) = serde_json::to_string(c) {
            let _ = std::fs::write(p, s);
        }
    }
}

pub struct PlannerData {
    pub undetermined: bool,
    pub names: Vec<String>,
    pub events: Vec<MacEvent>,
    pub tasks: Vec<Task>,
    pub errors: Vec<String>,
}

fn helper_path() -> Option<std::path::PathBuf> {
    let p = std::env::current_exe().ok()?.parent()?.join("flint-cal");
    p.exists().then_some(p)
}

pub fn has_helper() -> bool {
    helper_app().is_some() || helper_path().is_some()
}

fn helper_app() -> Option<std::path::PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let app = exe.ancestors().nth(3)?;
    let p = app.parent()?.join("Flint Calendar Helper.app");
    p.exists().then_some(p)
}

static LAST_PROMPT: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(0);

pub fn planner() -> Result<PlannerData, String> {
    let first = planner_once()?;
    let undetermined = helper_app().is_some() && std::path::Path::new("/tmp/flint-planner-fixture.json").exists() == false && first.undetermined;
    if undetermined {
        let now = chrono::Local::now().timestamp();
        let last = LAST_PROMPT.load(std::sync::atomic::Ordering::SeqCst);
        if now - last > 3600 {
            LAST_PROMPT.store(now, std::sync::atomic::Ordering::SeqCst);
            grant_access_blocking();
            return planner_once();
        }
    }
    Ok(first)
}

fn planner_once() -> Result<PlannerData, String> {
    use chrono::{Local, TimeZone};
    let _guard = OSA_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let stdout = if let Ok(fixture) = std::fs::read("/tmp/flint-planner-fixture.json") {
        fixture
    } else if let Some(app) = helper_app() {
        let out = std::env::temp_dir().join(format!("flint-cal-{}.json", std::process::id()));
        let _ = std::fs::remove_file(&out);
        let status = Command::new("open").args(["-W", "-g", "-j", "-a"]).arg(&app).arg("--args").arg("--out").arg(&out).status().map_err(|e| e.to_string())?;
        if !status.success() {
            return Err("calendar helper could not start".into());
        }
        let data = std::fs::read(&out).map_err(|_| "calendar helper returned nothing".to_string())?;
        let _ = std::fs::remove_file(&out);
        data
    } else {
        let path = helper_path().ok_or("calendar helper missing")?;
        Command::new(path).output().map_err(|e| e.to_string())?.stdout
    };
    let v: serde_json::Value = serde_json::from_slice(&stdout).map_err(|e| format!("calendar helper: {}", e))?;
    let ts = |x: &serde_json::Value| x.as_f64().and_then(|t| Local.timestamp_opt(t as i64, 0).single());
    let strs = |x: &serde_json::Value| -> Vec<String> { x.as_array().map(|a| a.iter().filter_map(|s| s.as_str().map(String::from)).collect()).unwrap_or_default() };
    let mut events = Vec::new();
    for e in v["events"].as_array().cloned().unwrap_or_default() {
        if let (Some(start), Some(end)) = (ts(&e["start"]), ts(&e["end"])) {
            events.push(MacEvent {
                calendar: e["calendar"].as_str().unwrap_or("").into(),
                title: e["title"].as_str().unwrap_or("").into(),
                start,
                end,
                location: e["location"].as_str().unwrap_or("").into(),
                all_day: e["all_day"].as_bool().unwrap_or(false),
            });
        }
    }
    let mut tasks = Vec::new();
    for t in v["tasks"].as_array().cloned().unwrap_or_default() {
        tasks.push(Task {
            id: t["id"].as_str().unwrap_or("").into(),
            list: t["list"].as_str().unwrap_or("").into(),
            title: t["title"].as_str().unwrap_or("").into(),
            due: ts(&t["due"]),
            flagged: t["flagged"].as_bool().unwrap_or(false),
        });
    }
    let undetermined = v["status"]["calendar"].as_i64() == Some(0) || v["status"]["reminders"].as_i64() == Some(0);
    Ok(PlannerData { undetermined, names: strs(&v["names"]), events, tasks, errors: strs(&v["errors"]) })
}

pub fn add_reminder(title: &str, due: Option<chrono::DateTime<chrono::Local>>) -> bool {
    let mut args = vec!["add".to_string(), title.to_string()];
    if let Some(d) = due {
        args.push(d.timestamp().to_string());
    }
    run_helper(&args)
}

fn login_plist_path() -> Option<std::path::PathBuf> {
    dirs::home_dir().map(|h| h.join("Library/LaunchAgents/local.flintmail.login.plist"))
}

pub fn login_item_enabled() -> bool {
    login_plist_path().map(|p| p.exists()).unwrap_or(false)
}

pub fn set_login_item(on: bool) -> Result<(), String> {
    let path = login_plist_path().ok_or("no home folder")?;
    if !on {
        let _ = std::fs::remove_file(&path);
        return Ok(());
    }
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let app = exe.ancestors().nth(3).ok_or("not running from an app bundle")?.to_path_buf();
    if app.extension().and_then(|e| e.to_str()) != Some("app") {
        return Err("Open Flint Mail.app to turn this on".into());
    }
    let plist = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n<plist version=\"1.0\"><dict>\n<key>Label</key><string>local.flintmail.login</string>\n<key>ProgramArguments</key><array><string>/usr/bin/open</string><string>-a</string><string>{}</string></array>\n<key>RunAtLoad</key><true/>\n</dict></plist>\n",
        app.display()
    );
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    std::fs::write(&path, plist).map_err(|e| e.to_string())
}

pub fn grant_access_blocking() {
    if let Some(app) = helper_app() {
        let out = std::env::temp_dir().join("flint-cal-grant.json");
        let _ = Command::new("open").args(["-W", "-a"]).arg(&app).arg("--args").arg("--interactive").arg("--out").arg(&out).status();
    }
}
