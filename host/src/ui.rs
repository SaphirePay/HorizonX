//! Horizon Host's controls, in the Horizon launcher's glass style: its tokens (`horizon_theme::glass`),
//! Geist for text and Bricolage for headings, white at a few opacities over frosted plates, one
//! violet accent. Built here for this app's own layout.
use gpui::{
    div, prelude::*, rgb, svg, App, ClickEvent, Div, FontWeight, Hsla, SharedString, Stateful, Svg,
    Window,
};
use horizon_theme::glass::{fill, ink, radius};

thread_local! {
    /// How much bigger everything is drawn: 1 in a window up to 1400 px wide, growing with it to
    /// 1.6 (a full-screen 1440p display), so a maximized window is filled, not a small island.
    static SCALE: std::cell::Cell<f32> = const { std::cell::Cell::new(1.0) };
}

thread_local! {
    /// The window's width in logical pixels, for layouts that stack on a small window.
    static WIDTH: std::cell::Cell<f32> = const { std::cell::Cell::new(1400.0) };
}

/// Set as each frame starts, from the window's width.
pub fn set_scale(width: f32) {
    SCALE.set((width / 1400.).clamp(1.0, 1.6));
    WIDTH.set(width);
}

/// Wide enough for side-by-side columns (the dashboard's table beside its side panels): the window's width in
/// the app's own units (before the scale), as the mockups were laid out at 1600.
pub fn wide() -> bool {
    WIDTH.get() / SCALE.get() >= 1460.
}

/// A dropdown: the current choice on a button; open, its choices float under it. `open` says whether it's
/// open; `on_toggle` opens or closes it; each choice's click comes with its index.
pub fn picker(
    id: &'static str,
    current: impl Into<SharedString>,
    lead: Option<Tone>,
    choices: Vec<(SharedString, Option<Tone>)>,
    open: bool,
    on_toggle: impl Fn(&mut Window, &mut App) + 'static,
    on_pick: impl Fn(usize, &mut Window, &mut App) + Clone + 'static,
) -> Div {
    let button = div()
        .id(id)
        .flex()
        .flex_none()
        .items_center()
        .gap(px(9.))
        .h(px(36.))
        .pl(px(14.))
        .pr(px(12.))
        .rounded(px(radius::CONTROL))
        .bg(white(if open { fill::BUTTON_HOVER } else { fill::BUTTON }))
        .hover(|s| s.bg(white(fill::BUTTON_HOVER)))
        .cursor_pointer()
        .children(lead.map(dot))
        .child(text(14., FontWeight::MEDIUM, ink::FULL).child(current.into()))
        .child(icon("arrow-down-01", 14., white(ink::QUIET)))
        .on_click(move |_: &ClickEvent, w, cx| on_toggle(w, cx));
    let mut wrap = div().relative().flex_none().child(button);
    if open {
        let mut list = div()
            .id(SharedString::from(format!("{id}-menu")))
            .occlude()
            .mt(px(6.))
            .min_w(px(220.))
            .max_h(px(360.))
            .overflow_y_scroll()
            .p(px(6.))
            .rounded(px(radius::POPOVER))
            .bg(color(0x16122e))
            .border_1()
            .border_color(white(0.1))
            .shadow_lg()
            .flex()
            .flex_col();
        for (i, (label, tone)) in choices.into_iter().enumerate() {
            let pick = on_pick.clone();
            list = list.child(
                div()
                    .id(SharedString::from(format!("{id}-opt-{i}")))
                    .flex()
                    .items_center()
                    .gap(px(9.))
                    .h(px(34.))
                    .px(px(10.))
                    .rounded(px(radius::SMALL))
                    .cursor_pointer()
                    .hover(|s| s.bg(white(fill::ROW_HOVER)))
                    .children(tone.map(dot))
                    .child(text(13.5, FontWeight::MEDIUM, ink::FULL).child(label))
                    .on_click(move |_: &ClickEvent, w, cx| pick(i, w, cx)),
            );
        }
        wrap = wrap.child(gpui::deferred(gpui::anchored().snap_to_window_with_margin(px(8.)).child(list)).with_priority(1));
    }
    wrap
}

