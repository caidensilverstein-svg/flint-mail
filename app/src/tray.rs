// Menu bar (top bar) item for macOS. It only renders a menumodel::Model and
// reports clicks; all content decisions live in menumodel.rs.
use std::sync::mpsc::{channel, Receiver};

use tray_icon::menu::{CheckMenuItem, IsMenuItem, Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem, Submenu};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};

use crate::menumodel::{Do, Icon as MIcon, Model, Row};

pub struct MenuBar {
    tray: TrayIcon,
    menu: Option<Menu>,
    ids: Vec<(MenuId, Do)>,
    rx: Receiver<MenuEvent>,
    icon: MIcon,
    last_title: String,
    last_rows: Vec<Row>,
}

fn draw_icon(state: MIcon) -> Icon {
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
    let dot = |put: &mut dyn FnMut(i32, i32), cx: i32, cy: i32, rad: i32| {
        for y in -rad..=rad {
            for x in -rad..=rad {
                if x * x + y * y <= rad * rad {
                    put(cx + x, cy + y);
                }
            }
        }
    };
    match state {
        MIcon::Plain => {}
        MIcon::Meeting => dot(&mut put, 27, 4, 4),
        MIcon::Unread => dot(&mut put, 27, 4, 3),
        MIcon::Overdue => {
            dot(&mut put, 24, 4, 2);
            dot(&mut put, 29, 4, 2);
        }
    }
    Icon::from_rgba(px, S, S).expect("icon")
}

fn add_rows(rows: &[Row], append: &dyn Fn(&dyn IsMenuItem), ids: &mut Vec<(MenuId, Do)>) {
    for r in rows {
        match r {
            Row::Sep => append(&PredefinedMenuItem::separator()),
            Row::Item { label, enabled, act, checked } => match checked {
                Some(c) => {
                    let it = CheckMenuItem::new(label, *enabled, *c, None);
                    if let Some(a) = act {
                        ids.push((it.id().clone(), a.clone()));
                    }
                    append(&it);
                }
                None => {
                    let it = MenuItem::new(label, *enabled, None);
                    if let Some(a) = act {
                        ids.push((it.id().clone(), a.clone()));
                    }
                    append(&it);
                }
            },
            Row::Sub { label, rows } => {
                let sm = Submenu::new(label, true);
                add_rows(rows, &|i: &dyn IsMenuItem| {
                    let _ = sm.append(i);
                }, ids);
                append(&sm);
            }
        }
    }
}

impl MenuBar {
    pub fn new(ctx: &eframe::egui::Context) -> Result<Self, String> {
        let (tx, rx) = channel();
        let repaint = ctx.clone();
        MenuEvent::set_event_handler(Some(move |e: MenuEvent| {
            let _ = tx.send(e);
            repaint.request_repaint();
        }));
        let menu = Menu::new();
        let quit = MenuItem::new("Quit Flint Mail", true, None);
        let ids = vec![(quit.id().clone(), Do::Quit)];
        menu.append(&quit).map_err(|e| e.to_string())?;
        let tray = TrayIconBuilder::new()
            .with_menu(Box::new(menu.clone()))
            .with_icon(draw_icon(MIcon::Plain))
            .with_icon_as_template(true)
            .with_tooltip("Flint Mail")
            .build()
            .map_err(|e| e.to_string())?;
        Ok(MenuBar { tray, menu: Some(menu), ids, rx, icon: MIcon::Plain, last_title: String::new(), last_rows: Vec::new() })
    }

    pub fn poll(&self) -> Vec<Do> {
        let mut out = Vec::new();
        while let Ok(e) = self.rx.try_recv() {
            if let Some((_, a)) = self.ids.iter().find(|(id, _)| *id == e.id) {
                out.push(a.clone());
            }
        }
        out
    }

    pub fn update(&mut self, model: &Model) {
        if model.title != self.last_title {
            self.tray.set_title(if model.title.is_empty() { None } else { Some(model.title.as_str()) });
            self.last_title = model.title.clone();
        }
        if model.icon != self.icon {
            let _ = self.tray.set_icon(Some(draw_icon(model.icon)));
            self.tray.set_icon_as_template(true);
            self.icon = model.icon;
        }
        if model.rows != self.last_rows {
            let menu = Menu::new();
            let mut ids = Vec::new();
            add_rows(&model.rows, &|i: &dyn IsMenuItem| {
                let _ = menu.append(i);
            }, &mut ids);
            self.tray.set_menu(Some(Box::new(menu.clone())));
            self.menu = Some(menu);
            self.ids = ids;
            self.last_rows = model.rows.clone();
        }
    }
}
