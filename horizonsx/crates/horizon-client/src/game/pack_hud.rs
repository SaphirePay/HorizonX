//! Item displays a resource pack's entity shader puts on the screen (UltimateUI's `make_hud` in `hud.glsl`): a
//! model below y -1000 (camera-relative) is drawn in a 1920x1080 frame instead of the world, a band of a million
//! below that names the player-model part a player head stands for (`entity.fsh` remaps the head's skin faces to
//! that part's), and the screen-edge bands keep it on the left, middle or right of a wider screen. Vanilla runs the
//! pack's GLSL; here the same mapping is done to the model's vertices, which then draw with the GUI like the lifted
//! text (`text_displays.rs`), in layer order with it.

use glam::Vec3;
use horizon_client::hud::Rect;
use std::collections::HashMap;

const PART_BAND: f32 = 1_000_000.0;
const PART_MAX: i32 = 5;
const CLIP_BAND: f32 = 16384.0;

/// One lifted face: camera-relative corners (part and clip bands still on), atlas UVs, colour.
pub(super) type Face = ([Vec3; 4], [[f32; 2]; 4], u32);

/// `uui_part_id`: the part a position's band names (0: none).
pub(super) fn part_of(y: f32) -> i32 {
    if y > -(PART_BAND * 0.5) {
        return 0;
    }
    let part = ((-y) / PART_BAND + 0.5).floor() as i32;
    if (1..=PART_MAX).contains(&part) { part } else { 0 }
}

/// `make_hud`: a camera-relative position to normalised device coordinates (x, y) and its depth toward the viewer,
/// for a screen `aspect` = (height / 9 * 16) / width.
pub(super) fn to_screen(p: Vec3, aspect: f32) -> (f32, f32, f32) {
    let mut raw = p;
    raw.y += part_of(raw.y) as f32 * PART_BAND;
    let clip = (raw.x.abs() / CLIP_BAND + 0.5).floor();
    raw.x -= raw.x.signum() * clip * CLIP_BAND;
    let mut pos = raw + Vec3::new(0.0, 15000.0, 0.0);
    pos.x = -pos.x;
    let mut offset = 0.0;
    if raw.y < -20000.0 {
        if raw.y < -40000.0 {
            pos.y += 20000.0;
            offset = 1.0 - aspect;
        } else if raw.y < -30000.0 {
            pos.y += 10000.0;
        } else {
            offset = -1.0 + aspect;
        }
        pos.y += 10000.0;
        pos.x *= aspect;
    }
    (pos.x / 960.0 + offset, pos.y / 540.0, raw.z)
}

/// `entity.fsh`'s faces of each part (top, bottom, then the four sides) in skin pixels, wide arms, slim arms, body.
const SUB_UV: [[f32; 4]; 18] = [
    [4.0, 0.0, 8.0, 4.0],
    [8.0, 0.0, 12.0, 4.0],
    [0.0, 4.0, 4.0, 16.0],
    [4.0, 4.0, 8.0, 16.0],
    [8.0, 4.0, 12.0, 16.0],
    [12.0, 4.0, 16.0, 16.0],
    [4.0, 0.0, 7.0, 4.0],
    [7.0, 0.0, 10.0, 4.0],
    [0.0, 4.0, 4.0, 16.0],
    [4.0, 4.0, 7.0, 16.0],
    [7.0, 4.0, 11.0, 16.0],
    [11.0, 4.0, 14.0, 16.0],
    [4.0, 0.0, 12.0, 4.0],
    [12.0, 0.0, 20.0, 4.0],
    [0.0, 4.0, 4.0, 16.0],
    [4.0, 4.0, 12.0, 16.0],
    [12.0, 4.0, 16.0, 16.0],
    [16.0, 4.0, 24.0, 16.0],
];
/// Each part's corner in the skin, base layer then overlay: right arm, left arm, body, right leg, left leg.
const ORIGIN: [[f32; 2]; 10] = [[40.0, 16.0], [40.0, 32.0], [32.0, 48.0], [48.0, 48.0], [16.0, 16.0], [16.0, 32.0], [0.0, 16.0], [0.0, 32.0], [16.0, 48.0], [0.0, 48.0]];

