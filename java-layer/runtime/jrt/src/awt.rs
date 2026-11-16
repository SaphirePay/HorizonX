//! The part of `java.awt` and `javax.imageio` plugins use for pictures: read an image (a player's face from a
//! skin site, a logo in the plugin's folder), scale it, read and set its pixels, draw one image on another,
//! write it as PNG; `java.awt.Color` for its pixels' colours. Pixels are ARGB, as `getRGB` gives them.
use crate::{R, V, exc, lock, native, s};
use std::sync::Mutex;

/// `BufferedImage`: width, height, ARGB pixels row by row.
pub struct JImage {
    w: usize,
    h: usize,
    px: Mutex<Vec<u32>>,
}
/// `Graphics2D` drawing on an image.
pub struct JGraphics(V);
/// `AffineTransform`: a scale and a move.
pub struct JTransform(Mutex<[f64; 4]>);
/// `AffineTransformOp`: a transform to apply.
pub struct JTransformOp([f64; 4]);
/// `java.awt.Color`: ARGB.
pub struct JColor(pub u32);

fn image(w: usize, h: usize, px: Vec<u32>) -> V {
    native(JImage { w, h, px: Mutex::new(px) })
}

/// The image in these bytes (PNG, JPEG, GIF, BMP), or null as `ImageIO.read` gives for what it can't read.
fn decode(bytes: &[u8]) -> V {
    match image::load_from_memory(bytes) {
        Ok(decoded) => {
            let rgba = decoded.to_rgba8();
            let (w, h) = (rgba.width() as usize, rgba.height() as usize);
            let px = rgba.pixels().map(|p| (u32::from(p[3]) << 24) | (u32::from(p[0]) << 16) | (u32::from(p[1]) << 8) | u32::from(p[2])).collect();
            image(w, h, px)
        }
        Err(_) => V::Null,
    }
}

fn png(img: &JImage) -> Vec<u8> {
    let px = lock(&img.px);
    let mut rgba = Vec::with_capacity(img.w * img.h * 4);
    for p in px.iter() {
        rgba.extend_from_slice(&[(p >> 16) as u8, (p >> 8) as u8, *p as u8, (p >> 24) as u8]);
    }
    let mut out = std::io::Cursor::new(Vec::new());
    let _ = image::RgbaImage::from_raw(img.w as u32, img.h as u32, rgba).map(|buffer| buffer.write_to(&mut out, image::ImageFormat::Png));
    out.into_inner()
}

/// Nearest-neighbour scaling to `w` by `h`, as `AffineTransformOp.TYPE_NEAREST_NEIGHBOR` and the default
/// `getScaledInstance` look at this size.
fn scaled(img: &JImage, w: usize, h: usize) -> V {
    let (w, h) = (w.max(1), h.max(1));
    let px = lock(&img.px);
    let mut out = Vec::with_capacity(w * h);
    for y in 0..h {
        let sy = (y * img.h / h).min(img.h.saturating_sub(1));
        for x in 0..w {
            let sx = (x * img.w / w).min(img.w.saturating_sub(1));
            out.push(px.get(sy * img.w + sx).copied().unwrap_or(0));
        }
    }
    image(w, h, out)
}

fn read_source(source: &V) -> R {
    // A URL (text), a file, or a stream's bytes.
    if let V::Str(text) = source {
        if text.starts_with("http://") || text.starts_with("https://") {
            let stream = crate::call(source, "openStream", &[])?;
            return Ok(decode(&crate::javaio::bytes_of(&stream).unwrap_or_default()));
        }
    }
    if let Some(path) = source.native::<crate::javaio::JFile>().map(|f| f.0.clone()) {
        return std::fs::read(&path).map(|b| decode(&b)).map_err(|e| exc("IIOException", format!("Can't read input file! {e}")));
    }
    Ok(decode(&crate::javaio::bytes_of(source).unwrap_or_default()))
}