/// Tabs in one soft row (Settings' sections, a server's sections): the chosen one filled.
pub fn tabs<T: Copy + PartialEq + 'static, L: Into<SharedString> + Clone>(
    id: &'static str,
    options: &[(T, L)],
    current: T,
    on_pick: impl Fn(T, &mut Window, &mut App) + Clone + 'static,
) -> Stateful<Div> {
    div()
        .id(id)
        .flex()
        .flex_none()
        .gap(px(4.))
        .p(px(4.))
        .rounded(px(radius::ROW))
        .bg(white(0.05))
        .children(options.iter().enumerate().map(|(n, (value, label))| {
            let (value, label): (T, SharedString) = (*value, label.clone().into());
            let active = current == value;
            let pick = on_pick.clone();
            div()
                .id(SharedString::from(format!("{id}-{n}")))
                .flex()
                .flex_none()
                .items_center()
                .h(px(32.))
                .px(px(14.))
                .rounded(px(radius::SMALL))
                .cursor_pointer()
                .font_family(FONT)
                .text_size(px(13.5))
                .font_weight(FontWeight::MEDIUM)
                .when(active, |d| d.bg(white(fill::ACTIVE)).text_color(white(ink::FULL)))
                .when(!active, |d| d.text_color(white(ink::BODY)).hover(|s| s.text_color(white(ink::FULL))))
                .child(label)
                .on_click(move |_: &ClickEvent, window, cx| pick(value, window, cx))
        }))
}

/// A player's head: the 8×8 face of their skin, or a plain tile while it's unknown.
pub fn head(face: Option<&[[u8; 4]]>, size: f32) -> Div {
    let Some(face) = face.filter(|f| f.len() == 64) else {
        return div().flex_none().size(px(size)).rounded(px(size / 6.)).bg(white(0.1));
    };
    let cell = size / 8.;
    let mut grid = div().flex_none().flex().flex_col().size(px(size)).rounded(px(size / 6.)).overflow_hidden();
    for y in 0..8 {
        let mut row = div().flex().h(px(cell));
        for x in 0..8 {
            let [r, g, b, _] = face[y * 8 + x];
            row = row.child(div().w(px(cell)).h_full().bg(rgb(((r as u32) << 16) | ((g as u32) << 8) | b as u32)));
        }
        grid = grid.child(row);
    }
    grid
}

/// A small tag beside a name (`OP`, `NEW`).
pub fn tag(label: &'static str, accent: bool) -> Div {
    div()
        .flex_none()
        .px(px(6.))
        .rounded(px(5.))
        .bg(if accent { color(ACCENT).opacity(0.24) } else { white(0.08) })
        .child(mono(10., if accent { ink::FULL } else { ink::BODY }).child(label))
}

/// A table's column title.
pub fn th(t: &'static str) -> Div {
    mono(10.5, ink::QUIET).child(t)
}

/// Sizes: 1.3 GB, 182 MB.
pub fn bytes(b: u64) -> String {
    if b >= 1_000_000_000 {
        format!("{:.1} GB", b as f64 / 1e9)
    } else {
        format!("{} MB", b / 1_000_000)
    }
}

/// A length of time: 42m, 6h 12m, 3 days.
pub fn span(secs: u64) -> String {
    match secs {
        s if s >= 2 * 86_400 => format!("{} days", s / 86_400),
        s if s >= 3600 => format!("{}h {:02}m", s / 3600, s % 3600 / 60),
        s if s >= 60 => format!("{}m", s / 60),
        _ => "now".to_string(),
    }
}

pub fn scale() -> f32 {
    SCALE.get()
}

/// A length at the window's scale: every size in Horizon Host's own views goes through this.
pub fn px(v: f32) -> gpui::Pixels {
    gpui::px(v * SCALE.get())
}

pub const ACCENT: u32 = horizon_theme::glass::ACCENT;
pub const LIVE: u32 = horizon_theme::glass::LIVE;
pub const WARN: u32 = horizon_theme::glass::WARN;
pub const DANGER: u32 = horizon_theme::glass::DANGER;
pub const FONT: &str = horizon_theme::glass::FONT;
pub const DISPLAY: &str = horizon_theme::glass::DISPLAY_FONT;
pub const HEADING: &str = horizon_theme::glass::HEADING_FONT;
pub const MONO: &str = horizon_theme::MONO;

