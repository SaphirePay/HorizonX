//! Players: everyone on every server (or one), online now or ever, operators and bans, with the chosen player
//! beside the list: their head, playtime, sessions, days played, the operator and friends-list switches, a
//! message, Kick and Ban.
use crate::app::{Field, HostApp};
use crate::chart;
use crate::frame::server_tone;
use crate::pages::{self, PlayersTab};
use crate::ui::{self, px, white, Kind, Tone};
use gpui::{div, prelude::*, ClickEvent, Context, Div, FontWeight, SharedString};
use horizon_launcher::servers;
use horizon_launcher::stats::PlayerRecord;
use horizon_theme::glass::{fill, ink, radius};
use std::path::PathBuf;

const PER_PAGE: usize = 12;

/// One player on one server.
#[derive(Clone)]
struct Person {
    dir: PathBuf,
    server: String,
    name: String,
    /// UUID (or the lowercase name when the server gave none).
    key: String,
    online: bool,
    ping: Option<u64>,
    op: bool,
    rec: Option<PlayerRecord>,
}

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn day(t: u64) -> String {
    use chrono::TimeZone;
    if now().saturating_sub(t) < 86_400 {
        return "today".into();
    }
    chrono::Local.timestamp_opt(t as i64, 0).single().map(|d| d.format("%b %-d").to_string()).unwrap_or_default()
}

fn ago(t: u64) -> String {
    match now().saturating_sub(t) {
        s if s < 120 => "just now".into(),
        s => format!("{} ago", ui::span(s)),
    }
}

fn hours(secs: u64) -> String {
    if secs >= 3600 {
        format!("{} h", secs / 3600)
    } else if secs >= 60 {
        format!("{}m", secs / 60)
    } else {
        "–".into()
    }
}

impl HostApp {
    /// Everyone the page could list, by its tab and server.
    fn people(&self, tab: PlayersTab, only: Option<&PathBuf>) -> Vec<Person> {
        let mut out = vec![];
        for e in &self.list {
            if only.is_some_and(|d| d != &e.dir) {
                continue;
            }
            let st = self.status_of(&e.dir);
            let stats = self.stats.get(&e.dir);
            let ops: Vec<String> = servers::names(&e.dir, "ops.json").into_iter().map(|n| n.to_lowercase()).collect();
            let record = |key: &str, name: &str| stats.and_then(|s| s.players.get(key).or_else(|| s.players.values().find(|p| p.name.eq_ignore_ascii_case(name)))).cloned();
            match tab {
                PlayersTab::Online => {
                    for p in st.players.iter().filter(|_| st.running()) {
                        let key = if p.uuid.is_empty() { p.name.to_lowercase() } else { p.uuid.clone() };
                        out.push(Person { dir: e.dir.clone(), server: e.name.clone(), name: p.name.clone(), rec: record(&key, &p.name), key, online: true, ping: p.ping, op: ops.contains(&p.name.to_lowercase()) });
                    }
                }
                PlayersTab::Everyone => {
                    for (key, rec) in stats.map(|s| s.players.iter().collect::<Vec<_>>()).unwrap_or_default() {
                        let live = st.running().then(|| st.players.iter().find(|p| &p.uuid == key || p.name.eq_ignore_ascii_case(&rec.name))).flatten();
                        out.push(Person { dir: e.dir.clone(), server: e.name.clone(), name: rec.name.clone(), key: key.clone(), online: live.is_some(), ping: live.and_then(|p| p.ping), op: ops.contains(&rec.name.to_lowercase()), rec: Some(rec.clone()) });
                    }
                }
                PlayersTab::Operators | PlayersTab::Banned => {
                    let file = if tab == PlayersTab::Operators { "ops.json" } else { "banned-players.json" };
                    for (name, uuid) in servers::entries(&e.dir, file) {
                        let live = st.running().then(|| st.players.iter().find(|p| p.name.eq_ignore_ascii_case(&name))).flatten();
                        let key = if uuid.is_empty() { name.to_lowercase() } else { uuid.clone() };
                        out.push(Person { dir: e.dir.clone(), server: e.name.clone(), rec: record(&key, &name), name, key, online: live.is_some(), ping: live.and_then(|p| p.ping), op: tab == PlayersTab::Operators });
                    }
                }
            }
        }
        out.sort_by(|a, b| b.online.cmp(&a.online).then(b.rec.as_ref().map(|r| r.last_seen).cmp(&a.rec.as_ref().map(|r| r.last_seen))).then(a.name.to_lowercase().cmp(&b.name.to_lowercase())));
        out
    }

