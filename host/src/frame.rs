//! The sidebar every page sits beside: Horizon's logo, the pages, the servers with their state, this PC's
//! processor and memory, and New server.
use crate::app::{HostApp, View};
use crate::ui::{self, px, white, Tone};
use gpui::{div, prelude::*, ClickEvent, Context, Div, FontWeight, SharedString};
use horizon_theme::glass::{fill, ink, radius};

/// The pages in the sidebar.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Page {
    Dashboard,
    Servers,
    Players,
    Plugins,
    Network,
    Backups,
    Schedules,
    Settings,
}

const PAGES: [(Page, &str, &str); 8] = [
    (Page::Dashboard, "Dashboard", "dashboard-square-01"),
    (Page::Servers, "Servers", "computer"),
    (Page::Players, "Players", "user-multiple-02"),
    (Page::Plugins, "Plugins", "puzzle"),
    (Page::Network, "Network", "link-01"),
    (Page::Backups, "Backups", "download-04"),
    (Page::Schedules, "Schedules", "refresh"),
    (Page::Settings, "Settings", "sliders-horizontal"),
];

/// How many servers the sidebar lists before "N more".
const LISTED: usize = 6;

/// Horizon's logo: the H split by the violet horizon line (packaging/icons/.../horizon.svg), drawn in blocks.
pub fn logo(size: f32) -> Div {
    let u = size / 16.;
    let block = |x: f32, y: f32, w: f32, h: f32, c: gpui::Hsla| div().absolute().left(px(x * u)).top(px(y * u)).w(px(w * u)).h(px(h * u)).bg(c);
    let light = ui::color(0xecedf3);
    div()
        .relative()
        .flex_none()
        .size(px(size))
        .rounded(px(3.5 * u))
        .overflow_hidden()
        .bg(ui::color(0x16181f))
        .child(block(4., 3., 2., 4., light))
        .child(block(10., 3., 2., 4., light))
        .child(block(4., 8., 2., 4., light.opacity(0.36)))
        .child(block(10., 8., 2., 4., light.opacity(0.36)))
        .child(block(0., 7., 16., 1., ui::color(0x9a86ff)))
}

impl HostApp {
    pub fn page(&self) -> Option<Page> {
        match &self.view {
            View::Page(p) => Some(*p),
            View::Server(_) => Some(Page::Servers),
            View::Wizard | View::Setup => None,
        }
    }

    pub fn open_page(&mut self, p: Page, cx: &mut Context<Self>) {
        self.list = horizon_launcher::servers::list();
        self.view = View::Page(p);
        self.pages.menu = None;
        self.page_opened(p);
        cx.notify();
    }

