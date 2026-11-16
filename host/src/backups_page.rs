//! Backups: how much room they take (a bar by server, the drive's free space), then every backup by day, newest
//! first, each with Restore and its folder. Back up now for one server or every running one.
use crate::app::HostApp;
use crate::pages::{self, panel_title};
use crate::ui::{self, px, white, Kind};
use gpui::{div, prelude::*, ClickEvent, Context, Div, FontWeight, SharedString};
use horizon_launcher::servers::Backup;
use horizon_launcher::stats;
use horizon_theme::glass::{ink, radius};
use std::path::PathBuf;

/// One colour per server, in the theme's family.
const COLORS: [u32; 8] = [0x6b4bff, 0x5bb8ff, 0x4ade80, 0xf2c46b, 0xc9bdff, 0xff8a8a, 0x8be3d0, 0xe7a6ff];

fn day_label(t: u64) -> String {
    use chrono::TimeZone;
    let Some(d) = chrono::Local.timestamp_opt(t as i64, 0).single() else { return String::new() };
    let today = chrono::Local::now().date_naive();
    match (today - d.date_naive()).num_days() {
        0 => "Today".into(),
        1 => "Yesterday".into(),
        _ => d.format("%A, %B %-d").to_string(),
    }
}

impl HostApp {
    pub fn backups_page(&mut self, cx: &mut Context<Self>) -> Div {
        let only = self.pages.backups_server.clone().filter(|d| self.list.iter().any(|e| &e.dir == d));
        // Every backup with its server: (server index, server dir, backup).
        let mut all: Vec<(usize, PathBuf, Backup)> = vec![];
        for (i, e) in self.list.iter().enumerate() {
            for b in self.backups.get(&e.dir).cloned().unwrap_or_default() {
                all.push((i, e.dir.clone(), b));
            }
        }
        let total: u64 = all.iter().map(|b| b.2.bytes).sum();
        let newest = all.iter().filter_map(|b| b.2.modified).max();
        let over = match newest {
            Some(t) => format!("{} backups · {} · newest {}", all.len(), ui::bytes(total), {
                let s = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0).saturating_sub(t);
                if s < 120 { "just now".to_string() } else { format!("{} ago", ui::span(s)) }
            }),
            None => format!("{} backups", all.len()),
        };
        let picker = self.server_picker("backups-server", only.clone(), true, cx, |this, d, _| this.pages.backups_server = d);
        let head = pages::header(over, "Backups", Some(div().flex().items_center().gap(px(10.)).child(ui::text(13., FontWeight::NORMAL, ink::QUIET).child("Server")).child(picker)));
        // Storage: a bar by server and the drive's room.
        // The chosen server's folder; with all servers, the one holding the most backups.
        let most = self.list.iter().max_by_key(|e| self.backups.get(&e.dir).map(|b| b.len()).unwrap_or(0)).map(|e| e.dir.clone());
        let folder = only.clone().or(most).or_else(|| self.first_server()).map(|d| d.join("backups"));
        let free = folder.as_ref().and_then(|f| stats::disk_free(f.parent().unwrap_or(f)));
        let mut bar = div().flex().gap(px(2.)).h(px(10.)).w_full().rounded(px(5.)).overflow_hidden().bg(white(0.07));
        let mut legend = div().flex().flex_wrap().gap_x(px(18.)).gap_y(px(6.)).mt(px(10.));
        let room = (total + free.unwrap_or(0)).max(1);
        for (i, e) in self.list.iter().enumerate() {
            let list = self.backups.get(&e.dir).cloned().unwrap_or_default();
            let bytes: u64 = list.iter().map(|b| b.bytes).sum();
            if bytes == 0 {
                continue;
            }
            let c = ui::color(COLORS[i % COLORS.len()]);
            bar = bar.child(div().h_full().w(gpui::relative((bytes as f32 / room as f32).max(0.004))).bg(c));
            legend = legend.child(div().flex().items_center().gap(px(7.)).child(div().size(px(9.)).rounded(px(2.)).bg(c)).child(ui::text(12., FontWeight::NORMAL, ink::BODY).child(format!("{} {} · {}", e.name, ui::bytes(bytes), list.len()))));
        }
        let open_folder = folder.clone().map(|f| {
            ui::icon_button("backups-open", "folder-01", "Open folder", Kind::Secondary).on_click(move |_, _, cx| {
                let _ = std::fs::create_dir_all(&f);
                cx.open_with_system(&f);
            })
        });
        let storage = ui::panel()
            .px(px(20.))
            .py(px(18.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(10.))
                    .child(div().flex().items_center().gap(px(10.)).child(ui::heading(17.).child("Storage")).child(ui::mono(12., ink::QUIET).child(folder.as_ref().map(|f| f.display().to_string()).unwrap_or_default())))
                    .child(div().flex().items_center().gap(px(10.)).children(free.map(|f| ui::text(12.5, FontWeight::NORMAL, ink::QUIET).child(format!("{} free on this drive", ui::bytes(f))))).children(open_folder)),
            )
            .child(bar.mt(px(14.)))
            .child(legend);
        // Back up now: the chosen server, or every running one.
        let targets: Vec<PathBuf> = match &only {
            Some(d) => vec![d.clone()],
            None => self.list.iter().filter(|e| self.status_of(&e.dir).running()).map(|e| e.dir.clone()).collect(),
        };
        let now_button = ui::icon_button("backups-now", "download-04", if only.is_some() { "Back up now" } else { "Back up running servers" }, Kind::Secondary).on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
            if targets.is_empty() {
                this.note = Some(("No server is running. Pick one to back it up.".into(), std::time::Instant::now()));
            }
            for d in targets.clone() {
                this.server_action(d, "Backing up", vec!["backup".into()], cx);
            }
        }));
        let mut shown: Vec<&(usize, PathBuf, Backup)> = all.iter().filter(|b| only.as_ref().is_none_or(|d| d == &b.1)).collect();
        shown.sort_by_key(|b| std::cmp::Reverse(b.2.modified.unwrap_or(0)));
        let cols = |a: Div, b: Div, c: Div, d: Div| div().flex().items_center().gap(px(12.)).child(a.flex_1().min_w_0()).child(b.flex_none().w(px(200.))).child(c.flex_none().w(px(100.))).child(d.flex_none().w(px(170.)));
        let mut list = div().flex().flex_col();
        if shown.is_empty() {
            list = list.child(ui::text(13., FontWeight::NORMAL, ink::QUIET).px(px(12.)).py(px(14.)).child("No backups yet. Back up now makes one; Schedules makes them on their own."));
        }
        let mut last_day = String::new();
        for (k, (i, dir, b)) in shown.into_iter().enumerate() {
            let label = b.modified.map(day_label).unwrap_or_else(|| "Earlier".into());
            if label != last_day {
                list = list.child(div().px(px(12.)).pt(px(14.)).pb(px(6.)).child(ui::text(12., FontWeight::SEMIBOLD, ink::BODY).child(label.clone())));
                last_day = label;
            }
            let time = b.modified.map(|t| {
                use chrono::TimeZone;
                chrono::Local.timestamp_opt(t as i64, 0).single().map(|d| d.format("%-I:%M %p").to_string()).unwrap_or_default()
            });
            let running = self.status_of(dir).running();
            let key = format!("restore:{}:{}", dir.display(), b.name);
            let confirming = self.pages.confirm.as_deref() == Some(key.as_str());
            let (d, name, key2) = (dir.clone(), b.name.clone(), key.clone());
            let restore = ui::sized_button(SharedString::from(format!("bk-restore-{k}")), if confirming { "Restore? Press again" } else { "Restore" }, if confirming { Kind::Danger } else { Kind::Secondary }, 30., 12.5).on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                if running {
                    this.note = Some(("Stop the server before restoring a backup.".into(), std::time::Instant::now()));
                } else if this.pages.confirm.as_deref() == Some(key2.as_str()) {
                    this.pages.confirm = None;
                    this.server_action(d.clone(), "Restoring", vec!["restore".into(), name.clone(), "--yes".into()], cx);
                } else {
                    this.pages.confirm = Some(key2.clone());
                }
                cx.notify();
            }));
            let folder = dir.join("backups");
            let open = ui::square(SharedString::from(format!("bk-open-{k}")), "folder-01", 30.).on_click(move |_, _, cx| cx.open_with_system(&folder));
            list = list.child(
                cols(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(10.))
                        .child(ui::icon("download-04", 15., white(ink::QUIET)))
                        .child(ui::mono(12.5, ink::FULL).child(time.unwrap_or_else(|| b.name.clone())))
                        .child(ui::text(11.5, FontWeight::NORMAL, ink::QUIET).truncate().child(b.name.clone())),
                    div().flex().items_center().gap(px(8.)).child(div().flex_none().size(px(8.)).rounded(px(2.)).bg(ui::color(COLORS[*i % COLORS.len()]))).child(ui::text(13., FontWeight::NORMAL, ink::FULL).truncate().child(self.name_of(dir))),
                    ui::mono(12., ink::FULL).child(ui::bytes(b.bytes)),
                    div().flex().items_center().justify_end().gap(px(6.)).child(restore).child(open),
                )
                .h(px(48.))
                .px(px(12.))
                .border_b_1()
                .border_color(white(0.06)),
            );
        }
        let table = ui::panel()
            .px(px(16.))
            .py(px(16.))
            .child(div().flex().items_center().justify_between().px(px(4.)).pb(px(10.)).child(panel_title(if only.is_some() { "This server's backups" } else { "Every backup" }, None)).child(now_button))
            .child(cols(ui::th("BACKUP"), ui::th("SERVER"), ui::th("SIZE"), div()).h(px(32.)).px(px(12.)).border_b_1().border_color(white(0.08)))
            .child(list);
        let _ = radius::ROW;
        pages::page("backups-scroll", head, div().flex().flex_col().gap(px(16.)).child(storage).child(table))
    }
}