/// `uui_remap` for a head face: its four skin UVs (in the 64x64 skin, as fractions) to `part`'s same face.
pub(super) fn remap_face(uv: [[f32; 2]; 4], part: i32, slim: bool) -> [[f32; 2]; 4] {
    let px: [[f32; 2]; 4] = uv.map(|u| [u[0] * 64.0, u[1] * 64.0]);
    // The face from its middle (corners sit on the edges between faces).
    let mid = [px.iter().map(|p| p[0]).sum::<f32>() / 4.0, px.iter().map(|p| p[1]).sum::<f32>() / 4.0];
    let outer = mid[0] >= 32.0;
    let shift = if outer { 32.0 } else { 0.0 };
    let (face, origin) = if mid[1] < 8.0 {
        if mid[0] - shift < 16.0 { (0, [8.0, 0.0]) } else { (1, [16.0, 0.0]) }
    } else {
        let side = ((mid[0] - shift) / 8.0).floor().clamp(0.0, 3.0);
        (side as usize + 2, [side * 8.0, 8.0])
    };
    let index = (part - 1).rem_euclid(5) as usize;
    let sub = SUB_UV[face + if slim && index <= 1 { 6 } else if index == 2 { 12 } else { 0 }];
    let base = ORIGIN[2 * index + usize::from(outer)];
    px.map(|p| {
        let f = [((p[0] - shift - origin[0]) / 8.0).clamp(0.0, 1.0), ((p[1] - origin[1]) / 8.0).clamp(0.0, 1.0)];
        [(base[0] + sub[0] + (sub[2] - sub[0]) * f[0]) / 64.0, (base[1] + sub[1] + (sub[3] - sub[1]) * f[1]) / 64.0]
    })
}

/// A lifted model's faces: a part's skin faces remapped (the skin's slot found by the UVs), the rest as they are.
pub(super) fn faces(verts: &[horizon_client::mesher::Vertex], slim: bool, sprites: &HashMap<String, Rect>) -> Vec<Face> {
    let skins: Vec<&Rect> = sprites.iter().filter(|(n, _)| n.starts_with("horizon/skin") || n.starts_with("entity/player/")).map(|(_, r)| r).collect();
    let mut out = Vec::new();
    for q in verts.as_chunks::<4>().0 {
        let pos: [Vec3; 4] = std::array::from_fn(|i| Vec3::from_array(q[i].pos));
        let mut uv: [[f32; 2]; 4] = std::array::from_fn(|i| q[i].uv);
        let part = part_of(pos[0].y);
        let mid = [uv.iter().map(|u| u[0]).sum::<f32>() / 4.0, uv.iter().map(|u| u[1]).sum::<f32>() / 4.0];
        if part > 0
            && let Some(r) = skins.iter().find(|r| mid[0] >= r.u0 && mid[0] <= r.u1 && mid[1] >= r.v0 && mid[1] <= r.v1)
        {
            let (w, h) = (r.u1 - r.u0, r.v1 - r.v0);
            let local = uv.map(|u| [(u[0] - r.u0) / w, (u[1] - r.v0) / h]);
            uv = remap_face(local, part, slim).map(|u| [r.u0 + u[0] * w, r.v0 + u[1] * h]);
        }
        out.push((pos, uv, q[0].color));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mapping_matches_the_shader() {
        // A glyph at the screen's middle: raw (0, -15000, z) is the centre; x mirrored, 960 px to the edge.
        let (x, y, z) = to_screen(Vec3::new(0.0, -15000.0, -500.0), 1.0);
        assert!(x.abs() < 1e-6 && y.abs() < 1e-6 && z == -500.0);
        assert!((to_screen(Vec3::new(-960.0, -15000.0 + 540.0, 0.0), 1.0).0 - 1.0).abs() < 1e-6);
        // A part band below: the same place, part 3.
        assert_eq!(part_of(-15000.0 - 3.0 * PART_BAND), 3);
        let a = to_screen(Vec3::new(10.0, -15000.0 - 3.0 * PART_BAND, 0.0), 1.0);
        assert!((a.0 + 10.0 / 960.0).abs() < 1e-3 && a.1.abs() < 1e-3);
        // The middle band on a wide screen: x squeezed by the aspect.
        let b = to_screen(Vec3::new(-480.0, -35000.0, 0.0), 0.5);
        assert!((b.0 - 0.25).abs() < 1e-6);
    }

    #[test]
    fn head_faces_land_on_the_part() {
        // The head's front (8..16, 8..16) as the right arm's front (44..48, 20..32).
        let front = [[8.0, 8.0], [16.0, 8.0], [16.0, 16.0], [8.0, 16.0]].map(|p: [f32; 2]| [p[0] / 64.0, p[1] / 64.0]);
        let arm = remap_face(front, 1, false).map(|u| [u[0] * 64.0, u[1] * 64.0]);
        assert_eq!(arm, [[44.0, 20.0], [48.0, 20.0], [48.0, 32.0], [44.0, 32.0]]);
        // The hat's front (40..48) as the arm's sleeve (40, 32 + ...).
        let hat = [[40.0, 8.0], [48.0, 8.0], [48.0, 16.0], [40.0, 16.0]].map(|p: [f32; 2]| [p[0] / 64.0, p[1] / 64.0]);
        assert_eq!(remap_face(hat, 1, false)[0].map(|v| v * 64.0), [44.0, 36.0]);
        // The body's front is 8 wide.
        let body = remap_face(front, 3, false).map(|u| [u[0] * 64.0, u[1] * 64.0]);
        assert_eq!((body[0], body[2]), ([20.0, 20.0], [28.0, 32.0]));
    }
}
