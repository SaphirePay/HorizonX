//! Schedules: the next 24 hours of restarts and backups on a timeline by server, every rule (when, next, last,
//! remove), and the form for a new one. A server has one backup rule (its `backups.every`) and one restart rule
//! (every 6 or 12 hours up, or daily at an hour: host.json); a new rule takes the place of the old.
use crate::app::{Ev, HostApp};
use crate::frame::server_tone;
use crate::pages::{self, panel_title, ScheduleForm};
use crate::ui::{self, px, white};
use gpui::{div, prelude::*, ClickEvent, Context, Div, FontWeight, SharedString};
use horizon_launcher::{host, servers, stats};
use horizon_theme::glass::{ink, radius};
use std::path::PathBuf;

const BACKUP_EVERY: [(&str, u64); 6] = [("1h", 1), ("3h", 3), ("6h", 6), ("12h", 12), ("24h", 24), ("1d", 24)];
const AT: [(u32, &str); 6] = [(0, "0:00"), (3, "3:00"), (4, "4:00"), (5, "5:00"), (6, "6:00"), (12, "12:00")];

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn hour_name(h: u32) -> String {
    format!("{}:00 {}", if h % 12 == 0 { 12 } else { h % 12 }, if h < 12 { "AM" } else { "PM" })
}

/// Seconds until the next local `hour`:00.
fn until_hour(hour: u32) -> u64 {
    use chrono::Timelike;
    let n = chrono::Local::now();
    let secs_now = n.hour() * 3600 + n.minute() * 60 + n.second();
    let target = hour * 3600;
    (if target > secs_now { target - secs_now } else { 86_400 - secs_now + target }) as u64
}

/// One rule as the page lists it.
struct Rule {
    dir: PathBuf,
    backup: bool,
    when: String,
    /// Seconds until it runs (None: when the server next runs).
    next: Option<u64>,
    /// Seconds between runs, for the timeline (daily: 86400).
    every: u64,
    last: Option<u64>,
}

impl HostApp {
    fn rules(&self) -> Vec<Rule> {
        let t = now();
        let mut out = vec![];
        for e in &self.list {
            let st = self.status_of(&e.dir);
            if let Some(cfg) = self.configs.get(&e.dir) {
                let every = cfg.pointer("/backups/every").and_then(|v| v.as_str()).unwrap_or("0").to_string();
                if let Some(h) = BACKUP_EVERY.iter().find(|(k, _)| *k == every).map(|(_, h)| *h) {
                    let keep = cfg.pointer("/backups/keep").and_then(|v| v.as_u64()).unwrap_or(12);
                    let last = self.backups.get(&e.dir).and_then(|b| b.iter().filter_map(|b| b.modified).max());
                    let next = st.running().then(|| match last {
                        Some(l) => (l + h * 3600).saturating_sub(t),
                        None => h * 3600 - st.uptime.unwrap_or(0) % (h * 3600),
                    });
                    out.push(Rule { dir: e.dir.clone(), backup: true, when: format!("Every {} · keep {keep}", if h == 24 { "day".to_string() } else if h == 1 { "hour".to_string() } else { format!("{h} hours") }), next, every: h * 3600, last });
                }
            }
            let s = host::schedule(&e.dir);
            let last = (s.last_restart > 0).then_some(s.last_restart);
            if s.restart_every > 0 {
                let every = s.restart_every as u64 * 3600;
                let next = st.running().then(|| every.saturating_sub(st.uptime.unwrap_or(0) % every));
                out.push(Rule { dir: e.dir.clone(), backup: false, when: format!("Every {} hours", s.restart_every), next, every, last });
            } else if let Some(h) = s.restart_hour {
                out.push(Rule { dir: e.dir.clone(), backup: false, when: format!("Daily at {}", hour_name(h)), next: st.running().then(|| until_hour(h)), every: 86_400, last });
            }
        }
        out
    }