pub fn white(a: f32) -> Hsla {
    gpui::white().opacity(a)
}

pub fn color(c: u32) -> Hsla {
    rgb(c).into()
}

/// Text in Geist at `size`, white at `a`.
pub fn text(size: f32, weight: FontWeight, a: f32) -> Div {
    div()
        .font_family(FONT)
        .text_size(px(size))
        .line_height(px((size * 1.45).round()))
        .font_weight(weight)
        .text_color(white(a))
}

/// A big title in Bricolage Display (pages, the wizard's questions).
pub fn display(size: f32) -> Div {
    div()
        .font_family(DISPLAY)
        .font_weight(FontWeight::SEMIBOLD)
        .text_size(px(size))
        .line_height(px((size * 1.1).round()))
        .text_color(white(ink::FULL))
}

/// A section heading in Bricolage.
pub fn heading(size: f32) -> Div {
    div()
        .font_family(HEADING)
        .font_weight(FontWeight::SEMIBOLD)
        .text_size(px(size))
        .line_height(px((size * 1.3).round()))
        .text_color(white(ink::FULL))
}

/// A small mono label (counts, ports, steps).
pub fn mono(size: f32, a: f32) -> Div {
    div().font_family(MONO).text_size(px(size)).line_height(px((size * 1.5).round())).text_color(white(a))
}

/// One of the launcher's icons (Hugeicons), stroked in `c`.
pub fn icon(name: &'static str, size: f32, c: Hsla) -> Svg {
    svg().path(format!("icons/glass/{name}.svg")).flex_none().size(px(size)).text_color(c)
}

/// A frosted plate (the scene blurred behind it), round at `r`.
pub fn plate(r: f32) -> Div {
    crate::glass::plate(r)
}

/// A panel: a frosted plate with padding, children stacked.
pub fn panel() -> Div {
    plate(radius::PANEL).relative().flex().flex_col().p(px(24.))
}

#[derive(Clone, Copy, PartialEq)]
pub enum Kind {
    Primary,
    Secondary,
    Ghost,
    Danger,
}

/// A button: Primary is the accent (one per screen), Secondary white at 8 %, Ghost text only.
pub fn button(id: impl Into<SharedString>, label: impl Into<SharedString>, kind: Kind) -> Stateful<Div> {
    sized_button(id, label, kind, 40., 15.)
}

pub fn sized_button(id: impl Into<SharedString>, label: impl Into<SharedString>, kind: Kind, h: f32, size: f32) -> Stateful<Div> {
    let d = div()
        .id(id.into())
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .gap(px(8.))
        .h(px(h))
        .px(px(if h >= 44. { 22. } else { 16. }))
        .rounded(px(radius::CONTROL))
        .cursor_pointer()
        .font_family(FONT)
        .text_size(px(size))
        .font_weight(FontWeight::MEDIUM);
    let d = match kind {
        Kind::Primary => d
            .bg(color(ACCENT))
            .text_color(gpui::white())
            .font_weight(FontWeight::SEMIBOLD)
            .hover(|s| s.opacity(0.9)),
        Kind::Secondary => d
            .bg(white(fill::BUTTON))
            .text_color(white(ink::FULL))
            .hover(|s| s.bg(white(fill::BUTTON_HOVER))),
        Kind::Ghost => d.text_color(white(ink::BODY)).hover(|s| s.bg(white(0.06)).text_color(white(ink::FULL))),
        Kind::Danger => d
            .bg(color(DANGER).opacity(0.14))
            .text_color(color(DANGER))
            .hover(|s| s.bg(color(DANGER).opacity(0.22))),
    };
    d.child(label.into())
}

/// A button with an icon before its label.
pub fn icon_button(id: impl Into<SharedString>, name: &'static str, label: impl Into<SharedString>, kind: Kind) -> Stateful<Div> {
    let tint = match kind {
        Kind::Primary => gpui::white(),
        Kind::Danger => color(DANGER),
        _ => white(ink::FULL),
    };
    let label: SharedString = label.into();
    let d = button(id, "", kind);
    d.child(icon(name, 16., tint)).child(label)
}