    pub fn players_page(&mut self, cx: &mut Context<Self>) -> Div {
        let only = self.pages.players_server.clone().filter(|d| self.list.iter().any(|e| &e.dir == d));
        let tab = self.pages.players_tab;
        let online_now: usize = self.list.iter().map(|e| self.status_of(&e.dir)).filter(|s| s.running()).map(|s| s.players.len()).sum();
        let ever: usize = self.stats.values().map(|s| s.players.len()).sum();
        let counts = [
            self.people(PlayersTab::Online, only.as_ref()).len(),
            self.people(PlayersTab::Everyone, only.as_ref()).len(),
            self.people(PlayersTab::Operators, only.as_ref()).len(),
            self.people(PlayersTab::Banned, only.as_ref()).len(),
        ];
        let people = self.people(tab, only.as_ref());
        let picker = self.server_picker("players-server", only.clone(), true, cx, |this, d, _| {
            this.pages.players_server = d;
            this.pages.players_page = 0;
        });
        let head = pages::header(
            format!("{online_now} online now · {ever} have played"),
            "Players",
            Some(div().flex().items_center().gap(px(10.)).child(ui::text(13., FontWeight::NORMAL, ink::QUIET).child("Server")).child(picker)),
        );
        let tabs = ui::tabs(
            "players-tab",
            &[
                (PlayersTab::Online, format!("Online {}", counts[0])),
                (PlayersTab::Everyone, format!("Everyone {}", counts[1])),
                (PlayersTab::Operators, format!("Operators {}", counts[2])),
                (PlayersTab::Banned, format!("Banned {}", counts[3])),
            ],
            tab,
            {
                let e = cx.entity();
                move |t, _, cx| {
                    e.update(cx, |this, cx| {
                        this.pages.players_tab = t;
                        this.pages.players_page = 0;
                        cx.notify();
                    })
                }
            },
        );
        // The chosen player: kept when still listed, else the first.
        let chosen = self
            .pages
            .player
            .clone()
            .and_then(|(d, k)| people.iter().find(|p| p.dir == d && p.key == k).cloned())
            .or_else(|| people.first().cloned());
        if let Some(p) = &chosen {
            self.pages.player = Some((p.dir.clone(), p.key.clone()));
        }
        let cols = |a: Div, b: Div, c: Div, d: Div, e: Div, f: Div| {
            div()
                .flex()
                .items_center()
                .gap(px(12.))
                .child(a.flex_1().min_w_0())
                .child(b.flex_none().w(px(150.)))
                .child(c.flex_none().w(px(150.)))
                .child(d.flex_none().w(px(84.)))
                .child(e.flex_none().w(px(100.)))
                .child(f.flex_none().w(px(64.)))
        };
        let pages_n = people.len().div_ceil(PER_PAGE).max(1);
        let page = self.pages.players_page.min(pages_n - 1);
        let mut rows = div().flex().flex_col();
        if people.is_empty() {
            rows = rows.child(ui::text(13., FontWeight::NORMAL, ink::QUIET).px(px(10.)).py(px(14.)).child(match tab {
                PlayersTab::Online => "Nobody is online right now.",
                PlayersTab::Everyone => "Players show here once they've joined while HorizonX was open.",
                PlayersTab::Operators => "No operators. Make someone one from their row, while the server runs.",
                PlayersTab::Banned => "Nobody is banned.",
            }));
        }
        for (i, p) in people.iter().enumerate().skip(page * PER_PAGE).take(PER_PAGE) {
            let face = self.face(&p.key);
            let sel = chosen.as_ref().is_some_and(|c| c.dir == p.dir && c.key == p.key);
            let new = p.rec.as_ref().is_some_and(|r| now().saturating_sub(r.first_seen) < 86_400);
            let session = match (&p.rec, p.online) {
                (Some(r), true) if r.session_start > 0 => match now().saturating_sub(r.session_start) {
                    s if s < 60 => "Online · just joined".into(),
                    s => format!("Online · {}", ui::span(s)),
                },
                (_, true) => "Online".into(),
                (Some(r), false) => format!("seen {}", ago(r.last_seen)),
                (None, false) => "–".into(),
            };
            let tone = server_tone(&self.status_of(&p.dir));
            let (d, k) = (p.dir.clone(), p.key.clone());
            rows = rows.child(
                cols(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(10.))
                        .child(ui::head(face.as_deref(), 28.))
                        .child(ui::text(13.5, FontWeight::SEMIBOLD, ink::FULL).truncate().child(p.name.clone()))
                        .when(p.op, |d| d.child(ui::tag("OP", true)))
                        .when(new && !p.op, |d| d.child(ui::tag("NEW", false))),
                    div().flex().items_center().gap(px(7.)).child(ui::dot(tone)).child(ui::text(13., FontWeight::NORMAL, ink::FULL).truncate().child(p.server.clone())),
                    ui::text(13., FontWeight::NORMAL, if p.online { ink::FULL } else { ink::QUIET }).child(session),
                    ui::text(13., FontWeight::NORMAL, ink::FULL).child(p.rec.as_ref().map(|r| hours(r.playtime)).unwrap_or_else(|| "–".into())),
                    ui::text(13., FontWeight::NORMAL, ink::QUIET).child(p.rec.as_ref().map(|r| day(r.first_seen)).unwrap_or_else(|| "–".into())),
                    ui::mono(12., ink::BODY).text_color(if p.ping.is_some_and(|ms| ms > 60) { ui::color(ui::WARN) } else { white(ink::BODY) }).child(p.ping.map(|ms| format!("{ms} ms")).unwrap_or_else(|| "–".into())),
                )
                .id(SharedString::from(format!("pl-row-{i}")))
                .px(px(10.))
                .py(px(8.))
                .rounded(px(radius::ROW))
                .cursor_pointer()
                .when(sel, |d| d.bg(white(fill::HOVER + 0.01)))
                .when(!sel, |d| d.hover(|s| s.bg(white(0.03))))
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    this.pages.player = Some((d.clone(), k.clone()));
                    this.pages.confirm = None;
                    cx.notify();
                })),
            );
        }
        let pager = div()
            .flex()
            .items_center()
            .justify_between()
            .pt(px(12.))
            .mt(px(6.))
            .border_t_1()
            .border_color(white(0.06))
            .child(ui::text(12., FontWeight::NORMAL, ink::QUIET).child(if people.is_empty() { String::new() } else { format!("Showing {}–{} of {}", page * PER_PAGE + 1, ((page + 1) * PER_PAGE).min(people.len()), people.len()) }))
            .child(
                div()
                    .flex()
                    .gap(px(6.))
                    .child(ui::sized_button("pl-prev", "Previous", Kind::Ghost, 30., 12.5).on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                        this.pages.players_page = this.pages.players_page.saturating_sub(1);
                        cx.notify();
                    })))
                    .child(ui::sized_button("pl-next", "Next", Kind::Secondary, 30., 12.5).on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        this.pages.players_page = (this.pages.players_page + 1).min(pages_n - 1);
                        cx.notify();
                    }))),
            );
        let table = ui::panel()
            .flex_1()
            .min_w_0()
            .px(px(18.))
            .py(px(18.))
            .child(div().flex().items_center().justify_between().mb(px(12.)).child(tabs))
            .child(cols(ui::th("PLAYER"), ui::th("SERVER"), ui::th("SESSION"), ui::th("PLAYTIME"), ui::th("FIRST JOINED"), ui::th("PING")).px(px(10.)).pb(px(9.)).border_b_1().border_color(white(0.08)))
            .child(rows)
            .when(people.len() > PER_PAGE, |d| d.child(pager));
        let detail = match chosen {
            Some(p) => self.player_detail(&p, !ui::wide(), cx),
            None => ui::panel().child(ui::text(13., FontWeight::NORMAL, ink::QUIET).child("Pick a player to see them here.")),
        };
        let body = if ui::wide() {
            div().flex().items_start().gap(px(16.)).child(table).child(detail.flex_none().w(px(372.)))
        } else {
            div().flex().flex_col().gap(px(16.)).child(table).child(detail)
        };
        pages::page("players-scroll", head, body)
    }

    fn player_detail(&mut self, p: &Person, two_columns: bool, cx: &mut Context<Self>) -> Div {
        let face = self.face(&p.key);
        let running = self.status_of(&p.dir).running();
        let rec = p.rec.clone().unwrap_or_default();
        let whitelisted = servers::names(&p.dir, "whitelist.json").iter().any(|n| n.eq_ignore_ascii_case(&p.name));
        // Who they are and where.
        let since = |t: u64| match now().saturating_sub(t) {
            s if s < 60 => "just joined".to_string(),
            s => format!("for {}", ui::span(s)),
        };
        let status = if p.online {
            div().flex().items_center().gap(px(6.)).child(ui::dot(Tone::Live)).child(ui::text(12.5, FontWeight::NORMAL, ink::FULL).text_color(ui::color(ui::LIVE)).child(format!("Online on {}{}", p.server, if rec.session_start > 0 { format!(" · {}", since(rec.session_start)) } else { String::new() })))
        } else {
            div().flex().items_center().gap(px(6.)).child(ui::dot(Tone::Off)).child(ui::text(12.5, FontWeight::NORMAL, ink::QUIET).child(if rec.last_seen > 0 { format!("Offline · last on {} {}", p.server, ago(rec.last_seen)) } else { format!("Offline · {}", p.server) }))
        };
        let short = if p.key.len() > 12 { format!("{}…{}", &p.key[..4], &p.key[p.key.len() - 4..]) } else { p.key.clone() };
        let uuid = p.key.clone();
        let copied = self.copied(&uuid);
        let uuid_line = div()
            .id("pd-uuid")
            .flex()
            .items_center()
            .gap(px(5.))
            .mt(px(3.))
            .cursor_pointer()
            .hover(|s| s.opacity(0.8))
            .child(ui::mono(10.5, ink::FAINT).child(format!("UUID {short}")))
            .child(ui::icon(if copied { "tick-02" } else { "copy-01" }, 11., white(ink::FAINT)))
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.copy(uuid.clone(), cx)));
        // The numbers, one row.
        let stat = |v: String, l: &'static str| div().flex_1().min_w_0().flex().flex_col().gap(px(1.)).child(ui::heading(16.).child(v)).child(ui::text(11.5, FontWeight::NORMAL, ink::QUIET).child(l));
        let average = if rec.sessions > 0 { ui::span(rec.playtime / rec.sessions as u64).replace("now", "<1m") } else { "–".into() };
        let numbers = div()
            .flex()
            .gap(px(12.))
            .mt(px(16.))
            .px(px(14.))
            .py(px(12.))
            .rounded(px(radius::ROW))
            .bg(white(0.045))
            .child(stat(hours(rec.playtime).replace('–', "<1m"), "Playtime"))
            .child(stat(if rec.sessions > 0 { rec.sessions.to_string() } else { "–".into() }, "Sessions"))
            .child(stat(average, "Avg session"))
            .child(stat(if rec.first_seen > 0 { day(rec.first_seen) } else { "–".into() }, "First joined"));
        // Time played by day, the last 14 days: minutes until there are hours.
        let today = chrono::Local::now().date_naive();
        let secs: Vec<(chrono::NaiveDate, u32)> = (0..14)
            .rev()
            .map(|k| {
                let d = today - chrono::Duration::days(k);
                (d, rec.days.get(&d.format("%Y-%m-%d").to_string()).copied().unwrap_or(0))
            })
            .collect();
        let max = secs.iter().map(|s| s.1).max().unwrap_or(0);
        let in_hours = max >= 3600;
        let values: Vec<f32> = secs.iter().map(|s| if in_hours { s.1 as f32 / 3600. } else { s.1 as f32 / 60. }).collect();
        let total: u32 = secs.iter().map(|s| s.1).sum();
        let today_secs = secs.last().map(|s| s.1).unwrap_or(0);
        let fmt = |s: u32| if s == 0 { "none".to_string() } else if s < 60 { "<1m".to_string() } else { ui::span(s as u64) };
        let mut day_labels = div().flex().gap(px(3.)).mt(px(5.));
        for (k, (d, _)) in secs.iter().enumerate() {
            let label = if k == secs.len() - 1 { "Today".to_string() } else if k % 2 == 1 { d.format("%-d").to_string() } else { String::new() };
            day_labels = day_labels.child(div().flex_1().flex().justify_center().child(ui::mono(9.5, ink::FAINT).child(label)));
        }
        let activity = div()
            .flex()
            .flex_col()
            .child(section("Activity · last 14 days"))
            .child(div().flex().items_end().justify_between().mb(px(8.)).child(ui::text(12.5, FontWeight::NORMAL, ink::FULL).child(format!("{} today", fmt(today_secs)))).child(ui::text(12., FontWeight::NORMAL, ink::QUIET).child(format!("{} in 14 days · bars in {}", fmt(total), if in_hours { "hours" } else { "minutes" }))))
            .child(chart::bars(values, ui::color(ui::ACCENT), 46.))
            .child(day_labels);
        // Access: what they may do. These run commands on the server, so they need it running.
        let (d1, n1, op) = (p.dir.clone(), p.name.clone(), p.op);
        let op_toggle = ui::toggle("pd-op", p.op).when(running, |t| {
            t.on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                this.run_console(&d1, format!("{} {}", if op { "deop" } else { "op" }, n1));
                cx.notify();
            }))
        });
        let (d2, n2) = (p.dir.clone(), p.name.clone());
        let wl_toggle = ui::toggle("pd-wl", whitelisted).when(running, |t| {
            t.on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                this.run_console(&d2, format!("whitelist {} {}", if whitelisted { "remove" } else { "add" }, n2));
                cx.notify();
            }))
        });
        let access_row = |title: &'static str, note: &'static str, control: gpui::Stateful<Div>| {
            div()
                .flex()
                .items_center()
                .justify_between()
                .gap(px(16.))
                .py(px(9.))
                .child(div().flex().flex_col().min_w_0().child(ui::text(13., FontWeight::MEDIUM, ink::FULL).child(title)).child(ui::text(11.5, FontWeight::NORMAL, ink::QUIET).child(note)))
                .child(div().when(!running, |d| d.opacity(0.4)).child(control))
        };
        let access = div()
            .flex()
            .flex_col()
            .child(if two_columns { div().mb(px(6.)).child(ui::mono(10.5, ink::QUIET).child("ACCESS")) } else { section("Access") })
            .child(access_row("Operator", "Can use admin commands", op_toggle))
            .child(access_row("Whitelisted", "Can join when the server is friends-only", wl_toggle));
        // Message, while they're on.
        let message = self.field(Field::Message, "Write a message…", cx);
        let send = ui::sized_button("pd-send", "Send", Kind::Secondary, 40., 13.).on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.send_message(cx)));
        // Moderation: rows like Access, a small button where Access has its switch.
        let banned = servers::names(&p.dir, "banned-players.json").iter().any(|n| n.eq_ignore_ascii_case(&p.name));
        let ban_key = format!("ban:{}:{}", p.dir.display(), p.name);
        let confirming = self.pages.confirm.as_deref() == Some(ban_key.as_str());
        let (d3, n3) = (p.dir.clone(), p.name.clone());
        let can_kick = running && p.online;
        let kick = ui::sized_button("pd-kick", "Kick", Kind::Secondary, 30., 12.5).when(can_kick, |b| {
            b.on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                this.run_console(&d3, format!("kick {n3}"));
                cx.notify();
            }))
        });
        let (d4, n4) = (p.dir.clone(), p.name.clone());
        let ban = ui::sized_button("pd-ban", if banned { "Unban" } else if confirming { "Confirm ban" } else { "Ban" }, if banned { Kind::Secondary } else { Kind::Danger }, 30., 12.5).when(running, |b| {
            b.on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                if banned {
                    this.run_console(&d4, format!("pardon {n4}"));
                } else if this.pages.confirm.as_deref() == Some(ban_key.as_str()) {
                    this.pages.confirm = None;
                    this.run_console(&d4, format!("ban {n4}"));
                } else {
                    this.pages.confirm = Some(ban_key.clone());
                }
                cx.notify();
            }))
        });
        let action_row = |title: &'static str, note: &'static str, enabled: bool, control: gpui::Stateful<Div>| {
            div()
                .flex()
                .items_center()
                .justify_between()
                .gap(px(16.))
                .py(px(9.))
                .child(div().flex().flex_col().min_w_0().child(ui::text(13., FontWeight::MEDIUM, ink::FULL).child(title)).child(ui::text(11.5, FontWeight::NORMAL, ink::QUIET).child(note)))
                .child(div().flex_none().when(!enabled, |d| d.opacity(0.4)).child(control))
        };
        let moderation = div()
            .flex()
            .flex_col()
            .child(section("Moderation"))
            .child(action_row("Kick", if p.online { "Removes them from the server now. They can join again." } else { "Only while they're online." }, can_kick, kick))
            .child(if banned {
                action_row("Banned", "Can't join this server.", running, ban)
            } else {
                action_row("Ban", "Keeps them off this server.", running, ban)
            })
            .when(!running, |d| d.child(ui::text(11.5, FontWeight::NORMAL, ink::QUIET).mt(px(4.)).child(format!("Start {} to change these.", p.server))));
        let who = div()
            .flex()
            .items_center()
            .gap(px(14.))
            .child(ui::head(face.as_deref(), 52.))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .min_w_0()
                    .child(div().flex().items_center().gap(px(6.)).child(ui::heading(20.).truncate().child(p.name.clone())).when(p.op, |d| d.child(ui::tag("OP", true))))
                    .child(status)
                    .child(uuid_line),
            );
        let left = div().flex().flex_col().min_w_0().child(who).child(numbers).child(activity);
        let right = div()
            .flex()
            .flex_col()
            .min_w_0()
            .child(access)
            .when(running && p.online, |d| d.child(div().flex().flex_col().child(section("Message")).child(div().flex().gap(px(8.)).child(div().flex_1().child(message)).child(send))))
            .child(moderation);
        if two_columns {
            // Under the list, the panel is wide: who they are on the left, what can be done on the right.
            ui::panel()
                .p(px(20.))
                .child(
                    div()
                        .flex()
                        .gap(px(28.))
                        .child(left.flex_1())
                        .child(div().flex_none().w(px(1.)).bg(white(0.07)))
                        .child(right.flex_none().w(px(420.))),
                )
        } else {
            ui::panel().p(px(20.)).child(left).child(right)
        }
    }
    /// The message in the Players page's field, to the chosen player.
    pub fn send_message(&mut self, cx: &mut Context<Self>) {
        let text = self.text(Field::Message).trim().to_string();
        let Some((dir, key)) = self.pages.player.clone() else { return };
        if text.is_empty() {
            return;
        }
        let name = self.people(PlayersTab::Everyone, Some(&dir)).into_iter().chain(self.people(PlayersTab::Online, Some(&dir))).find(|p| p.key == key).map(|p| p.name);
        if let Some(name) = name {
            self.run_console(&dir, format!("tell {name} {text}"));
            self.set_text(Field::Message, "");
            self.note = Some((format!("Sent to {name}."), std::time::Instant::now()));
        }
        cx.notify();
    }
}

/// A section's small heading, a rule above it.
fn section(title: &'static str) -> Div {
    div().mt(px(16.)).pt(px(14.)).mb(px(6.)).border_t_1().border_color(white(0.07)).child(ui::mono(10.5, ink::QUIET).child(title.to_uppercase()))
}

#[allow(unused)]
fn _keep(_: SharedString) {}
