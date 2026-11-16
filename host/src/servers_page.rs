//! Servers: every server on one calm list, running ones first, one line each (state, players, TPS, memory,
//! uptime, address, Stop or Start, Manage).
use crate::app::HostApp;
use crate::frame::server_tone;
use crate::home::health;
use crate::pages;
use crate::ui::{self, px, white, Kind, Tone};
use gpui::{div, prelude::*, ClickEvent, Context, Div, FontWeight, SharedString};
use horizon_launcher::servers::Status;
use horizon_theme::glass::{ink, radius};
use std::path::PathBuf;

impl HostApp {
    pub fn servers_page(&mut self, cx: &mut Context<Self>) -> Div {
        let rows: Vec<(String, PathBuf, Status)> = self.list.iter().map(|e| (e.name.clone(), e.dir.clone(), self.status_of(&e.dir))).collect();
        let running = rows.iter().filter(|(_, _, s)| s.running()).count();
        let stopped = rows.len() - running;
        let filter = self.pages.servers_filter;
        let labels: [(u8, String); 3] = [(0, format!("All {}", rows.len())), (1, format!("Running {running}")), (2, format!("Stopped {stopped}"))];
        let tabs = ui::tabs("servers-filter", &labels, filter, {
            let e = cx.entity();
            move |v, _, cx| {
                e.update(cx, |this, cx| {
                    this.pages.servers_filter = v;
                    cx.notify();
                })
            }
        });
        let head = pages::header(format!("{} server{} on this PC · {running} running", rows.len(), if rows.len() == 1 { "" } else { "s" }), "Servers", Some(div().child(tabs)));
        if rows.is_empty() {
            return pages::page("servers-scroll", head, ui::panel().child(ui::text(14., FontWeight::NORMAL, ink::QUIET).child("No servers yet. New server, at the bottom left, makes one in a few clicks.")));
        }
        let cols = |a: Div, b: Div, c: Div, d: Div, e: Div, f: Div, g: Div| {
            div()
                .flex()
                .items_center()
                .gap(px(14.))
                .child(a.flex_1().min_w_0())
                .child(b.flex_none().w(px(170.)))
                .child(c.flex_none().w(px(96.)))
                .child(d.flex_none().w(px(70.)))
                .child(e.flex_none().w(px(84.)))
                .child(f.flex_none().w(px(220.)))
                .child(g.flex_none().w(px(130.)))
        };
        let mut body = ui::panel().px(px(16.)).pt(px(14.)).pb(px(8.)).child(
            cols(ui::th("SERVER"), ui::th("STATUS"), ui::th("PLAYERS"), ui::th("TPS"), ui::th("MEMORY"), ui::th("ADDRESS"), div())
                .h(px(34.))
                .px(px(14.))
                .border_b_1()
                .border_color(white(0.08)),
        );
        // One list: running servers first, each line saying its own state (the tabs above count them).
        let mut shown: Vec<(usize, &(String, PathBuf, Status))> = rows
            .iter()
            .enumerate()
            .filter(|(_, (_, _, s))| match filter {
                1 => s.running(),
                2 => !s.running(),
                _ => true,
            })
            .collect();
        shown.sort_by_key(|(_, (n, _, s))| (!s.running(), n.to_lowercase()));
        let count = shown.len();
        if count == 0 {
            body = body.child(ui::text(13., FontWeight::NORMAL, ink::QUIET).px(px(14.)).py(px(14.)).child(if filter == 1 { "No server is running." } else { "Every server is running." }));
        }
        for (k, (i, (name, dir, st))) in shown.into_iter().enumerate() {
            let tone = server_tone(st);
            let mode = self.configs.get(dir).and_then(|c| c.pointer("/world/game_mode").and_then(|v| v.as_str()).map(crate::wizard::cap)).unwrap_or_default();
            let address = self.share_address(dir);
            let copied = self.copied(&address);
            let busy = self.busy.get(dir).cloned();
            let (tps, tps_c) = match st.tps.filter(|_| st.running()) {
                Some(t) => (format!("{t:.1}"), ui::color(health(t).2)),
                None => ("–".into(), white(ink::QUIET)),
            };
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
            let control = if busy.is_some() {
                div().child(ui::chip("Working", Some(Tone::Busy)))
            } else if st.running() {
                let d = dir.clone();
                div().child(ui::square(SharedString::from(format!("sv-stop-{i}")), "stop", 30.).on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.stop(d.clone(), cx))))
            } else {
                let d = dir.clone();
                div().child(
                    div()
                        .id(SharedString::from(format!("sv-start-{i}")))
                        .flex()
                        .items_center()
                        .justify_center()
                        .size(px(30.))
                        .rounded(px(radius::CONTROL))
                        .bg(ui::color(ui::ACCENT))
                        .cursor_pointer()
                        .hover(|s| s.opacity(0.9))
                        .child(ui::icon("play", 13., gpui::white()))
                        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.start(d.clone(), cx))),
                )
            };
            let d_open = dir.clone();
            let text = address.clone();
            let quiet = if st.running() { ink::FULL } else { ink::QUIET };
            body = body.child(
                cols(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(10.))
                        .child(ui::text(14., FontWeight::SEMIBOLD, if st.running() { ink::FULL } else { ink::BODY }).truncate().child(name.clone()))
                        .child(ui::text(12., FontWeight::NORMAL, ink::QUIET).child(mode)),
                    div().flex().items_center().gap(px(8.)).child(ui::dot(tone)).child(ui::text(13., FontWeight::NORMAL, quiet).truncate().child(state)),
                    ui::text(13.5, FontWeight::NORMAL, quiet).child(match st.max_players {
                        Some(m) if st.running() => format!("{} / {m}", st.players.len()),
                        _ => "–".into(),
                    }),
                    ui::text(13.5, FontWeight::SEMIBOLD, ink::FULL).text_color(tps_c).child(tps),
                    ui::text(13.5, FontWeight::NORMAL, quiet).child(st.ram_bytes.filter(|_| st.running()).map(ui::bytes).unwrap_or_else(|| "–".into())),
                    div().min_w_0().child(div()
                        .id(SharedString::from(format!("sv-copy-{i}")))
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .min_w_0()
                        .cursor_pointer()
                        .hover(|s| s.opacity(0.8))
                        .child(ui::mono(12., ink::QUIET).truncate().child(address))
                        .child(ui::icon(if copied { "tick-02" } else { "copy-01" }, 12., white(if copied { ink::FULL } else { ink::QUIET })))
                        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.copy(text.clone(), cx)))),
                    div()
                        .flex()
                        .items_center()
                        .justify_end()
                        .gap(px(6.))
                        .child(control)
                        .child(ui::sized_button(SharedString::from(format!("sv-manage-{i}")), "Manage", Kind::Secondary, 30., 12.5).on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.open_server(d_open.clone(), cx)))),
                )
                .h(px(52.))
                .px(px(14.))
                .when(k + 1 < count, |d| d.border_b_1().border_color(white(0.06))),
            );
        }
        pages::page("servers-scroll", head, body)
    }
}