/// A square button with one icon.
pub fn square(id: impl Into<SharedString>, name: &'static str, size: f32) -> Stateful<Div> {
    div()
        .id(id.into())
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .size(px(size))
        .rounded(px(radius::CONTROL))
        .bg(white(fill::BUTTON))
        .hover(|s| s.bg(white(fill::BUTTON_HOVER)))
        .cursor_pointer()
        .child(icon(name, (size * 0.45).round(), white(ink::FULL)))
}

#[derive(Clone, Copy, PartialEq)]
pub enum Tone {
    Live,
    Busy,
    Off,
    Bad,
}

/// A state dot.
pub fn dot(tone: Tone) -> Div {
    let c = match tone {
        Tone::Live => color(LIVE),
        Tone::Busy => color(WARN),
        Tone::Bad => color(DANGER),
        Tone::Off => white(0.35),
    };
    div().flex_none().size(px(8.)).rounded_full().bg(c)
}

/// A small rounded label (`Running`, `Built in`).
pub fn chip(label: impl Into<SharedString>, tone: Option<Tone>) -> Div {
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(px(6.))
        .h(px(24.))
        .px(px(10.))
        .rounded_full()
        .bg(white(fill::BUTTON))
        .children(tone.map(dot))
        .child(text(12., FontWeight::MEDIUM, ink::STRONG).child(label.into()))
}

/// A field's box (44 px, round, white at 7 %; 11 % and an accent ring with the focus).
pub fn field_box(focused: bool) -> Div {
    div()
        .flex()
        .items_center()
        .h(px(44.))
        .px(px(14.))
        .rounded(px(radius::ROW))
        .bg(white(if focused { fill::FIELD_FOCUS } else { fill::FIELD }))
        .border_1()
        .border_color(if focused { color(ACCENT) } else { white(0.0) })
        .font_family(FONT)
        .text_size(px(15.))
        .overflow_hidden()
}

/// A switch: 44×26, accent when on.
pub fn toggle(id: impl Into<SharedString>, on: bool) -> Stateful<Div> {
    let id: SharedString = id.into();
    let knob = div().absolute().top(px(4.)).size(px(18.)).rounded_full().bg(if on { gpui::white() } else { white(0.7) });
    // The slide is in plain pixels: give it the window's scale like the track, or the knob stops
    // short of the end on a big window.
    let knob = crate::motion::knob(&id, on, knob, 4. * scale(), 18. * scale());
    div()
        .id(id)
        .relative()
        .flex_none()
        .w(px(44.))
        .h(px(26.))
        .rounded_full()
        .bg(if on { color(ACCENT) } else { white(fill::BUTTON_HOVER) })
        .cursor_pointer()
        .child(knob)
}

