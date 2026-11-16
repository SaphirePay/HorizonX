//! Charts drawn on a canvas in the theme's colours: the players area chart (a smooth line over a soft fill,
//! dashed gridlines, its scale on the right, a hover marker with the time and count), sparklines and day bars.
use crate::ui::{self, px, white};
use gpui::{
    canvas, div, linear_color_stop, linear_gradient, point, prelude::*, Bounds, Div, Hsla, PathBuilder, Pixels,
};
use horizon_theme::glass::ink;
use std::cell::Cell;
use std::rc::Rc;

/// Where a chart was last drawn, for the mouse: shared between its canvas and its hover listener.
pub type Spot = Rc<Cell<Option<Bounds<Pixels>>>>;

/// A smooth path through `pts` (Catmull-Rom as cubic Béziers).
fn smooth(builder: &mut PathBuilder, pts: &[(f32, f32)]) {
    let p = |i: usize| point(gpui::px(pts[i].0), gpui::px(pts[i].1));
    builder.move_to(p(0));
    for i in 0..pts.len().saturating_sub(1) {
        let p0 = pts[i.saturating_sub(1)];
        let (p1, p2) = (pts[i], pts[i + 1]);
        let p3 = pts[(i + 2).min(pts.len() - 1)];
        let c1 = point(gpui::px(p1.0 + (p2.0 - p0.0) / 6.), gpui::px(p1.1 + (p2.1 - p0.1) / 6.));
        let c2 = point(gpui::px(p2.0 - (p3.0 - p1.0) / 6.), gpui::px(p2.1 - (p3.1 - p1.1) / 6.));
        builder.cubic_bezier_to(p(i + 1), c1, c2);
    }
}

/// The points of `values` in `b`, scaled to `top` (bottom = 0).
fn points(b: Bounds<Pixels>, values: &[f32], top: f32, pad: f32) -> Vec<(f32, f32)> {
    let (x0, y0) = (f32::from(b.origin.x), f32::from(b.origin.y));
    let (w, h) = (f32::from(b.size.width), f32::from(b.size.height));
    let n = values.len().max(2) - 1;
    values
        .iter()
        .enumerate()
        .map(|(i, v)| (x0 + w * i as f32 / n as f32, y0 + pad + (h - 2. * pad) * (1. - (v / top).clamp(0., 1.))))
        .collect()
}

/// An area: the soft fill under the line, then the line.
fn paint_area(window: &mut gpui::Window, b: Bounds<Pixels>, values: &[f32], top: f32, color: Hsla, line: f32, pad: f32) {
    if values.len() < 2 {
        return;
    }
    let pts = points(b, values, top, pad);
    let bottom = f32::from(b.origin.y) + f32::from(b.size.height);
    let mut fill = PathBuilder::fill();
    smooth(&mut fill, &pts);
    fill.line_to(point(gpui::px(pts[pts.len() - 1].0), gpui::px(bottom)));
    fill.line_to(point(gpui::px(pts[0].0), gpui::px(bottom)));
    fill.close();
    if let Ok(path) = fill.build() {
        window.paint_path(path, linear_gradient(180., linear_color_stop(color.opacity(0.36), 0.), linear_color_stop(color.opacity(0.0), 1.)));
    }
    let mut stroke = PathBuilder::stroke(gpui::px(line));
    smooth(&mut stroke, &pts);
    if let Ok(path) = stroke.build() {
        window.paint_path(path, color);
    }
}

/// A small line chart (with its fill when `fill`) in `w`×`h`, scaled to its own peak.
pub fn spark(values: Vec<f32>, color: Hsla, w: f32, h: f32, fill: bool, range: Option<(f32, f32)>) -> impl IntoElement {
    canvas(
        |_, _, _| {},
        move |b, _, window, _| {
            if values.len() < 2 {
                return;
            }
            let (lo, hi) = range.unwrap_or((0., values.iter().cloned().fold(1., f32::max) * 1.15));
            let shifted: Vec<f32> = values.iter().map(|v| (v - lo).max(0.)).collect();
            if fill {
                paint_area(window, b, &shifted, hi - lo, color, 1.6 * ui::scale(), 2.);
            } else {
                let pts = points(b, &shifted, hi - lo, 2.);
                let mut stroke = PathBuilder::stroke(gpui::px(1.6 * ui::scale()));
                smooth(&mut stroke, &pts);
                if let Ok(path) = stroke.build() {
                    window.paint_path(path, color);
                }
            }
        },
    )
    .w(px(w))
    .h(px(h))
}

