//! The Dashboard: a greeting; the totals of every server (players, servers online, health, memory); players
//! over time for one server or all (its range, peak, average, different players, busiest hour, average
//! session, a hover readout); every server's performance, slowest first; the servers as a table (a row picks
//! the chart's server); what happened lately. With no servers yet, a welcome that leads into the wizard.
use crate::app::HostApp;
use crate::chart;
use crate::frame::{server_tone, Page};
use crate::pages::{self, panel_title};
use crate::ui::{self, px, white, Kind, Tone};
use gpui::{div, prelude::*, ClickEvent, Context, Div, FontWeight, MouseMoveEvent, SharedString};
use horizon_launcher::servers::Status;
use horizon_launcher::stats::Range;
use horizon_theme::glass::{fill, ink, radius};
use std::path::{Path, PathBuf};

const SIDE_W: f32 = 340.;
const SKY: u32 = 0x5bb8ff;

/// How a server's ticks are going (20 TPS is full speed): its word, dot and colour.
pub fn health(tps: f64) -> (&'static str, Tone, u32) {
    if tps >= 19.5 {
        ("Smooth", Tone::Live, ui::LIVE)
    } else if tps >= 15. {
        ("Busy", Tone::Busy, ui::WARN)
    } else {
        ("Struggling", Tone::Bad, ui::DANGER)
    }
}

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn local(t: u64, fmt: &str) -> String {
    use chrono::TimeZone;
    chrono::Local.timestamp_opt(t as i64, 0).single().map(|d| d.format(fmt).to_string()).unwrap_or_default()
}

/// The range's slots: how many, and seconds each.
fn slots(range: Range) -> (usize, u64) {
    match range {
        Range::Hour => (60, 60),
        Range::Day => (144, 600),
        Range::Week => (168, 3600),
        Range::Month => (180, 4 * 3600),
    }
}

/// Samples laid on the range's even time slots (a slot with none is 0: the server wasn't running), oldest first.
fn on_slots(samples: &[(u64, f32)], range: Range, t: u64) -> Vec<(u64, f32)> {
    let (n, step) = slots(range);
    let start = t.saturating_sub(n as u64 * step);
    let mut out: Vec<(u64, f32, u32)> = (0..n).map(|i| (start + i as u64 * step, 0., 0)).collect();
    for (ts, v) in samples {
        if *ts < start {
            continue;
        }
        let i = (((ts - start) / step) as usize).min(n - 1);
        out[i].1 += v;
        out[i].2 += 1;
    }
    out.into_iter().map(|(ts, s, k)| (ts, if k > 0 { s / k as f32 } else { 0. })).collect()
}

impl HostApp {
    /// The players of one server, or all added together, on the range's slots.
    fn player_series(&self, server: Option<&Path>, range: Range) -> Vec<(u64, f32)> {
        let t = now();
        let mut total: Vec<(u64, f32)> = vec![];
        for e in &self.list {
            if server.is_some_and(|d| d != e.dir) {
                continue;
            }
            let Some(s) = self.stats.get(&e.dir) else { continue };
            let one = on_slots(&s.series(range, t), range, t);
            if total.is_empty() {
                total = one;
            } else {
                for (a, b) in total.iter_mut().zip(one) {
                    a.1 += b.1;
                }
            }
        }
        if total.is_empty() {
            total = on_slots(&[], range, t);
        }
        total
    }

