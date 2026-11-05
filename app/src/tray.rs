// Menu bar (top bar) item for macOS: unread count and next-meeting countdown
// in the title, an agenda and quick actions in its menu.
use std::sync::mpsc::{channel, Receiver};

use tray_icon::menu::{Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TrayAction {
    Open,
    Agenda,
    NewEmail,
    Calendar,
    Snapshot,
    AskAi,
    Settings,
    Quit,
    Event(usize),
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum IconState {
    Default,
    MeetingInProgress,
}

pub struct MenuBar {
    tray: TrayIcon,
    status: MenuItem,
    agenda: Vec<MenuItem>,
    ids: Vec<(MenuId, TrayAction)>,
    rx: Receiver<MenuEvent>,
    state: IconState,
    last_title: String,
}

fn draw_icon(state: IconState) -> Icon {
    const S: u32 = 32;
    let mut px = vec![0u8; (S * S * 4) as usize];
    let mut put = |x: i32, y: i32| {
        if x >= 0 && y >= 0 && (x as u32) < S && (y as u32) < S {
            let i = ((y as u32 * S + x as u32) * 4) as usize;
            px[i + 3] = 255;
        }
    };
    let (l, r, t, b) = (3, 28, 8, 24);
    for w in 0..2 {
        for x in l..=r {
            put(x, t + w);
            put(x, b - w);
        }
        for y in t..=b {
            put(l + w, y);
            put(r - w, y);
        }
    }
    // flap of the envelope
    for i in 0..=12 {
        for w in 0..2 {
            put(l + i + w, t + (i * 8) / 12);
            put(r - i - w, t + (i * 8) / 12);
        }
    }
    if state == IconState::MeetingInProgress {
        for y in 0..9 {
            for x in 0..9 {
                let (dx, dy) = (x - 4, y - 4);
                if dx * dx + dy * dy <= 16 {
                    put(23 + x, y);
                }
            }
        }
    }
    Icon::from_rgba(px, S, S).expect("icon")
}

impl MenuBar {
    pub fn new(ctx: &eframe::egui::Context) -> Result<Self, String> {
        let menu = Menu::new();
        let status = MenuItem::new("Inbox up to date", false, None);
        let mut agenda = Vec::new();
        let mut ids = Vec::new();
        menu.append(&status).map_err(|e| e.to_string())?;
        menu.append(&PredefinedMenuItem::separator()).map_err(|e| e.to_string())?;
        for i in 0..3 {
            let item = MenuItem::new("", false, None);
            ids.push((item.id().clone(), TrayAction::Event(i)));
            menu.append(&item).map_err(|e| e.to_string())?;
            agenda.push(item);
        }
        menu.append(&PredefinedMenuItem::separator()).map_err(|e| e.to_string())?;
        let entries = [
            ("Open Flint Mail", TrayAction::Open),
            ("Open Agenda", TrayAction::Agenda),
            ("New Email", TrayAction::NewEmail),
            ("Open Calendar", TrayAction::Calendar),
            ("Snapshot My Planner (Cmd+Shift+B)", TrayAction::Snapshot),
            ("Ask AI About My Inbox", TrayAction::AskAi),
            ("Settings...", TrayAction::Settings),
        ];
        for (label, act) in entries {
            let item = MenuItem::new(label, true, None);
            ids.push((item.id().clone(), act));
            menu.append(&item).map_err(|e| e.to_string())?;
            std::mem::forget(item);
        }
        menu.append(&PredefinedMenuItem::separator()).map_err(|e| e.to_string())?;
        let quit = MenuItem::new("Quit Flint Mail", true, None);
        ids.push((quit.id().clone(), TrayAction::Quit));
        menu.append(&quit).map_err(|e| e.to_string())?;
        std::mem::forget(quit);

        let (tx, rx) = channel();
        let repaint = ctx.clone();
        MenuEvent::set_event_handler(Some(move |e: MenuEvent| {
            let _ = tx.send(e);
            repaint.request_repaint();
        }));

        let tray = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_icon(draw_icon(IconState::Default))
            .with_icon_as_template(true)
            .with_tooltip("Flint Mail")
            .build()
            .map_err(|e| e.to_string())?;
        Ok(MenuBar { tray, status, agenda, ids, rx, state: IconState::Default, last_title: String::new() })
    }

    pub fn poll(&self) -> Vec<TrayAction> {
        let mut out = Vec::new();
        while let Ok(e) = self.rx.try_recv() {
            if let Some((_, a)) = self.ids.iter().find(|(id, _)| *id == e.id) {
                out.push(*a);
            }
        }
        out
    }

    pub fn update(&mut self, title: &str, status: &str, agenda: &[String], state: IconState) {
        if title != self.last_title {
            #[cfg(debug_assertions)]
            eprintln!("menu bar title: {:?}, status: {:?}, agenda: {:?}", title, status, agenda);
            self.tray.set_title(if title.is_empty() { None } else { Some(title) });
            self.last_title = title.to_string();
        }
        self.status.set_text(status);
        for (i, item) in self.agenda.iter().enumerate() {
            match agenda.get(i) {
                Some(t) => {
                    item.set_text(t);
                    item.set_enabled(true);
                }
                None => {
                    item.set_text(if i == 0 { "No more events today" } else { "" });
                    item.set_enabled(false);
                }
            }
        }
        if state != self.state {
            let _ = self.tray.set_icon(Some(draw_icon(state)));
            self.tray.set_icon_as_template(true);
            self.state = state;
        }
    }
}