/// Segmented options; `on_pick` gets the chosen value.
pub fn segments<T: Copy + PartialEq + 'static>(
    id: &'static str,
    options: &[(T, &'static str)],
    current: T,
    on_pick: impl Fn(T, &mut Window, &mut App) + Clone + 'static,
) -> Stateful<Div> {
    div()
        .id(id)
        .flex()
        .flex_none()
        .gap(px(4.))
        .p(px(4.))
        .rounded(px(radius::LARGE))
        .bg(white(0.05))
        .children(options.iter().map(|&(value, label)| {
            let active = current == value;
            let pick = on_pick.clone();
            div()
                .id(SharedString::from(format!("{id}-{label}")))
                .flex()
                .flex_none()
                .items_center()
                .justify_center()
                .h(px(34.))
                .px(px(14.))
                .rounded(px(radius::CONTROL))
                .cursor_pointer()
                .font_family(FONT)
                .text_size(px(14.))
                .when(active, |d| d.bg(white(fill::BUTTON_HOVER)).font_weight(FontWeight::MEDIUM).text_color(white(ink::FULL)))
                .when(!active, |d| d.text_color(white(ink::MUTED)).hover(|s| s.text_color(white(ink::FULL))))
                .child(label)
                .on_click(move |_: &ClickEvent, window, cx| pick(value, window, cx))
        }))
}

/// − value + : a number in steps, between `min` and `max`.
pub fn stepper(
    id: &'static str,
    value: u32,
    step: u32,
    min: u32,
    max: u32,
    unit: &'static str,
    on_change: impl Fn(u32, &mut Window, &mut App) + Clone + 'static,
) -> Div {
    let down = on_change.clone();
    let up = on_change;
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(px(8.))
        .child(
            square(SharedString::from(format!("{id}-down")), "minus-sign", 36.)
                .on_click(move |_: &ClickEvent, w, cx| down(value.saturating_sub(step).max(min), w, cx)),
        )
        .child(
            div()
                .min_w(px(92.))
                .flex()
                .justify_center()
                .child(text(16., FontWeight::MEDIUM, ink::FULL).child(format!("{value}{unit}"))),
        )
        .child(
            square(SharedString::from(format!("{id}-up")), "plus-sign", 36.)
                .on_click(move |_: &ClickEvent, w, cx| up((value + step).min(max), w, cx)),
        )
}

/// A setting row: name and note on the left, the control on the right, a rule below.
pub fn setting_row(name: impl Into<SharedString>, note: impl Into<SharedString>, control: impl IntoElement) -> Div {
    let note: SharedString = note.into();
    div()
        .flex()
        .items_center()
        .justify_between()
        .gap(px(24.))
        .py(px(14.))
        .border_b_1()
        .border_color(white(0.07))
        .child(
            div()
                .flex()
                .flex_col()
                .flex_1()
                .min_w_0()
                .child(text(15., FontWeight::MEDIUM, ink::FULL).child(name.into()))
                .when(!note.is_empty(), |d| d.child(text(13., FontWeight::NORMAL, ink::QUIET).child(note))),
        )
        .child(control)
}

/// A card that can be chosen (the wizard's options): its fill and an accent ring when chosen.
pub fn choice(id: impl Into<SharedString>, chosen: bool) -> Stateful<Div> {
    div()
        .id(id.into())
        .relative()
        .flex()
        .flex_col()
        .gap(px(8.))
        .p(px(20.))
        .rounded(px(radius::PANEL))
        .cursor_pointer()
        .bg(white(if chosen { 0.1 } else { 0.05 }))
        .border_1()
        .border_color(if chosen { color(ACCENT) } else { white(0.08) })
        .hover(|s| s.bg(white(0.1)))
}

/// A number in big type over its label (the dashboard's stats).
pub fn stat(value: impl Into<SharedString>, label: impl Into<SharedString>) -> Div {
    plate(radius::PANEL)
        .relative()
        .flex()
        .flex_col()
        .flex_1()
        .min_w_0()
        .gap(px(2.))
        .px(px(20.))
        .py(px(16.))
        .child(heading(26.).child(value.into()))
        .child(text(12., FontWeight::MEDIUM, ink::QUIET).child(label.into()))
}

/// A dashboard total: a tinted icon, the label, the number big, a note under it.
pub fn stat_tile(icon_name: &'static str, tint: u32, value: impl Into<SharedString>, label: &'static str, note: impl Into<SharedString>) -> Div {
    plate(radius::PANEL)
        .relative()
        .flex()
        .flex_1()
        .min_w_0()
        .items_center()
        .gap(px(16.))
        .px(px(20.))
        .py(px(18.))
        .child(
            div()
                .flex()
                .flex_none()
                .items_center()
                .justify_center()
                .size(px(48.))
                .rounded(px(radius::ROW))
                .bg(color(tint).opacity(0.18))
                .child(icon(icon_name, 22., color(tint))),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .min_w_0()
                .child(text(13., FontWeight::MEDIUM, ink::QUIET).child(label))
                .child(heading(28.).child(value.into()))
                .child(text(12., FontWeight::NORMAL, ink::QUIET).truncate().child(note.into())),
        )
}

/// A thin rule.
pub fn rule() -> Div {
    div().h(px(1.)).w_full().bg(white(0.08))
}