pub fn static_call(class: &str, name: &str, a: &[V]) -> Option<R> {
    let simple = class.rsplit('.').next().unwrap_or(class);
    Some(match (simple, name, a.len()) {
        ("ImageIO", "read", 1) => read_source(&a[0]),
        ("ImageIO", "write", 3) => {
            let Some(img) = a[0].native::<JImage>() else {
                return Some(Err(exc("IllegalArgumentException", "image == null!")));
            };
            let bytes = png(img);
            match a[2].native::<crate::javaio::JFile>().map(|f| f.0.clone()) {
                Some(path) => std::fs::write(&path, &bytes).map(|()| V::Bool(true)).map_err(|e| exc("IOException", e.to_string())),
                None => crate::call(&a[2], "write", &[crate::javaio::to_byte_array(&bytes)]).map(|_| V::Bool(true)),
            }
        }
        ("AffineTransform", "getScaleInstance", 2) => Ok(native(JTransform(Mutex::new([a[0].double(), a[1].double(), 0.0, 0.0])))),
        _ => return None,
    })
}

pub fn construct(class: &str, a: &[V]) -> Option<R> {
    let simple = class.rsplit('.').next().unwrap_or(class);
    Some(Ok(match (simple, a.len()) {
        ("BufferedImage", 3) => {
            let (w, h) = (a[0].int().max(0) as usize, a[1].int().max(0) as usize);
            image(w, h, vec![0; w * h])
        }
        ("AffineTransform", 0) => native(JTransform(Mutex::new([1.0, 1.0, 0.0, 0.0]))),
        ("AffineTransformOp", 2) => native(JTransformOp(a[0].native::<JTransform>().map_or([1.0, 1.0, 0.0, 0.0], |t| *lock(&t.0)))),
        // `java.awt.Color` only by its full name: `Color` alone is the server's.
        _ if class == "java.awt.Color" => {
            let byte = |v: &V| match v {
                V::Double(f) if *f <= 1.0 => (f * 255.0).round() as u32,
                other => other.int().clamp(0, 255) as u32,
            };
            native(JColor(match a {
                [rgb] => 0xff00_0000 | (rgb.int() as u32 & 0xff_ffff),
                [rgb, V::Bool(alpha)] => if *alpha { rgb.int() as u32 } else { 0xff00_0000 | (rgb.int() as u32 & 0xff_ffff) },
                [r, g, b] => 0xff00_0000 | (byte(r) << 16) | (byte(g) << 8) | byte(b),
                [r, g, b, alpha] => (byte(alpha) << 24) | (byte(r) << 16) | (byte(g) << 8) | byte(b),
                _ => return Some(Err(exc("IllegalArgumentException", "Color"))),
            }))
        }
        _ => return None,
    }))
}