/// Bars, one per value (playtime by day).
pub fn bars(values: Vec<f32>, color: Hsla, h: f32) -> Div {
    let max = values.iter().cloned().fold(0., f32::max).max(1.);
    let mut row = div().flex().items_end().gap(px(3.)).h(px(h)).w_full();
    for v in values {
        let f = (v / max).clamp(0., 1.);
        row = row.child(
            div()
                .flex_1()
                .h(if v > 0. { gpui::relative(f.max(0.06)) } else { px(2.).into() })
                .rounded(px(2.))
                .bg(if v > 0. { color } else { white(0.08) }),
        );
    }
    row
}

/// The players chart: `samples` (Unix seconds, players) over its width, gridlines at the tidy scale, the scale's
/// numbers on the right, the hovered sample marked. Returns the chart and the scale's top.
pub struct Players {
    pub samples: Vec<(u64, f32)>,
    pub color: Hsla,
    pub height: f32,
    pub hover: Option<usize>,
    pub spot: Spot,
}

impl Players {
    pub fn element(self) -> Div {
        let peak = self.samples.iter().map(|s| s.1).fold(0., f32::max);
        let (top, step) = horizon_launcher::stats::nice_scale(peak);
        let ticks = (top / step).round() as usize;
        let values: Vec<f32> = self.samples.iter().map(|s| s.1).collect();
        let hover = self.hover.filter(|i| *i < values.len());
        let (color, spot) = (self.color, self.spot.clone());
        let pad = 4.;
        let plot = canvas(
            move |b, _, _| {
                spot.set(Some(b));
            },
            move |b, _, window, _| {
                let (x0, y0) = (f32::from(b.origin.x), f32::from(b.origin.y));
                let (w, h) = (f32::from(b.size.width), f32::from(b.size.height));
                // Gridlines: dashed, the baseline solid.
                for t in 0..=ticks {
                    let y = y0 + pad + (h - 2. * pad) * (t as f32 / ticks as f32);
                    if t == ticks {
                        window.paint_quad(gpui::fill(Bounds::new(point(gpui::px(x0), gpui::px(y)), gpui::size(gpui::px(w), gpui::px(1.))), white(0.14)));
                    } else {
                        let mut x = x0;
                        while x < x0 + w {
                            window.paint_quad(gpui::fill(Bounds::new(point(gpui::px(x), gpui::px(y)), gpui::size(gpui::px(4.), gpui::px(1.))), white(0.07)));
                            x += 10.;
                        }
                    }
                }
                paint_area(window, b, &values, top, color, 2.2 * ui::scale(), pad);
                if let Some(i) = hover {
                    let pts = points(b, &values, top, pad);
                    let (hx, hy) = pts[i];
                    let mut y = hy;
                    while y < y0 + h - pad {
                        window.paint_quad(gpui::fill(Bounds::new(point(gpui::px(hx), gpui::px(y)), gpui::size(gpui::px(1.), gpui::px(3.)))  , white(0.4)));
                        y += 7.;
                    }
                    let r = 5. * ui::scale();
                    window.paint_quad(gpui::fill(Bounds::new(point(gpui::px(hx - r - 2.), gpui::px(hy - r - 2.)), gpui::size(gpui::px(2. * r + 4.), gpui::px(2. * r + 4.))), gpui::white()).corner_radii(gpui::px(r + 2.)));
                    window.paint_quad(gpui::fill(Bounds::new(point(gpui::px(hx - r), gpui::px(hy - r)), gpui::size(gpui::px(2. * r), gpui::px(2. * r))), color).corner_radii(gpui::px(r)));
                }
            },
        )
        .size_full();
        // The scale on the right: each number beside its gridline.
        let mut axis = div().relative().flex_none().w(px(46.)).h(px(self.height));
        for t in 0..=ticks {
            let v = top - step * t as f32;
            let frac = (pad + (self.height - 2. * pad) * (t as f32 / ticks as f32)) / self.height;
            axis = axis.child(
                div()
                    .absolute()
                    .left(px(10.))
                    .top(gpui::relative(frac))
                    .mt(px(-7.))
                    .child(ui::mono(10., ink::FAINT).child(thousands(v))),
            );
        }
        div()
            .flex()
            .w_full()
            .child(div().relative().flex_1().min_w_0().h(px(self.height)).child(plot))
            .child(axis)
    }
}

/// 1650 -> "1,650"; 2.5 -> "2.5".
pub fn thousands(v: f32) -> String {
    if v.fract().abs() > 0.01 {
        return format!("{v:.1}");
    }
    let s = (v.round() as i64).to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn thousands_has_commas() {
        assert_eq!(super::thousands(1650.), "1,650");
        assert_eq!(super::thousands(25.), "25");
        assert_eq!(super::thousands(2000000.), "2,000,000");
    }
}