    pub fn home(&mut self, cx: &mut Context<Self>) -> Div {
        if self.list.is_empty() {
            return self.welcome(cx);
        }
        let rows: Vec<(String, PathBuf, Status)> = self.list.iter().map(|e| (e.name.clone(), e.dir.clone(), self.status_of(&e.dir))).collect();
        use chrono::Timelike;
        let now_local = chrono::Local::now();
        let greeting = match now_local.hour() {
            5..=11 => "Good morning",
            12..=17 => "Good afternoon",
            _ => "Good evening",
        };
        let running: Vec<PathBuf> = rows.iter().filter(|(_, _, s)| s.running()).map(|(_, d, _)| d.clone()).collect();
        let backup_all = ui::icon_button("dash-backup-all", "download-04", "Back up all", Kind::Secondary)
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                for d in running.clone() {
                    this.server_action(d, "Backing up", vec!["backup".into()], cx);
                }
                if running.is_empty() {
                    this.note = Some(("No server is running.".into(), std::time::Instant::now()));
                }
            }));
        let head = pages::header(format!("{greeting} · {}", now_local.format("%A, %B %-d")), "Dashboard", Some(div().flex().gap(px(8.)).child(backup_all)));
        let tiles = self.tiles(&rows);
        let chart = self.players_panel(cx);
        let perf = self.performance_panel(&rows, cx);
        let table = self.servers_table(&rows, cx);
        let activity = self.activity_panel();
        let (middle, bottom) = if ui::wide() {
            (
                div().flex().gap(px(16.)).h(px(300.)).child(div().flex_1().min_w_0().flex().child(chart)).child(div().flex_none().w(px(SIDE_W)).flex().child(perf)),
                div().flex().gap(px(16.)).flex_1().min_h(px(300.)).child(div().flex_1().min_w_0().flex().child(table)).child(div().flex_none().w(px(SIDE_W)).flex().child(activity)),
            )
        } else {
            (
                div().flex().flex_col().gap(px(16.)).child(div().h(px(300.)).flex().child(chart)).child(div().h(px(300.)).flex().child(perf)),
                div().flex().flex_col().gap(px(16.)).child(table).child(activity),
            )
        };
        pages::page("home-scroll", head, div().flex_1().flex().flex_col().gap(px(16.)).child(tiles).child(middle).child(bottom))
    }

    /// The four totals across the top.
    fn tiles(&self, rows: &[(String, PathBuf, Status)]) -> Div {
        let running: Vec<&(String, PathBuf, Status)> = rows.iter().filter(|(_, _, s)| s.running()).collect();
        let players: usize = running.iter().map(|(_, _, s)| s.players.len()).sum();
        let hour = self.player_series(None, Range::Hour);
        let day = self.player_series(None, Range::Day);
        let peak_today = day.iter().map(|s| s.1).fold(0., f32::max).max(players as f32);
        let tile = |label: &'static str, badge: Div, value: Div, foot: Div| {
            ui::plate(radius::PANEL)
                .relative()
                .flex()
                .flex_col()
                .flex_1()
                .min_w_0()
                .px(px(18.))
                .pt(px(16.))
                .pb(px(12.))
                .child(div().flex().items_center().justify_between().child(ui::text(13., FontWeight::MEDIUM, ink::QUIET).child(label)).child(badge))
                .child(value.mt(px(6.)))
                .child(foot.mt(px(4.)))
        };
        let pill = |text: String, c: u32| div().px(px(7.)).py(px(1.)).rounded_full().bg(ui::color(c).opacity(0.13)).child(ui::text(11.5, FontWeight::SEMIBOLD, ink::FULL).text_color(ui::color(c)).child(text));
        let big = |v: String, unit: &'static str| div().flex().items_end().gap(px(6.)).child(ui::heading(30.).child(v)).when(!unit.is_empty(), |d| d.child(ui::text(16., FontWeight::MEDIUM, ink::QUIET).mb(px(4.)).child(unit)));
        let foot = |note: String, extra: Option<gpui::AnyElement>| {
            div().flex().items_end().justify_between().gap(px(10.)).h(px(30.)).child(ui::text(12., FontWeight::NORMAL, ink::QUIET).truncate().child(note)).children(extra)
        };
        // Players online.
        let t1 = tile(
            "Players online",
            pill(format!("peak {} today", peak_today as u32), ui::LIVE),
            big(players.to_string(), ""),
            foot(
                if running.is_empty() { "No server running".into() } else { format!("on {} running server{}", running.len(), if running.len() == 1 { "" } else { "s" }) },
                Some(chart::spark(hour.iter().map(|s| s.1).collect(), ui::color(ui::ACCENT), 110., 30., true, None).into_any_element()),
            ),
        );
        // Servers online, one dash per server.
        let mut dashes = div().flex().gap(px(4.));
        for (_, _, s) in rows.iter().take(12) {
            let c = match server_tone(s) {
                Tone::Live => ui::color(ui::LIVE),
                Tone::Busy => ui::color(ui::WARN),
                _ => white(0.18),
            };
            dashes = dashes.child(div().w(px(12.)).h(px(6.)).rounded(px(3.)).bg(c));
        }
        let stopped = rows.len() - running.len();
        let t2 = tile(
            "Servers online",
            ui::chip(format!("{} live", running.len()), Some(if running.is_empty() { Tone::Off } else { Tone::Live })),
            div().flex().items_end().gap(px(6.)).child(ui::heading(30.).child(running.len().to_string())).child(ui::heading(20.).text_color(white(ink::QUIET)).mb(px(3.)).child(format!("/ {}", rows.len()))),
            foot(if stopped == 0 { "All running".into() } else { format!("{stopped} stopped") }, Some(dashes.into_any_element())),
        );
        // Health: the average, and the slowest when one is behind.
        let tps: Vec<(f64, &str)> = running.iter().filter_map(|(n, _, s)| s.tps.map(|t| (t, n.as_str()))).collect();
        let t3 = if tps.is_empty() {
            tile("Server health", div(), big("–".into(), ""), foot("Start a server to see it".into(), None))
        } else {
            let avg = tps.iter().map(|t| t.0).sum::<f64>() / tps.len() as f64;
            let slow: Vec<&(f64, &str)> = tps.iter().filter(|t| t.0 < 19.5).collect();
            let worst = tps.iter().cloned().fold((f64::MAX, ""), |a, b| if b.0 < a.0 { b } else { a });
            let badge = if slow.is_empty() { pill("Smooth".into(), ui::LIVE) } else { pill(format!("{} need{} a look", slow.len(), if slow.len() == 1 { "s" } else { "" }), ui::WARN) };
            let note = if slow.is_empty() { "20 TPS is full speed".to_string() } else { format!("{} at {:.1} · 20 is full", worst.1, worst.0) };
            let mut avg_hist: Vec<f32> = vec![];
            for (_, d, _) in running.iter() {
                if let Some(h) = self.history.get(d) {
                    let take = h.tps.len().min(60);
                    let tail = &h.tps[h.tps.len() - take..];
                    if avg_hist.is_empty() {
                        avg_hist = tail.to_vec();
                    } else {
                        for (a, b) in avg_hist.iter_mut().rev().zip(tail.iter().rev()) {
                            *a = (*a + b) / 2.;
                        }
                    }
                }
            }
            tile("Server health", badge, big(format!("{avg:.1}"), "TPS avg"), foot(note, Some(chart::spark(avg_hist, ui::color(ui::LIVE), 96., 30., false, Some((14., 20.5))).into_any_element())))
        };
        // Memory.
        let memory: u64 = running.iter().filter_map(|(_, _, s)| s.ram_bytes).sum();
        let largest = running.iter().filter_map(|(n, _, s)| s.ram_bytes.map(|b| (b, n.clone()))).max_by_key(|(b, _)| *b);
        let total = self.pc.map(|p| p.memory_total).unwrap_or(0);
        let frac = if total > 0 { memory as f32 / total as f32 } else { 0. };
        let t4 = tile(
            "Memory in use",
            ui::mono(11.5, ink::QUIET).child(if total > 0 { format!("of {:.0} GB on this PC", total as f64 / 1e9) } else { String::new() }),
            big(if running.is_empty() { "–".into() } else { format!("{:.1}", memory as f64 / 1e9) }, if running.is_empty() { "" } else { "GB" }),
            div()
                .flex()
                .flex_col()
                .gap(px(6.))
                .h(px(30.))
                .justify_end()
                .child(div().h(px(6.)).w_full().rounded(px(3.)).bg(white(0.08)).child(div().h_full().w(gpui::relative(frac.clamp(0., 1.))).rounded(px(3.)).bg(ui::color(SKY))))
                .child(ui::text(12., FontWeight::NORMAL, ink::QUIET).truncate().child(match largest {
                    Some((b, n)) => format!("largest: {n} {:.1} GB", b as f64 / 1e9),
                    None => "Across running servers".into(),
                })),
        );
        div().flex().flex_none().gap(px(16.)).child(t1).child(t2).child(t3).child(t4)
    }

    /// Players over time: the server and range, the numbers, the chart with its hover readout.
    fn players_panel(&mut self, cx: &mut Context<Self>) -> Div {
        let range = self.pages.range;
        let server = self.pages.chart_server.clone().filter(|d| self.list.iter().any(|e| &e.dir == d));
        let series = self.player_series(server.as_deref(), range);
        let t = now();
        // The numbers: one server's own, or every server's records together.
        let summary = match &server {
            Some(d) => self.stats.get(d).map(|s| s.summary(range, t)).unwrap_or_default(),
            None => {
                let mut sum = horizon_launcher::stats::Summary::default();
                let (mut time, mut sessions) = (0u64, 0u64);
                for s in self.stats.values() {
                    let one = s.summary(range, t);
                    sum.different += one.different;
                    if let Some(a) = one.average_session {
                        let n = s.players.values().filter(|p| p.last_seen >= t.saturating_sub(range.seconds())).map(|p| p.sessions as u64).sum::<u64>();
                        time += a * n;
                        sessions += n;
                    }
                }
                sum.average_session = (sessions > 0).then(|| time / sessions);
                sum
            }
        };
        let peak = series.iter().cloned().fold(None, |b: Option<(u64, f32)>, s| if b.is_some_and(|b| b.1 >= s.1) { b } else { Some(s) }).filter(|p| p.1 > 0.);
        let live: Vec<&(u64, f32)> = series.iter().filter(|s| s.1 > 0.).collect();
        let average = if live.is_empty() { None } else { Some(series.iter().map(|s| s.1).sum::<f32>() / series.len() as f32) };
        let busiest = summary.busiest_hour.or_else(|| {
            let mut by = [(0f32, 0u32); 24];
            for (ts, v) in &series {
                let h: u32 = local(*ts, "%H").parse().unwrap_or(0);
                by[h as usize].0 += v;
                by[h as usize].1 += 1;
            }
            by.iter().enumerate().filter(|(_, (s, _))| *s > 0.).max_by(|a, b| (a.1 .0 / a.1 .1 as f32).total_cmp(&(b.1 .0 / b.1 .1 as f32))).map(|(h, _)| h as u32)
        });
        let fig = |v: String, l: &'static str| div().flex().flex_col().child(ui::heading(18.).child(v)).child(ui::text(11.5, FontWeight::NORMAL, ink::QUIET).child(l));
        let hour_label = |h: u32| {
            let a = |h: u32| format!("{}{}", if h % 12 == 0 { 12 } else { h % 12 }, if h < 12 { " AM" } else { " PM" });
            format!("{} – {}", a(h).replace(" AM", "").replace(" PM", ""), a((h + 1) % 24))
        };
        let figures = div()
            .flex()
            .gap(px(28.))
            .mt(px(10.))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .flex()
                            .items_end()
                            .gap(px(6.))
                            .child(ui::heading(18.).child(peak.map(|p| chart::thousands(p.1.round())).unwrap_or_else(|| "–".into())))
                            .children(peak.map(|p| ui::text(12., FontWeight::NORMAL, ink::QUIET).mb(px(2.)).child(format!("at {}", local(p.0, if range == Range::Hour || range == Range::Day { "%-I:%M %p" } else { "%b %-d" }))))),
                    )
                    .child(ui::text(11.5, FontWeight::NORMAL, ink::QUIET).child("Peak")),
            )
            .child(fig(average.map(|a| format!("{a:.1}")).unwrap_or_else(|| "–".into()), "Average online"))
            .child(fig(if summary.different > 0 { chart::thousands(summary.different as f32) } else { "–".into() }, "Different players"))
            .child(fig(busiest.map(hour_label).unwrap_or_else(|| "–".into()), "Busiest hour"))
            .child(fig(summary.average_session.map(ui::span).unwrap_or_else(|| "–".into()), "Average session"));
        // The server and the range.
        let picker = self.server_picker("chart-server", server.clone(), true, cx, |this, d, _| {
            this.pages.chart_server = d;
            this.pages.hover = None;
        });
        let range_tabs = ui::tabs("chart-range", &[(Range::Hour, "1 h"), (Range::Day, "24 h"), (Range::Week, "7 d"), (Range::Month, "30 d")], range, {
            let e = cx.entity();
            move |r, _, cx| {
                e.update(cx, |this, cx| {
                    this.pages.range = r;
                    this.pages.hover = None;
                    cx.notify();
                })
            }
        });
        // The chart, its time labels and the readout under the mouse.
        let hover = self.pages.hover.filter(|i| *i < series.len());
        let spot = self.pages.spot.clone();
        let plot = chart::Players { samples: series.clone(), color: ui::color(ui::ACCENT), height: 150., hover, spot: spot.clone() }.element();
        let n = series.len();
        let first = series.first().map(|s| s.0).unwrap_or(t);
        let mut labels = div().flex().justify_between().mr(px(46.)).mt(px(6.));
        for k in 0..7 {
            let ts = first + (t - first) * k / 6;
            let fmt = match range {
                Range::Hour => "%-I:%M",
                Range::Day => "%-I %p",
                Range::Week => "%a",
                Range::Month => "%b %-d",
            };
            labels = labels.child(ui::mono(10.5, ink::FAINT).child(if k == 6 { "now".to_string() } else { local(ts, fmt) }));
        }
        let tip = hover.and_then(|i| {
            let b = spot.get()?;
            let (ts, v) = series[i];
            let frac = i as f32 / (n.max(2) - 1) as f32;
            let x = f32::from(b.size.width) * frac;
            let when = local(ts, if range == Range::Hour || range == Range::Day { "Today · %-I:%M %p" } else { "%a %b %-d · %-I %p" });
            let tip = div()
                .absolute()
                .top(px(4.))
                .px(px(10.))
                .py(px(8.))
                .rounded(px(radius::CONTROL))
                .bg(ui::color(0x0c0a1c).opacity(0.94))
                .border_1()
                .border_color(white(0.1))
                .shadow_lg()
                .child(ui::text(11., FontWeight::NORMAL, ink::QUIET).child(when))
                .child(div().flex().items_end().gap(px(5.)).child(ui::heading(15.).child(chart::thousands(v.round()))).child(ui::text(12., FontWeight::NORMAL, ink::FULL).child(if v.round() == 1. { "player online" } else { "players online" })));
            // Beside the marker, on whichever side has room.
            let width = 150. * ui::scale();
            Some(if frac > 0.55 { tip.left(gpui::px((x - width - 14.).max(0.))) } else { tip.left(gpui::px(x + 14.)) })
        });
        let empty = live.is_empty();
        let chart_box = div()
            .id("players-chart")
            .relative()
            .mt(px(14.))
            .child(plot)
            .child(labels)
            .children(tip)
            .when(empty, |d| {
                d.child(
                    div()
                        .absolute()
                        .top(px(50.))
                        .left_0()
                        .right(px(46.))
                        .flex()
                        .justify_center()
                        .child(ui::text(13., FontWeight::NORMAL, ink::QUIET).child("No players yet in this range. It fills in while servers run with Horizon Host open.")),
                )
            })
            .on_mouse_move(cx.listener(move |this, ev: &MouseMoveEvent, _, cx| {
                let Some(b) = this.pages.spot.get() else { return };
                let x = f32::from(ev.position.x - b.origin.x) / f32::from(b.size.width).max(1.);
                let i = if (0. ..=1.).contains(&x) && n > 1 { Some((x * (n - 1) as f32).round() as usize) } else { None };
                if this.pages.hover != i {
                    this.pages.hover = i;
                    cx.notify();
                }
            }))
            .on_hover(cx.listener(|this, inside: &bool, _, cx| {
                if !*inside {
                    this.pages.hover = None;
                    cx.notify();
                }
            }));
        ui::panel()
            .flex_1()
            .min_w_0()
            .child(div().flex().items_center().justify_between().gap(px(10.)).child(ui::heading(17.).child("Players over time")).child(div().flex().items_center().gap(px(8.)).child(picker).child(range_tabs)))
            .child(figures)
            .child(chart_box)
    }

    /// Every server's ticks and memory, slowest first; scrolls with many servers.
    fn performance_panel(&mut self, rows: &[(String, PathBuf, Status)], cx: &mut Context<Self>) -> Div {
        let mut sorted: Vec<&(String, PathBuf, Status)> = rows.iter().collect();
        sorted.sort_by(|a, b| {
            let key = |s: &Status| if s.running() { s.tps.unwrap_or(20.) } else { 100. };
            key(&a.2).total_cmp(&key(&b.2))
        });
        let needs: usize = rows.iter().filter(|(_, _, s)| s.running() && s.tps.is_some_and(|t| t < 19.5)).count();
        let issues = self.pages.perf_issues;
        let filter = ui::tabs("perf-filter", &[(false, "All"), (true, "Needs a look")], issues, {
            let e = cx.entity();
            move |v, _, cx| {
                e.update(cx, |this, cx| {
                    this.pages.perf_issues = v;
                    cx.notify();
                })
            }
        });
        let grid = |a: Div, b: Div, c: Div, d: Div, e: Div| {
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .child(a.flex_none().w(px(12.)))
                .child(b.flex_1().min_w_0())
                .child(c.flex_none().w(px(42.)))
                .child(d.flex_none().w(px(60.)))
                .child(e.flex_none().w(px(46.)))
        };
        let mut list = div().id("perf-list").flex().flex_col().flex_1().min_h_0().overflow_y_scroll();
        let mut shown = 0;
        for (name, _, s) in sorted {
            let slow = s.running() && s.tps.is_some_and(|t| t < 19.5);
            if issues && !slow {
                continue;
            }
            shown += 1;
            let tone = server_tone(s);
            let (tps, color) = match s.tps.filter(|_| s.running()) {
                Some(t) => (format!("{t:.1}"), ui::color(health(t).2)),
                None => ("–".into(), white(ink::QUIET)),
            };
            let tick = s.mspt.filter(|_| s.running()).map(|m| (m / 50.).clamp(0., 1.) as f32);
            let bar = div().h(px(5.)).w_full().rounded(px(3.)).bg(white(0.08)).children(tick.map(|f| div().h_full().w(gpui::relative(f.max(0.03))).rounded(px(3.)).bg(color)));
            list = list.child(
                grid(
                    div().child(ui::dot(tone)),
                    ui::text(12.5, FontWeight::NORMAL, if s.running() { ink::FULL } else { ink::QUIET }).truncate().child(name.clone()),
                    div().flex().justify_end().child(ui::mono(11.5, ink::FULL).text_color(color).child(tps)),
                    bar,
                    div().flex().justify_end().child(ui::mono(11.5, if s.running() { ink::FULL } else { ink::QUIET }).child(s.ram_bytes.filter(|_| s.running()).map(|b| ui::bytes(b).replace(" GB", "G").replace(" MB", "M")).unwrap_or_else(|| "–".into()))),
                )
                .h(px(29.))
                .border_b_1()
                .border_color(white(0.06)),
            );
        }
        if shown == 0 {
            list = list.child(ui::text(12.5, FontWeight::NORMAL, ink::QUIET).py(px(10.)).child(if issues { "Every running server is at full speed." } else { "No servers yet." }));
        }
        ui::panel()
            .flex_1()
            .px(px(16.))
            .py(px(18.))
            .child(div().flex().items_center().justify_between().child(panel_title("Performance", Some(rows.len()))).child(filter))
            .child(grid(div(), ui::th("SERVER"), div().flex().justify_end().child(ui::th("TPS")), ui::th("TICK"), div().flex().justify_end().child(ui::th("MEM"))).mt(px(12.)).pb(px(6.)).border_b_1().border_color(white(0.08)))
            .child(list)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .mt(px(8.))
                    .child(ui::icon("tick-02", 13., ui::color(ui::LIVE)))
                    .child(ui::text(11.5, FontWeight::NORMAL, ink::QUIET).child(if needs == 0 { "Slowest first · tick bar to 50 ms".to_string() } else { format!("Slowest first · {needs} below full speed") })),
            )
    }

    /// The servers as a table; a row picks the chart's server.
    fn servers_table(&mut self, rows: &[(String, PathBuf, Status)], cx: &mut Context<Self>) -> Div {
        let selected = self.pages.chart_server.clone();
        let cols = |a: Div, b: Div, c: Div, d: Div, e: Div, f: Div, g: Div| {
            div()
                .flex()
                .items_center()
                .gap(px(12.))
                .child(a.flex_1().min_w_0())
                .child(b.flex_none().w(px(138.)))
                .child(c.flex_none().w(px(70.)))
                .child(d.flex_none().w(px(50.)))
                .child(e.flex_none().w(px(66.)))
                .child(f.flex_none().w(px(160.)))
                .child(g.flex_none().w(px(190.)))
        };
        let mut list = div().id("dash-table").flex().flex_col().flex_1().min_h_0().overflow_y_scroll();
        for (i, (name, dir, st)) in rows.iter().enumerate() {
            let busy = self.busy.get(dir).cloned();
            let tone = server_tone(st);
            let state = match (&busy, st.phase.as_str()) {
                (Some(b), _) => format!("{b}…"),
                (_, "running") if tone == Tone::Busy => "Online · busy".to_string(),
                (_, "running") => match st.uptime {
                    Some(s) => format!("Online · {}", ui::span(s)),
                    None => "Online".into(),
                },
                (_, "stopped") => "Offline".into(),
                (_, p) => crate::wizard::cap(p),
            };
            let address = self.share_address(dir);
            let copied = self.copied(&address);
            let (tps, tps_c) = match st.tps.filter(|_| st.running()) {
                Some(t) => (format!("{t:.1}"), ui::color(health(t).2)),
                None => ("–".into(), white(ink::QUIET)),
            };
            let mode = self.configs.get(dir).and_then(|c| c.pointer("/world/game_mode").and_then(|v| v.as_str()).map(crate::wizard::cap)).unwrap_or_default();
            let sel = selected.as_ref() == Some(dir);
            let sub = match (mode.is_empty(), sel) {
                (true, true) => "in the chart".to_string(),
                (false, true) => format!("{mode} · in the chart"),
                (_, false) => mode,
            };
            let mut actions = div().flex().items_center().justify_end().gap(px(6.));
            if busy.is_some() {
                actions = actions.child(ui::chip("Working", Some(Tone::Busy)));
            } else if st.running() {
                let (d1, d2) = (dir.clone(), dir.clone());
                actions = actions
                    .child(ui::square(SharedString::from(format!("dt-restart-{i}")), "refresh", 32.).on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        cx.stop_propagation();
                        this.restart(d1.clone(), cx)
                    })))
                    .child(ui::square(SharedString::from(format!("dt-stop-{i}")), "stop", 32.).on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        cx.stop_propagation();
                        this.stop(d2.clone(), cx)
                    })));
            } else {
                let d1 = dir.clone();
                actions = actions.child(
                    ui::sized_button(SharedString::from(format!("dt-start-{i}")), "", Kind::Primary, 32., 13.)
                        .child(ui::icon("play", 12., gpui::white()))
                        .child("Start")
                        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                            cx.stop_propagation();
                            this.start(d1.clone(), cx)
                        })),
                );
            }
            let d_open = dir.clone();
            actions = actions.child(
                ui::sized_button(SharedString::from(format!("dt-manage-{i}")), "", Kind::Secondary, 32., 13.)
                    .child("Manage")
                    .child(ui::icon("arrow-right-01", 13., white(ink::FULL)))
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        cx.stop_propagation();
                        this.open_server(d_open.clone(), cx)
                    })),
            );
            let text = address.clone();
            let d_sel = dir.clone();
            list = list.child(
                cols(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(10.))
                        .child(div().flex().flex_none().items_center().justify_center().size(px(32.)).rounded(px(radius::CONTROL)).bg(if st.running() { ui::color(ui::ACCENT).opacity(0.22) } else { white(0.07) }).child(ui::icon("computer-terminal-01", 15., white(ink::FULL))))
                        .child(div().flex().flex_col().min_w_0().child(ui::text(14., FontWeight::SEMIBOLD, ink::FULL).truncate().child(name.clone())).when(!sub.is_empty(), |d| d.child(ui::text(11.5, FontWeight::NORMAL, ink::QUIET).truncate().child(sub.clone())))),
                    div().flex().items_center().gap(px(7.)).child(ui::dot(tone)).child(ui::text(13., FontWeight::NORMAL, if st.running() { ink::FULL } else { ink::QUIET }).truncate().child(state)),
                    ui::text(13.5, FontWeight::NORMAL, if st.running() { ink::FULL } else { ink::QUIET }).child(match st.max_players {
                        Some(m) if st.running() => format!("{} / {m}", st.players.len()),
                        _ => "–".into(),
                    }),
                    ui::text(13.5, FontWeight::SEMIBOLD, ink::FULL).text_color(tps_c).child(tps),
                    ui::text(13.5, FontWeight::NORMAL, if st.running() { ink::FULL } else { ink::QUIET }).child(st.ram_bytes.filter(|_| st.running()).map(ui::bytes).unwrap_or_else(|| "–".into())),
                    div().min_w_0().child(div()
                        .id(SharedString::from(format!("dt-copy-{i}")))
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .min_w_0()
                        .cursor_pointer()
                        .hover(|s| s.opacity(0.8))
                        .child(ui::mono(12., ink::QUIET).truncate().child(address))
                        .child(ui::icon(if copied { "tick-02" } else { "copy-01" }, 12., white(if copied { ink::FULL } else { ink::QUIET })))
                        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                            cx.stop_propagation();
                            this.copy(text.clone(), cx)
                        }))),
                    actions,
                )
                .id(SharedString::from(format!("dt-row-{i}")))
                .px(px(12.))
                .py(px(9.))
                .rounded(px(radius::ROW))
                .cursor_pointer()
                .when(sel, |d| d.bg(white(fill::HOVER)))
                .when(!sel, |d| d.hover(|s| s.bg(white(0.03))))
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    this.pages.chart_server = if this.pages.chart_server.as_ref() == Some(&d_sel) { None } else { Some(d_sel.clone()) };
                    this.pages.hover = None;
                    cx.notify();
                })),
            );
        }
        let running = rows.iter().filter(|(_, _, s)| s.running()).count();
        let view_all = ui::sized_button("dash-view-all", "", Kind::Secondary, 30., 12.5)
            .child("View all")
            .child(ui::icon("arrow-right-01", 13., white(ink::FULL)))
            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.open_page(Page::Servers, cx)));
        ui::panel()
            .flex_1()
            .min_w_0()
            .px(px(20.))
            .py(px(18.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .mb(px(8.))
                    .child(div().flex().items_center().gap(px(10.)).child(ui::heading(17.).child("Servers")).child(ui::text(12., FontWeight::NORMAL, ink::QUIET).child("Pick one to see it in the chart")))
                    .child(div().flex().items_center().gap(px(8.)).child(ui::chip(format!("{running} running · {} total", rows.len()), Some(if running > 0 { Tone::Live } else { Tone::Off }))).child(view_all)),
            )
            .child(cols(ui::th("SERVER"), ui::th("STATUS"), ui::th("PLAYERS"), ui::th("TPS"), ui::th("MEMORY"), ui::th("ADDRESS"), div()).px(px(12.)).pb(px(9.)).border_b_1().border_color(white(0.08)))
            .child(list)
    }

    /// What happened lately: servers going on and off, players coming and going.
    fn activity_panel(&self) -> Div {
        let mut list = div().flex().flex_col().w_full().min_w_0().mt(px(6.));
        if self.activity.is_empty() {
            list = list.child(div().w_full().child(ui::text(13., FontWeight::NORMAL, ink::QUIET).w_full().child("Nothing yet. Servers starting and stopping and players joining show up here.")));
        }
        for (time, line, tone) in self.activity.iter().rev().take(9) {
            list = list.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .py(px(7.))
                    .child(ui::dot(*tone))
                    .child(div().flex_1().min_w_0().child(ui::text(13., FontWeight::NORMAL, ink::FULL).truncate().child(line.clone())))
                    .child(ui::mono(11., ink::FAINT).child(time.clone())),
            );
        }
        ui::panel().flex_1().min_w_0().overflow_hidden().child(ui::heading(17.).child("Activity")).child(list)
    }

    /// What changed on a server since the last look, for the activity feed.
    pub fn note_changes(&mut self, dir: &Path, new: &Status) {
        let Some(old) = self.status.get(dir) else { return };
        let name = self.name_of(dir);
        let name = if name.is_empty() { "A server".to_string() } else { name };
        let mut lines: Vec<(String, Tone)> = vec![];
        if old.phase != new.phase {
            // A fresh start has picked up the plugin changes that waited for it.
            if new.running() {
                self.pages.pending.remove(dir);
            }
            match new.phase.as_str() {
                "running" => lines.push((format!("{name} is online"), Tone::Live)),
                "stopped" => lines.push((format!("{name} stopped"), Tone::Off)),
                "starting" => lines.push((format!("{name} is starting"), Tone::Busy)),
                "restarting" => lines.push((format!("{name} is restarting"), Tone::Busy)),
                _ => {}
            }
        }
        if new.crashes > old.crashes {
            lines.push((format!("{name} crashed and came back"), Tone::Bad));
        }
        if let (Some(a), Some(b)) = (old.tps.filter(|_| old.running()), new.tps.filter(|_| new.running())) {
            if a >= 19.5 && b < 18.5 {
                lines.push((format!("{name} slowed to {b:.1} TPS"), Tone::Busy));
            }
        }
        if old.running() && new.running() {
            for p in new.players.iter().filter(|p| !old.players.iter().any(|o| o.uuid == p.uuid)) {
                lines.push((format!("{} joined {name}", p.name), Tone::Live));
            }
            for p in old.players.iter().filter(|p| !new.players.iter().any(|n| n.uuid == p.uuid)) {
                lines.push((format!("{} left {name}", p.name), Tone::Off));
            }
        }
        let time = chrono::Local::now().format("%-I:%M %p").to_string();
        for (line, tone) in lines {
            self.activity.push_back((time.clone(), line, tone));
        }
        while self.activity.len() > 60 {
            self.activity.pop_front();
        }
    }

    fn welcome(&mut self, cx: &mut Context<Self>) -> Div {
        let step = |n: &'static str, title: &'static str, note: &'static str| {
            div()
                .flex()
                .flex_col()
                .flex_1()
                .gap(px(6.))
                .p(px(18.))
                .rounded(px(radius::ROW))
                .bg(white(0.05))
                .child(ui::mono(12., ink::QUIET).child(n))
                .child(ui::text(16., FontWeight::SEMIBOLD, ink::FULL).child(title))
                .child(ui::text(13., FontWeight::NORMAL, ink::QUIET).child(note))
        };
        div().flex_1().flex().items_center().justify_center().px(px(32.)).child(
            ui::panel()
                .w_full()
                .max_w(px(820.))
                .p(px(44.))
                .child(ui::mono(12., ink::QUIET).child("HORIZON HOST"))
                .child(ui::display(46.).mt(px(4.)).child("Your own Minecraft server, in a few clicks"))
                .child(ui::text(16., FontWeight::NORMAL, ink::BODY).mt(px(10.)).child(
                    "Runs on HorizonsX, Horizon's fast server, right on this PC. Friends join from anywhere: no port forwarding, no commands.",
                ))
                .child(
                    div()
                        .flex()
                        .gap(px(12.))
                        .mt(px(28.))
                        .child(step("01", "Choose", "The server type and plugins"))
                        .child(step("02", "Set up", "A new world or one of yours"))
                        .child(step("03", "Share", "Send friends one address")),
                )
                .child(
                    div().flex().mt(px(30.)).child(
                        crate::demo::mark("welcome-start", ui::sized_button("welcome-start", "Make a server", Kind::Primary, 44., 15.).relative())
                            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.open_wizard(cx))),
                    ),
                ),
        )
    }
}

#[allow(unused)]
fn _keep(_: &Path) {}