pub fn method(this: &V, name: &str, a: &[V]) -> Option<R> {
    if let Some(img) = this.native::<JImage>() {
        let at = |x: &V, y: &V| {
            let (x, y) = (x.int(), y.int());
            (x >= 0 && y >= 0 && (x as usize) < img.w && (y as usize) < img.h).then(|| y as usize * img.w + x as usize)
        };
        return Some(Ok(match (name, a.len()) {
            ("getWidth", _) => V::Int(img.w as i32),
            ("getHeight", _) => V::Int(img.h as i32),
            ("getType", 0) => V::Int(2),
            ("getRGB", 2) => match at(&a[0], &a[1]) {
                Some(i) => V::Int(lock(&img.px)[i] as i32),
                None => return Some(Err(exc("ArrayIndexOutOfBoundsException", "Coordinate out of bounds!"))),
            },
            ("setRGB", 3) => {
                if let Some(i) = at(&a[0], &a[1]) {
                    lock(&img.px)[i] = a[2].int() as u32;
                }
                V::Null
            }
            ("getScaledInstance", 3) => {
                let ratio = img.w as f64 / img.h.max(1) as f64;
                let (mut w, mut h) = (a[0].int(), a[1].int());
                if w < 0 {
                    w = (f64::from(h) * ratio).round() as i32;
                }
                if h < 0 {
                    h = (f64::from(w) / ratio).round() as i32;
                }
                scaled(img, w.max(1) as usize, h.max(1) as usize)
            }
            ("getSubimage", 4) => {
                let (x0, y0, w, h) = (a[0].int().max(0) as usize, a[1].int().max(0) as usize, a[2].int().max(0) as usize, a[3].int().max(0) as usize);
                let px = lock(&img.px);
                let mut out = Vec::with_capacity(w * h);
                for y in y0..(y0 + h).min(img.h) {
                    for x in x0..(x0 + w).min(img.w) {
                        out.push(px[y * img.w + x]);
                    }
                }
                image(w.min(img.w.saturating_sub(x0)), h.min(img.h.saturating_sub(y0)), out)
            }
            ("createGraphics" | "getGraphics", 0) => native(JGraphics(this.clone())),
            ("flush", 0) => V::Null,
            _ => return None,
        }));
    }
    if let Some(g) = this.native::<JGraphics>() {
        return Some(Ok(match name {
            // `drawImage(img, x, y, [w, h,] observer)`.
            "drawImage" => {
                let (Some(src), Some(dst)) = (a.first().and_then(|v| v.native::<JImage>()), g.0.native::<JImage>()) else {
                    return Some(Ok(V::Bool(false)));
                };
                let (x0, y0) = (a.get(1).map_or(0, V::int), a.get(2).map_or(0, V::int));
                let drawn = if a.len() >= 5 && matches!(a[3], V::Int(_) | V::Long(_)) { scaled(src, a[3].int().max(1) as usize, a[4].int().max(1) as usize) } else { scaled(src, src.w, src.h) };
                if let Some(drawn) = drawn.native::<JImage>() {
                    let from = lock(&drawn.px).clone();
                    let mut to = lock(&dst.px);
                    for y in 0..drawn.h {
                        for x in 0..drawn.w {
                            let (tx, ty) = (x0 + x as i32, y0 + y as i32);
                            if tx >= 0 && ty >= 0 && (tx as usize) < dst.w && (ty as usize) < dst.h {
                                let p = from[y * drawn.w + x];
                                if p >> 24 != 0 {
                                    to[ty as usize * dst.w + tx as usize] = p;
                                }
                            }
                        }
                    }
                }
                V::Bool(true)
            }
            "dispose" | "setRenderingHint" | "setRenderingHints" | "setComposite" | "setColor" | "setBackground" | "setFont" | "setStroke" => V::Null,
            _ => return None,
        }));
    }
    if let Some(t) = this.native::<JTransform>() {
        return Some(Ok(match (name, a.len()) {
            ("scale", 2) => {
                let mut m = lock(&t.0);
                m[0] *= a[0].double();
                m[1] *= a[1].double();
                V::Null
            }
            ("translate", 2) => {
                let mut m = lock(&t.0);
                m[2] += a[0].double();
                m[3] += a[1].double();
                V::Null
            }
            ("getScaleX", 0) => V::Double(lock(&t.0)[0]),
            ("getScaleY", 0) => V::Double(lock(&t.0)[1]),
            _ => return None,
        }));
    }
    if let Some(op) = this.native::<JTransformOp>() {
        return Some(Ok(match name {
            "filter" => {
                let Some(src) = a.first().and_then(|v| v.native::<JImage>()) else {
                    return Some(Ok(V::Null));
                };
                let w = (src.w as f64 * op.0[0]).round().max(1.0) as usize;
                let h = (src.h as f64 * op.0[1]).round().max(1.0) as usize;
                scaled(src, w, h)
            }
            _ => return None,
        }));
    }
    if let Some(c) = this.native::<JColor>() {
        let c = c.0;
        return Some(Ok(match name {
            "getRed" => V::Int(((c >> 16) & 0xff) as i32),
            "getGreen" => V::Int(((c >> 8) & 0xff) as i32),
            "getBlue" => V::Int((c & 0xff) as i32),
            "getAlpha" => V::Int((c >> 24) as i32),
            "getRGB" => V::Int(c as i32),
            "equals" => V::Bool(a.first().and_then(|o| o.native::<JColor>()).is_some_and(|o| o.0 == c)),
            "hashCode" => V::Int(c as i32),
            "toString" => s(&format!("java.awt.Color[r={},g={},b={}]", (c >> 16) & 0xff, (c >> 8) & 0xff, c & 0xff)),
            _ => return None,
        }));
    }
    None
}
