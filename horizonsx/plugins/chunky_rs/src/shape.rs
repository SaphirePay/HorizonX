//! Chunky's shapes, chunk-aligned (`AbstractShape` with `chunkAligned`): is a chunk's centre inside?

use crate::Selection;

fn inside_line(ax: f64, az: f64, bx: f64, bz: f64, cx: f64, cz: f64) -> bool {
    (bx - ax) * (cz - az) > (bz - az) * (cx - ax)
}

fn intersection(l1: [(f64, f64); 2], l2: [(f64, f64); 2]) -> Option<(f64, f64)> {
    let (a1, a2) = (l1[1].1 - l1[0].1, l2[1].1 - l2[0].1);
    let (b1, b2) = (l1[0].0 - l1[1].0, l2[0].0 - l2[1].0);
    let det = a1 * b2 - a2 * b1;
    if det == 0.0 {
        return None;
    }
    let (c1, c2) = (a1 * l1[0].0 + b1 * l1[0].1, a2 * l2[0].0 + b2 * l2[0].1);
    Some(((b2 * c1 - b1 * c2) / det, (a1 * c2 - a2 * c1) / det))
}

pub enum Shape {
    Square { b1: (f64, f64), b2: (f64, f64) },
    Circle { c: (f64, f64), r: f64 },
    Ellipse { c: (f64, f64), rx: f64, rz: f64 },
    /// Convex polygon: inside every edge (points in order).
    Polygon(Vec<(f64, f64)>),
    Star(Vec<(f64, f64)>),
}

impl Shape {
    pub fn new(sel: &Selection) -> Shape {
        let (ccx, ccz) = sel.center_chunk();
        let (rcx, rcz) = sel.radius_chunks();
        let (cx, cz) = (((ccx << 4) as f64) + 8.0, ((ccz << 4) as f64) + 8.0);
        let (rx, rz) = (((2 * rcx + 1) << 4) as f64 / 2.0, ((2 * rcz + 1) << 4) as f64 / 2.0);
        let ring = |angles: &[f64]| -> Vec<(f64, f64)> { angles.iter().map(|a| (cx + rx * a.to_radians().cos(), cz + rx * a.to_radians().sin())).collect() };
        match sel.shape.as_str() {
            "circle" => Shape::Circle { c: (cx, cz), r: rx },
            "ellipse" | "oval" => Shape::Ellipse { c: (cx, cz), rx, rz },
            "rectangle" => Shape::Square { b1: (cx - rx, cz - rz), b2: (cx + rx, cz + rz) },
            "diamond" => Shape::Polygon(vec![(cx, cz + rx), (cx - rx, cz), (cx, cz - rx), (cx + rx, cz)]),
            "triangle" => Shape::Polygon(vec![(cx + rx, cz + rx), (cx - rx, cz + rx), (cx, cz - rx)]),
            "pentagon" => Shape::Polygon(ring(&[54.0, 126.0, 198.0, 270.0, 342.0])),
            "hexagon" => Shape::Polygon(ring(&[60.0, 120.0, 180.0, 240.0, 300.0, 360.0])),
            "star" => Shape::Star(ring(&[54.0, 126.0, 198.0, 270.0, 342.0])),
            _ => Shape::Square { b1: (cx - rx, cz - rx), b2: (cx + rx, cz + rx) },
        }
    }

    pub fn is_bounding(&self, x: f64, z: f64) -> bool {
        match self {
            Shape::Square { b1, b2 } => x >= b1.0 && x <= b2.0 && z >= b1.1 && z <= b2.1,
            Shape::Circle { c, r } => (c.0 - x).hypot(c.1 - z) <= *r,
            Shape::Ellipse { c, rx, rz } => ((x - c.0) / rx).powi(2) + ((z - c.1) / rz).powi(2) <= 1.0,
            Shape::Polygon(p) => (0..p.len()).all(|i| {
                let (a, b) = (p[i], p[(i + 1) % p.len()]);
                inside_line(a.0, a.1, b.0, b.1, x, z)
            }),
            Shape::Star(p) => {
                // `Star.isBounding`: inside one of the five 4-edge wedges drawn between every other point.
                let edge = |i: usize, j: usize| inside_line(p[i].0, p[i].1, p[j].0, p[j].1, x, z);
                let (e13, e24, e35, e41, e52) = (edge(0, 2), edge(1, 3), edge(2, 4), edge(3, 0), edge(4, 1));
                (e13 && e24 && e35 && e41) || (e24 && e35 && e41 && e52) || (e35 && e41 && e52 && e13) || (e41 && e52 && e13 && e24) || (e52 && e13 && e24 && e35)
            }
        }
    }
}

#[allow(dead_code)]
fn star_inner(p: &[(f64, f64)]) -> Option<(f64, f64)> {
    intersection([p[0], p[2]], [p[1], p[4]])
}

#[cfg(test)]
mod tests {
    use super::*;
    fn sel(shape: &str) -> Selection {
        Selection { world: "w".into(), center_x: 0.0, center_z: 0.0, radius_x: 100.0, radius_z: 50.0, pattern: "region".into(), pattern_value: None, shape: shape.into() }
    }
    #[test]
    fn shapes() {
        for name in crate::SHAPES {
            let s = Shape::new(&sel(name));
            assert!(s.is_bounding(8.0, 8.0), "{name} holds its centre");
            assert!(!s.is_bounding(400.0, 400.0), "{name} not far away");
        }
        assert!(Shape::new(&sel("square")).is_bounding(100.0, 100.0));
        assert!(!Shape::new(&sel("circle")).is_bounding(100.0, 100.0));
        assert!(!Shape::new(&sel("rectangle")).is_bounding(8.0, 90.0));
        assert!(star_inner(&[(0.0, 1.0), (1.0, 0.0), (0.0, -1.0), (-1.0, 0.0), (0.5, 0.5)]).is_some());
    }
}