    pub fn sidebar(&mut self, cx: &mut Context<Self>) -> Div {
        let current = self.page();
        let mut nav = div().flex().flex_col().gap(px(3.));
        for (page, label, icon) in PAGES {
            let on = current == Some(page);
            let count = match page {
                Page::Servers => Some(self.list.len()),
                Page::Players => Some(self.status.values().filter(|s| s.running()).map(|s| s.players.len()).sum()),
                _ => None,
            };
            nav = nav.child(
                div()
                    .id(SharedString::from(format!("nav-{label}")))
                    .flex()
                    .items_center()
                    .gap(px(11.))
                    .h(px(36.))
                    .px(px(10.))
                    .rounded(px(radius::CONTROL))
                    .cursor_pointer()
                    .bg(white(if on { fill::SELECTED } else { 0. }))
                    .hover(|s| s.bg(white(if on { fill::SELECTED } else { fill::HOVER })))
                    .child(ui::icon(icon, 16., white(if on { ink::FULL } else { ink::NOTE })))
                    .child(ui::text(13.5, FontWeight::MEDIUM, if on { ink::FULL } else { ink::BODY }).child(label))
                    .children(count.filter(|n| *n > 0).map(|n| div().ml_auto().child(ui::mono(11., ink::QUIET).child(n.to_string()))))
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.open_page(page, cx))),
            );
        }
        // The servers, running first.
        let mut servers: Vec<_> = self.list.iter().map(|e| (e.name.clone(), e.dir.clone(), self.status_of(&e.dir))).collect();
        servers.sort_by_key(|(_, _, s)| !s.running());
        let stopped = servers.iter().filter(|(_, _, s)| !s.running()).count();
        let open = match &self.view {
            View::Server(d) => Some(d.clone()),
            _ => None,
        };
        let mut list = div().flex().flex_col().gap(px(2.));
        for (i, (name, dir, st)) in servers.iter().enumerate().take(LISTED) {
            let tone = server_tone(st);
            let players = match st.max_players {
                Some(max) if st.running() => format!("{}/{max}", st.players.len()),
                _ => "off".into(),
            };
            let sel = open.as_ref() == Some(dir);
            let d = dir.clone();
            list = list.child(
                div()
                    .id(SharedString::from(format!("side-srv-{i}")))
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .h(px(30.))
                    .px(px(10.))
                    .rounded(px(radius::CONTROL))
                    .cursor_pointer()
                    .bg(white(if sel { fill::HOVER + 0.01 } else { 0. }))
                    .hover(|s| s.bg(white(fill::HOVER)))
                    .child(ui::dot(tone))
                    .child(div().flex_1().min_w_0().child(ui::text(13., FontWeight::NORMAL, if st.running() { ink::FULL } else { ink::BODY }).truncate().child(name.clone())))
                    .child(ui::mono(11., ink::QUIET).child(players))
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.open_server(d.clone(), cx))),
            );
        }
        if servers.len() > LISTED {
            let more = servers.len() - LISTED;
            list = list.child(
                div()
                    .id("side-more")
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .h(px(30.))
                    .px(px(10.))
                    .rounded(px(radius::CONTROL))
                    .cursor_pointer()
                    .hover(|s| s.bg(white(fill::HOVER)))
                    .child(div().w(px(8.)))
                    .child(ui::text(13., FontWeight::NORMAL, ink::QUIET).flex_1().child(if stopped >= more { format!("{more} more stopped") } else { format!("{more} more") }))
                    .child(ui::icon("arrow-right-01", 13., white(ink::QUIET)))
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.open_page(Page::Servers, cx))),
            );
        }
        let pc = self.pc;
        let bar = |frac: f32, c: u32| {
            div().h(px(6.)).w_full().rounded(px(3.)).bg(white(0.08)).child(div().h_full().w(gpui::relative(frac.clamp(0., 1.))).rounded(px(3.)).bg(ui::color(c)))
        };
        let (cpu, mem_used, mem_total) = pc.map(|p| (p.cpu, p.memory_used, p.memory_total)).unwrap_or((0., 0, 1));
        // What the servers take together, and what this PC still has for them.
        let running: Vec<&(String, std::path::PathBuf, horizon_launcher::servers::Status)> = servers.iter().filter(|(_, _, s)| s.running()).collect();
        let servers_mem: u64 = running.iter().filter_map(|(_, _, s)| s.ram_bytes).sum();
        let available = mem_total.saturating_sub(mem_used);
        let low_memory = available < 2_000_000_000;
        let (word, tone) = if low_memory {
            ("Low memory", Tone::Bad)
        } else if cpu >= 85. {
            ("Busy", Tone::Busy)
        } else {
            ("Healthy", Tone::Live)
        };
        let line = |label: &'static str, value: String| {
            div().flex().justify_between().gap(px(8.)).child(ui::text(12., FontWeight::NORMAL, ink::BODY).child(label)).child(ui::mono(12., ink::FULL).child(value))
        };
        let this_pc = div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .p(px(12.))
            .rounded(px(radius::ROW))
            .bg(white(0.05))
            .child(div().flex().items_center().justify_between().child(ui::mono(10.5, ink::QUIET).child("THIS PC")).child(ui::chip(word, Some(tone))))
            .child(div().flex().flex_col().gap(px(4.)).child(line("Processor", format!("{cpu:.0}%"))).child(bar(cpu / 100., ui::ACCENT)))
            .child(line(
                "Servers",
                match running.len() {
                    0 => "none running".to_string(),
                    n => format!("{n} running · {}", if servers_mem > 0 { ui::bytes(servers_mem) } else { "–".into() }),
                },
            ))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .child(line("Available memory", format!("{:.1} GB", available as f64 / 1e9)))
                    .child(bar(available as f32 / mem_total as f32, if low_memory { ui::DANGER } else { ui::LIVE }))
                    .child(ui::text(11., FontWeight::NORMAL, ink::QUIET).child(format!("free of {:.0} GB for new servers", mem_total as f64 / 1e9))),
            );
        let new_server = div()
            .id("side-new")
            .flex()
            .items_center()
            .justify_center()
            .gap(px(7.))
            .h(px(38.))
            .rounded(px(radius::CONTROL))
            .bg(ui::color(ui::ACCENT))
            .cursor_pointer()
            .hover(|s| s.opacity(0.9))
            .child(ui::icon("plus-sign", 15., gpui::white()))
            .child(ui::text(13.5, FontWeight::SEMIBOLD, ink::FULL).child("New server"))
            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.open_wizard(cx)));
        ui::plate(radius::PANEL)
            .relative()
            .flex()
            .flex_col()
            .flex_none()
            .gap(px(20.))
            .w(px(244.))
            .h_full()
            .px(px(14.))
            .py(px(18.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .child(logo(34.))
                    .child(div().flex().flex_col().child(ui::heading(16.).child("HorizonX")).child(ui::text(11.5, FontWeight::NORMAL, ink::QUIET).child("Servers on HorizonsX"))),
            )
            .child(nav)
            .when(!servers.is_empty(), |d| d.child(div().flex().flex_col().gap(px(6.)).child(div().px(px(10.)).child(ui::mono(10.5, ink::QUIET).child("YOUR SERVERS"))).child(list)))
            .child(div().mt_auto().flex().flex_col().gap(px(12.)).child(this_pc).child(new_server))
    }
}

/// A server's dot: green running, amber slow or busy, grey stopped.
pub fn server_tone(st: &horizon_launcher::servers::Status) -> Tone {
    if !st.running() {
        Tone::Off
    } else if st.tps.is_some_and(|t| t < 19.5) {
        Tone::Busy
    } else {
        Tone::Live
    }
}