    pub fn schedules_page(&mut self, cx: &mut Context<Self>) -> Div {
        let head = pages::header("Backups and restarts that run on their own", "Schedules", None);
        if self.list.is_empty() {
            return pages::page("schedules-scroll", head, ui::panel().child(ui::text(14., FontWeight::NORMAL, ink::QUIET).child("Make a server first: New server, at the bottom left.")));
        }
        // Backups are listed per server once; read them for "last run".
        for e in self.list.clone() {
            if !self.backups.contains_key(&e.dir) {
                self.backups.insert(e.dir.clone(), vec![]);
                self.load_backups(&e.dir);
            }
        }
        let rules = self.rules();
        // The next 24 hours.
        let mut timeline = div().flex().flex_col().gap(px(8.));
        for e in &self.list {
            let st = self.status_of(&e.dir);
            let mut track = div().relative().flex_1().h(px(26.)).rounded(px(7.)).bg(white(0.04)).child(div().absolute().left_0().top(px(-5.)).bottom(px(-5.)).w(px(1.5)).bg(white(0.5)));
            let mut any = false;
            for r in rules.iter().filter(|r| r.dir == e.dir) {
                let Some(first) = r.next else { continue };
                let mut at = first;
                while at <= 86_400 {
                    any = true;
                    let x = at as f32 / 86_400.;
                    let mark = if r.backup {
                        div().size(px(9.)).rounded(px(2.)).bg(ui::color(0x5bb8ff))
                    } else {
                        div().size(px(10.)).rounded_full().bg(ui::color(ui::ACCENT)).border_2().border_color(ui::color(ui::ACCENT).opacity(0.35))
                    };
                    track = track.child(div().absolute().top(px(8.)).left(gpui::relative(x)).ml(px(-5.)).child(mark));
                    at += r.every.max(3600);
                }
            }
            if !any {
                track = track.child(div().absolute().left(px(12.)).top(px(5.)).child(ui::text(11.5, FontWeight::NORMAL, ink::QUIET).child(if st.running() { "nothing scheduled" } else { "stopped" })));
            }
            timeline = timeline.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(14.))
                    .child(div().flex_none().w(px(150.)).flex().items_center().gap(px(8.)).child(ui::dot(server_tone(&st))).child(ui::text(13., FontWeight::NORMAL, ink::FULL).truncate().child(e.name.clone())))
                    .child(track),
            );
        }
        let mut ticks = div().flex().justify_between().ml(px(164.)).mt(px(6.));
        use chrono::Timelike;
        let h0 = chrono::Local::now().hour();
        for k in 0..7 {
            let h = (h0 + k * 4) % 24;
            ticks = ticks.child(ui::mono(10.5, ink::FAINT).child(if k == 0 { format!("now {}", hour_name(h).replace(":00", "")) } else { hour_name(h).replace(":00", "") }));
        }
        let legend = div()
            .flex()
            .gap(px(16.))
            .child(div().flex().items_center().gap(px(6.)).child(div().size(px(9.)).rounded_full().bg(ui::color(ui::ACCENT))).child(ui::text(12., FontWeight::NORMAL, ink::QUIET).child("Restart")))
            .child(div().flex().items_center().gap(px(6.)).child(div().size(px(9.)).rounded(px(2.)).bg(ui::color(0x5bb8ff))).child(ui::text(12., FontWeight::NORMAL, ink::QUIET).child("Backup")));
        let next24 = ui::panel()
            .px(px(20.))
            .py(px(18.))
            .child(div().flex().items_center().justify_between().mb(px(12.)).child(ui::heading(17.).child("Next 24 hours")).child(legend))
            .child(timeline)
            .child(ticks);
        // The rules.
        let cols = |a: Div, b: Div, c: Div, d: Div, e: Div, f: Div| {
            div().flex().items_center().gap(px(12.)).child(a.flex_1().min_w_0()).child(b.flex_none().w(px(100.))).child(c.flex_none().w(px(190.))).child(d.flex_none().w(px(110.))).child(e.flex_none().w(px(120.))).child(f.flex_none().w(px(70.)))
        };
        let mut list = div().flex().flex_col();
        if rules.is_empty() {
            list = list.child(ui::text(13., FontWeight::NORMAL, ink::QUIET).px(px(12.)).py(px(14.)).child("No schedules yet. Add one on the right."));
        }
        let n = rules.len();
        for (i, r) in rules.iter().enumerate() {
            let st = self.status_of(&r.dir);
            let task = div()
                .flex()
                .items_center()
                .gap(px(7.))
                .child(if r.backup { div().size(px(8.)).rounded(px(2.)).bg(ui::color(0x5bb8ff)) } else { div().size(px(8.)).rounded_full().bg(ui::color(ui::ACCENT)) })
                .child(ui::text(13., FontWeight::NORMAL, ink::FULL).child(if r.backup { "Backup" } else { "Restart" }));
            let next = match r.next {
                Some(s) => format!("in {}", ui::span(s)),
                None => "when it runs".into(),
            };
            let last = match r.last {
                Some(l) => {
                    let s = now().saturating_sub(l);
                    if s < 120 { "just now".to_string() } else { format!("{} ago", ui::span(s)) }
                }
                None => "not yet".into(),
            };
            let key = format!("rule:{}:{}", r.dir.display(), r.backup);
            let confirming = self.pages.confirm.as_deref() == Some(key.as_str());
            let (d, backup, key2) = (r.dir.clone(), r.backup, key.clone());
            let remove = div()
                .id(SharedString::from(format!("rule-rm-{i}")))
                .flex()
                .items_center()
                .justify_center()
                .h(px(28.))
                .px(px(if confirming { 8. } else { 0. }))
                .w(if confirming { gpui::Length::Auto } else { px(28.).into() })
                .rounded(px(radius::CONTROL))
                .cursor_pointer()
                .bg(if confirming { ui::color(ui::DANGER).opacity(0.14) } else { white(0.08) })
                .hover(|s| s.bg(ui::color(ui::DANGER).opacity(0.2)))
                .child(if confirming { ui::text(12., FontWeight::MEDIUM, ink::FULL).text_color(ui::color(ui::DANGER)).child("Remove?").into_any_element() } else { ui::icon("delete-02", 13., white(ink::BODY)).into_any_element() })
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    if this.pages.confirm.as_deref() != Some(key2.as_str()) {
                        this.pages.confirm = Some(key2.clone());
                    } else {
                        this.pages.confirm = None;
                        this.remove_rule(d.clone(), backup);
                    }
                    cx.notify();
                }));
            list = list.child(
                cols(
                    div().flex().items_center().gap(px(8.)).child(ui::dot(server_tone(&st))).child(ui::text(13.5, FontWeight::SEMIBOLD, ink::FULL).truncate().child(self.name_of(&r.dir))),
                    task,
                    ui::text(13., FontWeight::NORMAL, ink::FULL).child(r.when.clone()),
                    ui::mono(12., if r.next.is_some() { ink::FULL } else { ink::QUIET }).child(next),
                    div().flex().items_center().gap(px(6.)).children(r.last.map(|_| ui::icon("tick-02", 13., ui::color(ui::LIVE)))).child(ui::text(12.5, FontWeight::NORMAL, ink::QUIET).child(last)),
                    div().flex().justify_end().child(remove),
                )
                .h(px(46.))
                .px(px(12.))
                .when(i + 1 < n, |d| d.border_b_1().border_color(white(0.06))),
            );
        }
        let table = ui::panel()
            .flex_1()
            .min_w_0()
            .px(px(16.))
            .py(px(16.))
            .child(div().px(px(4.)).pb(px(10.)).child(panel_title("Rules", Some(rules.len()))))
            .child(cols(ui::th("SERVER"), ui::th("TASK"), ui::th("WHEN"), ui::th("NEXT"), ui::th("LAST RUN"), div()).h(px(32.)).px(px(12.)).border_b_1().border_color(white(0.08)))
            .child(list);
        let form = self.schedule_form(cx);
        let bottom = if ui::wide() {
            div().flex().items_start().gap(px(16.)).child(table).child(form.flex_none().w(px(360.)))
        } else {
            div().flex().flex_col().gap(px(16.)).child(table).child(form)
        };
        pages::page("schedules-scroll", head, div().flex().flex_col().gap(px(16.)).child(next24).child(bottom))
    }

    fn remove_rule(&mut self, dir: PathBuf, backup: bool) {
        if backup {
            let tx = self.tx.clone();
            std::thread::spawn(move || {
                if let Err(e) = servers::set_config(&dir, &[("backups.every", "0".to_string())]) {
                    let _ = tx.send(Ev::Note(e.message));
                }
                if let Some(c) = servers::config(&dir) {
                    let _ = tx.send(Ev::Settings(dir, c));
                }
            });
        } else {
            let mut s = host::schedule(&dir);
            s.restart_every = 0;
            s.restart_hour = None;
            if let Err(e) = host::save_schedule(&dir, &s) {
                self.note = Some((e, std::time::Instant::now()));
            }
        }
    }

    fn schedule_form(&mut self, cx: &mut Context<Self>) -> Div {
        let mut f = self.pages.form.clone();
        if f.server.as_ref().is_none_or(|d| !self.list.iter().any(|e| &e.dir == d)) {
            f.server = self.first_server();
        }
        if f.every == 0 {
            f.every = 6;
        }
        self.pages.form = f.clone();
        let picker = self.server_picker("form-server", f.server.clone(), false, cx, |this, d, _| this.pages.form.server = d);
        let e = cx.entity();
        let set = move |change: fn(&mut ScheduleForm, u32), v: u32, cx: &mut gpui::App, e: &gpui::Entity<HostApp>| {
            e.update(cx, |this, cx| {
                change(&mut this.pages.form, v);
                cx.notify();
            })
        };
        let task = ui::tabs("form-task", &[(0u32, "Restart"), (1, "Backup")], u32::from(f.backup), {
            let e = e.clone();
            move |v, _, cx| set(|f, v| f.backup = v == 1, v, cx, &e)
        });
        let repeats: Vec<(u32, &str)> = if f.backup { vec![(1, "Every 1 h"), (3, "3 h"), (6, "6 h"), (12, "12 h"), (24, "Daily")] } else { vec![(6, "Every 6 h"), (12, "Every 12 h"), (24, "Daily")] };
        let current = if repeats.iter().any(|(v, _)| *v == f.every) { f.every } else { repeats[0].0 };
        let repeat = ui::tabs("form-repeat", &repeats, current, {
            let e = e.clone();
            move |v, _, cx| set(|f, v| f.every = v, v, cx, &e)
        });
        let at = ui::tabs("form-at", &AT, f.hour, {
            let e = e.clone();
            move |v, _, cx| set(|f, v| f.hour = v, v, cx, &e)
        });
        let warn = self.settings.warn_before_restart;
        let warn_toggle = ui::toggle("form-warn", warn).on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
            this.settings.warn_before_restart = !warn;
            let _ = stats::save_settings(&this.settings);
            cx.notify();
        }));
        let label = |t: &'static str| ui::text(12., FontWeight::MEDIUM, ink::QUIET).mt(px(12.)).mb(px(6.)).child(t);
        let add = div()
            .id("form-add")
            .flex()
            .items_center()
            .justify_center()
            .flex_1()
            .h(px(38.))
            .rounded(px(radius::CONTROL))
            .bg(ui::color(ui::ACCENT))
            .cursor_pointer()
            .hover(|s| s.opacity(0.9))
            .child(ui::text(13.5, FontWeight::SEMIBOLD, ink::FULL).child("Add schedule"))
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                this.add_rule();
                cx.notify();
            }));
        let replaces = f.server.as_ref().is_some_and(|d| {
            let s = host::schedule(d);
            if f.backup {
                self.configs.get(d).and_then(|c| c.pointer("/backups/every").and_then(|v| v.as_str()).map(|v| v != "0")).unwrap_or(false)
            } else {
                s.restart_every > 0 || s.restart_hour.is_some()
            }
        });
        ui::panel()
            .p(px(20.))
            .child(ui::heading(17.).child("Add a schedule"))
            .child(label("Server"))
            .child(picker)
            .child(label("Task"))
            .child(task)
            .child(label("Repeat"))
            .child(repeat)
            .when(!f.backup && current == 24, |d| d.child(label("At")).child(at))
            .when(!f.backup, |d| {
                d.child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .mt(px(16.))
                        .child(div().flex().flex_col().child(ui::text(13., FontWeight::MEDIUM, ink::FULL).child("Warn players first")).child(ui::text(12., FontWeight::NORMAL, ink::QUIET).child("A chat message 5 minutes before")))
                        .child(warn_toggle),
                )
            })
            .when(replaces, |d| d.child(ui::text(12., FontWeight::NORMAL, ink::QUIET).mt(px(12.)).child(if f.backup { "Takes the place of this server's backup schedule." } else { "Takes the place of this server's restart schedule." })))
            .child(div().flex().mt(px(18.)).child(add))
            .child(ui::text(11.5, FontWeight::NORMAL, ink::FAINT).mt(px(10.)).child(if f.backup { "Backups run while the server runs." } else { "Restarts run while HorizonX is open." }))
    }

    fn add_rule(&mut self) {
        let f = self.pages.form.clone();
        let Some(dir) = f.server else { return };
        if f.backup {
            let every = if f.every >= 24 { "24h".to_string() } else { format!("{}h", f.every.max(1)) };
            let tx = self.tx.clone();
            std::thread::spawn(move || {
                let note = match servers::set_config(&dir, &[("backups.every", every)]) {
                    Ok(_) => "Backup schedule saved.".to_string(),
                    Err(e) => e.message,
                };
                let _ = tx.send(Ev::Note(note));
                if let Some(c) = servers::config(&dir) {
                    let _ = tx.send(Ev::Settings(dir, c));
                }
            });
        } else {
            let mut s = host::schedule(&dir);
            if f.every >= 24 {
                s.restart_every = 0;
                s.restart_hour = Some(f.hour);
            } else {
                s.restart_every = f.every;
                s.restart_hour = None;
            }
            self.note = Some((
                match host::save_schedule(&dir, &s) {
                    Ok(()) => "Restart schedule saved.".to_string(),
                    Err(e) => e,
                },
                std::time::Instant::now(),
            ));
        }
    }
}
