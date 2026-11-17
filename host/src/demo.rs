//! Demo mode for showcase videos (`HORIZON_HOST_DEMO=1`): a scripted tour drives the app the way a
//! person would. A cursor glides to each button, presses it (a ripple) and the button's own action
//! runs; fields are typed into a letter at a time. Nothing touches the real mouse or keyboard.
//! Addresses are masked (`mask_ips`), so a recording never shows the machine's IP.
use crate::app::{Field, HostApp, View};
use crate::wizard::Engine;
use gpui::{canvas, div, point, prelude::*, Bounds, Context, Div, Pixels, Point, Window};
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

pub fn on() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var("HORIZON_HOST_DEMO").is_ok_and(|v| v == "1"))
}

thread_local! {
    static TARGETS: RefCell<HashMap<String, Bounds<Pixels>>> = RefCell::new(HashMap::new());
}

/// Records where `el` is drawn under `name`, for the tour to aim at (demo mode only).
pub fn mark<E: ParentElement + Styled>(name: impl Into<String>, el: E) -> E {
    if !on() {
        return el;
    }
    let name = name.into();
    el.child(
        canvas(
            move |bounds, _, _| {
                TARGETS.with(|t| t.borrow_mut().insert(name.clone(), bounds));
            },
            |_, _, _, _| {},
        )
        .absolute()
        .top_0()
        .left_0()
        .size_full(),
    )
}

fn target(name: &str) -> Option<Bounds<Pixels>> {
    TARGETS.with(|t| t.borrow().get(name).copied())
}

/// IPv4 addresses (not loopback) in `text` with their last two numbers hidden.
/// Addresses hidden: in the demo, or with `HORIZON_HOST_MASK=1` (screenshots for docs).
fn masking() -> bool {
    static MASK: OnceLock<bool> = OnceLock::new();
    on() || *MASK.get_or_init(|| std::env::var("HORIZON_HOST_MASK").is_ok_and(|v| v == "1"))
}

pub fn mask_ips(text: &str) -> String {
    if !masking() {
        return text.to_string();
    }
    // A playit address is someone's own: shown as an example.
    if text.ends_with(".ply.gg") || text.contains(".ply.gg:") || text.ends_with(".joinmc.link") {
        return "play.example.gg".to_string();
    }
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        // A dotted quad starts here?
        let start = i;
        let mut parts = 0;
        let mut j = i;
        while j < chars.len() && parts < 4 {
            let s = j;
            while j < chars.len() && chars[j].is_ascii_digit() && j - s < 3 {
                j += 1;
            }
            if j == s {
                break;
            }
            parts += 1;
            if parts < 4 {
                if j < chars.len() && chars[j] == '.' {
                    j += 1;
                } else {
                    break;
                }
            }
        }
        let boundary = start == 0 || !chars[start - 1].is_ascii_alphanumeric();
        if parts == 4 && boundary {
            let quad: String = chars[start..j].iter().collect();
            if quad.starts_with("127.") {
                out.push_str(&quad);
            } else {
                let mut it = quad.split('.');
                out.push_str(&format!("{}.{}.x.x", it.next().unwrap_or(""), it.next().unwrap_or("")));
            }
            i = j;
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }
    hide_claims(&out)
}

/// playit claim codes hidden (`claim/<code>`, `claim code: <code>`): a claim code links the tunnel to
/// whoever opens it first.
fn hide_claims(text: &str) -> String {
    let mut out = text.to_string();
    for key in ["claim/", "claim code: "] {
        let mut from = 0;
        while let Some(at) = out[from..].find(key) {
            let start = from + at + key.len();
            let end = out[start..].find(|c: char| !c.is_ascii_alphanumeric()).map_or(out.len(), |e| start + e);
            from = start;
            if end > start {
                out.replace_range(start..end, "••••••••");
                from = start + "••••••••".len();
            }
            if from >= out.len() {
                break;
            }
        }
    }
    out
}

