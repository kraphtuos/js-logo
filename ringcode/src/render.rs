//! Drawing codes: SVG for display/download and an anti-aliased rasteriser.

use crate::image::GrayImage;
use crate::spec::{self, WordError};
use std::fmt::Write;

/// Options for [`svg`].
#[derive(Clone, Debug)]
pub struct SvgOptions {
    /// Ring colour (any CSS colour).
    pub foreground: String,
    /// Background colour, or `None` for transparent.
    pub background: Option<String>,
    /// Empty margin around the code, as a fraction of the outer radius.
    pub margin: f64,
}

impl Default for SvgOptions {
    fn default() -> Self {
        SvgOptions { foreground: "#000".into(), background: Some("#fff".into()), margin: 0.2 }
    }
}

/// Point at `radius` and `deg` clockwise from 12 o'clock (y points down).
fn polar(radius: f64, deg: f64) -> (f64, f64) {
    let t = deg.to_radians();
    (radius * t.sin(), -radius * t.cos())
}

/// Renders `word` as an SVG document.
pub fn svg(word: &str, opts: &SvgOptions) -> Result<String, WordError> {
    let slots = spec::parse_word(word)?;
    let n = slots.len();
    // Work in units of 100 = outer ring centreline radius.
    let unit = 100.0;
    let half = (spec::outer_edge(n) + opts.margin) * unit;
    let t = spec::thickness(n) * unit;

    let mut out = String::new();
    let _ = write!(
        out,
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="{:.3} {:.3} {:.3} {:.3}" role="img" aria-label="ringcode {}">"#,
        -half,
        -half,
        2.0 * half,
        2.0 * half,
        word.to_ascii_uppercase()
    );
    if let Some(bg) = &opts.background {
        let _ = write!(
            out,
            r#"<rect x="{:.3}" y="{:.3}" width="{:.3}" height="{:.3}" fill="{bg}"/>"#,
            -half,
            -half,
            2.0 * half,
            2.0 * half
        );
    }
    let _ = write!(out, r#"<g fill="{}">"#, opts.foreground);
    for (i, &slot) in slots.iter().enumerate() {
        let r = spec::ring_radius(n, i) * unit;
        let (ro, ri) = (r + t / 2.0, r - t / 2.0);
        let gap_half = spec::gap_width_deg(n, i) / 2.0;
        let centre = spec::slot_angle_deg(slot);
        // Annular sector running clockwise from one side of the gap to the other.
        let (start, end) = (centre + gap_half, centre + 360.0 - gap_half);
        let p = [polar(ro, start), polar(ro, end), polar(ri, end), polar(ri, start)];
        let _ = write!(
            out,
            r#"<path d="M{:.3} {:.3}A{ro:.3} {ro:.3} 0 1 1 {:.3} {:.3}L{:.3} {:.3}A{ri:.3} {ri:.3} 0 1 0 {:.3} {:.3}Z"/>"#,
            p[0].0, p[0].1, p[1].0, p[1].1, p[2].0, p[2].1, p[3].0, p[3].1
        );
    }
    out.push_str("</g></svg>");
    Ok(out)
}

/// Whether the point `(x, y)` (in code units, y down) is covered by a ring.
fn covered(slots: &[u8], x: f64, y: f64) -> bool {
    let n = slots.len();
    let r = x.hypot(y);
    let half_t = spec::thickness(n) / 2.0;
    slots.iter().enumerate().any(|(i, &slot)| {
        let rc = spec::ring_radius(n, i);
        if (r - rc).abs() > half_t {
            return false;
        }
        // Radial cut: compare the angle of the point with the gap's half-width.
        let deg = x.atan2(-y).to_degrees();
        let off = (deg - spec::slot_angle_deg(slot)).rem_euclid(360.0);
        let off = off.min(360.0 - off);
        off > spec::gap_width_deg(n, i) / 2.0
    })
}

/// A 2D affine map `(x, y) -> (a x + b y + c, d x + e y + f)`.
#[derive(Clone, Copy, Debug)]
pub struct Affine(pub [f64; 6]);

impl Affine {
    /// Places a code of outer radius `radius` px at `(cx, cy)`, rotated
    /// `rotation_deg` clockwise.
    pub fn place(cx: f64, cy: f64, radius: f64, rotation_deg: f64) -> Affine {
        let (s, c) = rotation_deg.to_radians().sin_cos();
        Affine([radius * c, -radius * s, cx, radius * s, radius * c, cy])
    }

    pub fn then(&self, next: &Affine) -> Affine {
        let [a, b, c, d, e, f] = self.0;
        let [p, q, r, s, t, u] = next.0;
        Affine([p * a + q * d, p * b + q * e, p * c + q * f + r, s * a + t * d, s * b + t * e, s * c + t * f + u])
    }

    pub fn apply(&self, x: f64, y: f64) -> (f64, f64) {
        let [a, b, c, d, e, f] = self.0;
        (a * x + b * y + c, d * x + e * y + f)
    }

    pub fn inverse(&self) -> Affine {
        let [a, b, c, d, e, f] = self.0;
        let det = a * e - b * d;
        let (ia, ib, id, ie) = (e / det, -b / det, -d / det, a / det);
        Affine([ia, ib, -(ia * c + ib * f), id, ie, -(id * c + ie * f)])
    }
}

/// Rasterises `slots` into a `width`×`height` image. `to_image` maps code
/// units (outer ring radius 1) to pixel coordinates; pixel centres are at
/// integer coordinates. Uses 4×4 supersampling.
pub fn rasterize(
    slots: &[u8],
    width: usize,
    height: usize,
    to_image: &Affine,
    foreground: u8,
    background: u8,
) -> GrayImage {
    let inv = to_image.inverse();
    let edge = spec::outer_edge(slots.len()) + 0.05;
    let corners = [(-edge, -edge), (edge, -edge), (-edge, edge), (edge, edge)].map(|(x, y)| to_image.apply(x, y));
    rasterize_with(slots, width, height, &corners, |x, y| inv.apply(x, y), foreground, background)
}

/// Like [`rasterize`] but with an arbitrary image→code mapping, e.g. a
/// perspective one. `bounds` are image points whose bounding box contains the
/// code.
pub fn rasterize_with(
    slots: &[u8],
    width: usize,
    height: usize,
    bounds: &[(f64, f64)],
    to_code: impl Fn(f64, f64) -> (f64, f64),
    foreground: u8,
    background: u8,
) -> GrayImage {
    const SS: usize = 4;
    let mut img = GrayImage::new(width, height, background);
    let clamp = |v: f64, hi: usize| v.max(0.0).min(hi as f64 - 1.0) as usize;
    let x0 = clamp(bounds.iter().map(|p| p.0).fold(f64::MAX, f64::min).floor(), width);
    let x1 = clamp(bounds.iter().map(|p| p.0).fold(f64::MIN, f64::max).ceil(), width);
    let y0 = clamp(bounds.iter().map(|p| p.1).fold(f64::MAX, f64::min).floor(), height);
    let y1 = clamp(bounds.iter().map(|p| p.1).fold(f64::MIN, f64::max).ceil(), height);
    for py in y0..=y1 {
        for px in x0..=x1 {
            let mut hits = 0;
            for sy in 0..SS {
                for sx in 0..SS {
                    let fx = px as f64 + (sx as f64 + 0.5) / SS as f64 - 0.5;
                    let fy = py as f64 + (sy as f64 + 0.5) / SS as f64 - 0.5;
                    let (u, v) = to_code(fx, fy);
                    hits += covered(slots, u, v) as i32;
                }
            }
            let cov = hits as f64 / (SS * SS) as f64;
            img.data[py * width + px] =
                (background as f64 + (foreground as f64 - background as f64) * cov).round() as u8;
        }
    }
    img
}

/// Renders `word` centred in a square image of `size` px (black on white),
/// with the given margin as a fraction of the outer radius.
pub fn render_word(word: &str, size: usize, margin: f64) -> Result<GrayImage, WordError> {
    let slots = spec::parse_word(word)?;
    let half = size as f64 / 2.0;
    let radius = half / (spec::outer_edge(slots.len()) + margin);
    let place = Affine::place(half - 0.5, half - 0.5, radius, 0.0);
    Ok(rasterize(&slots, size, size, &place, 0, 255))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn svg_has_one_path_per_ring() {
        let s = svg("JSC", &SvgOptions::default()).unwrap();
        assert_eq!(s.matches("<path").count(), 3);
        assert!(svg("J", &SvgOptions::default()).is_err());
    }

    #[test]
    fn gap_is_where_the_letter_says() {
        // Ring 0 of "JSC" has its gap at J = slot 9.
        let slots = [9, 18, 2];
        let r = spec::ring_radius(3, 0);
        let (gx, gy) = polar(r, spec::slot_angle_deg(9));
        assert!(!covered(&slots, gx, gy));
        let (ox, oy) = polar(r, spec::slot_angle_deg(9) + 180.0);
        assert!(covered(&slots, ox, oy));
    }

    #[test]
    fn affine_inverse_round_trips() {
        let a = Affine::place(10.0, 20.0, 50.0, 30.0).then(&Affine([1.0, 0.2, 3.0, 0.1, 0.9, -4.0]));
        let (x, y) = a.apply(0.3, -0.7);
        let (u, v) = a.inverse().apply(x, y);
        assert!((u - 0.3).abs() < 1e-9 && (v + 0.7).abs() < 1e-9);
    }
}