/// One thing the tour does.
#[derive(Clone)]
enum Act {
    Wait(u64),
    Click(&'static str),
    Type(Field, &'static str),
    /// Until the new server runs and its public-address step is done (or `u64` ms pass).
    UntilSetup(u64),
}

fn script() -> Vec<Act> {
    use Act::*;
    // `HORIZON_HOST_DEMO_SCRIPT=toggles`: flip a server's Friends only switch on and off (a check).
    if std::env::var("HORIZON_HOST_DEMO_SCRIPT").as_deref() == Ok("toggles") {
        return vec![Wait(2500), Click("set-friends"), Wait(3000), Click("set-friends"), Wait(3000)];
    }
    // `HORIZON_HOST_DEMO_SCRIPT=sidebar`: a first server from the welcome (no sidebar while setting up), then the
    // sidebar sliding in when it opens.
    if std::env::var("HORIZON_HOST_DEMO_SCRIPT").as_deref() == Ok("sidebar") {
        let plug = |name: &'static str| -> &'static str { Box::leak(format!("plug-{}", host_plugin(name)).into_boxed_str()) };
        return vec![
            Wait(2500),
            Click("welcome-start"),
            Wait(1300),
            Click("engine-hx"),
            Wait(700),
            Click("wiz-next"),
            Wait(1100),
            Click(plug("luckperms_rs")),
            Wait(400),
            Click(plug("essentials_bundle")),
            Wait(900),
            Click("wiz-next"),
            Wait(1100),
            Click("wiz-next"),
            Wait(1100),
            Click("field-Name"),
            Type(Field::Name, "Friends SMP"),
            Wait(700),
            Click("wiz-next"),
            Wait(2200),
            Click("wiz-create"),
            UntilSetup(90_000),
            Wait(2500),
            Click("setup-open"),
            Wait(4500),
        ];
    }
    vec![
        Wait(2200),
        Click("welcome-start"),
        Wait(1600),
        Click("engine-hx"),
        Wait(900),
        Click("wiz-next"),
        Wait(1300),
        Click("plug-luckperms_rs.dll"),
        Wait(450),
        Click("plug-coreprotect_rs.dll"),
        Wait(450),
        Click("plug-horizonworldedit.dll"),
        Wait(1100),
        Click("wiz-next"),
        Wait(1300),
        Click("field-Seed"),
        Type(Field::Seed, "horizon"),
        Wait(900),
        Click("wiz-next"),
        Wait(1300),
        Click("field-Name"),
        Type(Field::Name, "Showcase SMP"),
        Wait(900),
        Click("toggle-backups-row"),
        Wait(700),
        Click("toggle-backups-row"),
        Wait(900),
        Click("wiz-next"),
        Wait(2600),
        Click("wiz-create"),
        UntilSetup(150_000),
        Wait(3200),
        Click("setup-open"),
        Wait(3000),
        Click("nav-Console"),
        Wait(2600),
        Click("nav-Players"),
        Wait(2400),
        Click("nav-Plugins"),
        Wait(2600),
        Click("nav-Performance"),
        Wait(3200),
        Click("nav-Backups"),
        Wait(2400),
        Click("nav-Schedule"),
        Wait(2400),
        Click("nav-Settings"),
        Wait(2800),
        Click("nav-Files"),
        Wait(2600),
        Click("nav-Overview"),
        Wait(2600),
        Click("back-home"),
        Wait(3500),
    ]
}

#[derive(Clone, Copy, PartialEq)]
enum Phase {
    Idle,
    Moving,
    Pressing,
}

pub struct Tour {
    acts: Vec<Act>,
    i: usize,
    since: Instant,
    phase: Phase,
    cursor: Point<Pixels>,
    from: Point<Pixels>,
    to: Point<Pixels>,
    /// The last press, for its ripple.
    pressed: Option<(Point<Pixels>, Instant)>,
    typed: usize,
    pub done: bool,
}

const MOVE: Duration = Duration::from_millis(750);
const PRESS: Duration = Duration::from_millis(220);
const KEY: Duration = Duration::from_millis(85);

impl Tour {
    pub fn new() -> Tour {
        let p = point(gpui::px(1200.), gpui::px(700.));
        Tour { acts: script(), i: 0, since: Instant::now(), phase: Phase::Idle, cursor: p, from: p, to: p, pressed: None, typed: 0, done: false }
    }
}

fn ease(t: f32) -> f32 {
    // easeInOutCubic
    if t < 0.5 {
        4. * t * t * t
    } else {
        1. - (-2. * t + 2.).powi(3) / 2.
    }
}

impl HostApp {
    /// Advances the tour (each frame while it runs).
    pub fn tour_step(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(mut tour) = self.tour.take() else { return };
        if tour.done {
            self.tour = Some(tour);
            return;
        }
        window.request_animation_frame();
        // `HORIZON_HOST_DEMO_GO=path`: hold still until the recorder makes that file.
        if let Ok(go) = std::env::var("HORIZON_HOST_DEMO_GO") {
            if !std::path::Path::new(&go).exists() {
                tour.since = Instant::now();
                self.tour = Some(tour);
                return;
            }
        }
        let now = Instant::now();
        let el = now - tour.since;
        match tour.acts.get(tour.i).cloned() {
            None => {
                tour.done = true;
                if let Ok(path) = std::env::var("HORIZON_HOST_DEMO_DONE") {
                    let _ = std::fs::write(path, "done");
                }
            }
            Some(Act::Wait(ms)) => {
                if el >= Duration::from_millis(ms) {
                    tour.i += 1;
                    tour.since = now;
                }
            }
            Some(Act::UntilSetup(ms)) => {
                let ready = self.setup.steps.len() == 6 && self.setup.steps.iter().all(|s| s.1 == crate::app::Step::Done);
                if ready || self.setup.error.is_some() || el >= Duration::from_millis(ms) {
                    tour.i += 1;
                    tour.since = now;
                }
            }
            Some(Act::Type(f, text)) => {
                if tour.typed == 0 {
                    self.set_text(f, "");
                }
                let want = (el.as_millis() / KEY.as_millis()) as usize;
                if want > tour.typed && tour.typed < text.chars().count() {
                    tour.typed = want.min(text.chars().count());
                    let s: String = text.chars().take(tour.typed).collect();
                    self.set_text(f, &s);
                }
                if tour.typed >= text.chars().count() && el >= KEY * (text.chars().count() as u32 + 2) {
                    tour.typed = 0;
                    tour.i += 1;
                    tour.since = now;
                }
            }
            Some(Act::Click(name)) => match tour.phase {
                Phase::Idle => {
                    // Wait for the target to be laid out (a page that just opened).
                    if let Some(b) = target(name) {
                        tour.from = tour.cursor;
                        tour.to = b.center();
                        tour.phase = Phase::Moving;
                        tour.since = now;
                    } else if el > Duration::from_secs(8) {
                        tour.i += 1;
                        tour.since = now;
                    }
                }
                Phase::Moving => {
                    let t = (el.as_secs_f32() / MOVE.as_secs_f32()).min(1.);
                    let k = ease(t);
                    let lerp = |a: Pixels, b: Pixels| a + (b - a) * k;
                    tour.cursor = point(lerp(tour.from.x, tour.to.x), lerp(tour.from.y, tour.to.y));
                    if t >= 1. {
                        tour.phase = Phase::Pressing;
                        tour.since = now;
                        tour.pressed = Some((tour.to, now));
                        self.tour = Some(tour);
                        self.demo_click(name, window, cx);
                        let mut back = self.tour.take().unwrap();
                        back.since = Instant::now();
                        tour = back;
                    }
                }
                Phase::Pressing => {
                    if el >= PRESS {
                        tour.phase = Phase::Idle;
                        tour.i += 1;
                        tour.since = now;
                    }
                }
            },
        }
        self.tour = Some(tour);
    }

    /// What pressing `name` does: the same as the person's click.
    fn demo_click(&mut self, name: &str, window: &mut Window, cx: &mut Context<Self>) {
        match name {
            "welcome-start" => self.open_wizard(cx),
            "engine-hx" => self.wiz.engine = Engine::HorizonsX,
            "wiz-next" => {
                self.wiz.error = None;
                if self.wiz.step + 1 < crate::wizard::STEPS.len() {
                    self.wiz.step += 1;
                }
            }
            "wiz-create" => self.create(cx),
            "toggle-backups-row" => self.wiz.backups = !self.wiz.backups,
            "setup-open" => {
                if let Some(dir) = self.setup.dir.clone() {
                    self.open_server(dir, cx);
                }
            }
            "back-home" => self.go_home(cx),
            "set-friends" => {
                if let View::Server(dir) = self.view.clone() {
                    let on = self.dash.config.as_ref().and_then(|(_, c)| c.pointer("/server/whitelist")).and_then(|v| v.as_bool()).unwrap_or(false);
                    self.set_setting(&dir, "server.whitelist", (!on).to_string(), cx);
                }
            }
            n if n.starts_with("plug-") => {
                let file = n.trim_start_matches("plug-").to_string();
                if !self.wiz.plugins.remove(&file) {
                    self.wiz.plugins.insert(file);
                }
            }
            n if n.starts_with("field-") => {
                let f = if n == "field-Seed" { Field::Seed } else { Field::Name };
                if let Some(h) = self.focus.get(&f).cloned() {
                    window.focus(&h, cx);
                }
            }
            n if n.starts_with("nav-") => {
                let section = crate::dashboard::Section::from_name(n.trim_start_matches("nav-"));
                self.open_section(section, cx);
            }
            _ => {}
        }
        let _ = View::Wizard;
        cx.notify();
    }

    /// The tour's cursor and its press ripple, over everything.
    pub fn tour_overlay(&self) -> Option<Div> {
        let tour = self.tour.as_ref()?;
        let size = 26.;
        let c = tour.cursor;
        let mut layer = div().absolute().top_0().left_0().size_full();
        if let Some((at, when)) = tour.pressed {
            let t = (when.elapsed().as_secs_f32() / 0.5).min(1.);
            if t < 1. {
                let r = 14. + 30. * t;
                layer = layer.child(
                    div()
                        .absolute()
                        .left(at.x - gpui::px(r))
                        .top(at.y - gpui::px(r))
                        .size(gpui::px(r * 2.))
                        .rounded_full()
                        .border_2()
                        .border_color(gpui::white().opacity(0.8 * (1. - t))),
                );
            }
        }
        let pressing = tour.phase == Phase::Pressing;
        let s = if pressing { size * 0.82 } else { size };
        Some(
            layer.child(
                div()
                    .absolute()
                    .left(c.x - gpui::px(s / 2.))
                    .top(c.y - gpui::px(s / 2.))
                    .size(gpui::px(s))
                    .rounded_full()
                    .bg(gpui::white().opacity(0.92))
                    .border_2()
                    .border_color(gpui::black().opacity(0.35))
                    .shadow_lg(),
            ),
        )
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn masks() {
        // mask_ips is a no-op unless the demo is on; check the parser directly by turning it on.
        std::env::set_var("HORIZON_HOST_DEMO", "1");
        assert_eq!(super::mask_ips("join 192.168.12.113:25566 now"), "join 192.168.x.x:25566 now");
        assert_eq!(super::mask_ips("Prover[/127.0.0.1:5000]"), "Prover[/127.0.0.1:5000]");
        assert_eq!(super::mask_ips("v1.2.3 build"), "v1.2.3 build");
        assert_eq!(super::mask_ips("visit: https://playit.gg/claim/a566ac9a065a3171"), "visit: https://playit.gg/claim/••••••••");
        assert_eq!(super::mask_ips("generate claim code: a566ac9a"), "generate claim code: ••••••••");
    }
}

/// A plugin's file on this system (`luckperms_rs` -> `luckperms_rs.dll` / `libluckperms_rs.so`).
fn host_plugin(stem: &str) -> String {
    if cfg!(windows) { format!("{stem}.dll") } else { format!("lib{stem}.so") }
}
