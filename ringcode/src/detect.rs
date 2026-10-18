//! Finding and reading codes in images.
//!
//! Pipeline, per binarisation strategy (adaptive then global threshold, dark
//! then light rings) until something decodes:
//!
//! 1. Label 4-connected foreground components; keep hollow, roughly round ones.
//! 2. Fit an ellipse to each and keep those that look like a ring with exactly
//!    one gap.
//! 3. Taking each ring as a possible outer ring, remove perspective using the
//!    vanishing line implied by it and a ring inside it (see [`View`]), then
//!    gather the rings concentric with it in its normalised (unit-circle)
//!    frame and check their radii and thicknesses against the spec's ladder
//!    for 5, 4, 3 or 2 rings.
//! 4. Measure every gap's angle in that frame, find the single rotation offset
//!    (within half a slot) that best aligns all gaps with the letter slots, and
//!    snap each gap to its slot.
//!
//! Step 4 assumes the code is held roughly upright: a code rotated by more than
//! half a slot (≈ 6.9°) reads as different letters, just as a clock face does.

use crate::ellipse::{self, Ellipse};
use crate::image::GrayImage;
use crate::spec;

/// One ring of a decoded code.
#[derive(Clone, Debug)]
pub struct RingReading {
    /// Ellipse fitted to the ring, in image coordinates.
    pub ellipse: Ellipse,
    /// Measured gap angle, degrees clockwise from 12 o'clock in the code's frame.
    pub gap_deg: f64,
    /// Measured angular width of the gap, degrees.
    pub gap_width_deg: f64,
    /// Centre of the gap on the ring's centreline, in image coordinates.
    pub gap_point: (f64, f64),
    pub letter: char,
    /// Distance from the gap to its slot after removing the common rotation, degrees.
    pub residual_deg: f64,
}

/// A decoded code.
#[derive(Clone, Debug)]
pub struct Detection {
    pub text: String,
    /// Rings from innermost to outermost.
    pub rings: Vec<RingReading>,
    /// Common rotation of the code relative to upright, degrees clockwise.
    pub rotation_deg: f64,
    /// Worst per-ring residual, degrees. Anything under ~3° is a clean read.
    pub max_residual_deg: f64,
    /// Whether a significant perspective correction was applied.
    pub perspective: bool,
}

impl Detection {
    /// The outermost ring's ellipse.
    pub fn outer(&self) -> &Ellipse {
        &self.rings.last().expect("detections have rings").ellipse
    }

    fn map_coords(&mut self, scale: f64, offset: f64) {
        for ring in &mut self.rings {
            let e = &mut ring.ellipse;
            e.cx = e.cx * scale + offset;
            e.cy = e.cy * scale + offset;
            e.a *= scale;
            e.b *= scale;
            ring.gap_point = (ring.gap_point.0 * scale + offset, ring.gap_point.1 * scale + offset);
        }
    }
}

/// Images larger than this (on their longest side) are downscaled before
/// detection.
pub const DEFAULT_MAX_DIMENSION: usize = 1024;

/// Decodes all codes in an RGBA image (e.g. canvas `ImageData`).
pub fn decode_rgba(rgba: &[u8], width: usize, height: usize) -> Vec<Detection> {
    decode(&GrayImage::from_rgba(rgba, width, height), DEFAULT_MAX_DIMENSION)
}

/// Decodes all codes in a grayscale image, downscaling it first so its
/// longest side is at most `max_dimension`.
pub fn decode(img: &GrayImage, max_dimension: usize) -> Vec<Detection> {
    let longest = img.width.max(img.height);
    let factor = longest.div_ceil(max_dimension.max(1)).max(1);
    let small = img.downscale(factor);
    let mut found = detect(&small);
    if factor > 1 {
        // Pixel centre x in the small image covers [x f, (x + 1) f) in the original.
        let offset = (factor as f64 - 1.0) / 2.0;
        for d in &mut found {
            d.map_coords(factor as f64, offset);
        }
    }
    found
}

/// Detection at the image's own resolution.
pub fn detect(img: &GrayImage) -> Vec<Detection> {
    if img.width < 16 || img.height < 16 {
        return Vec::new();
    }
    let inverted = img.inverted();
    let otsu = otsu_threshold(img);
    let strategies: [Box<dyn Fn() -> Vec<bool>>; 4] = [
        Box::new(|| adaptive_dark(img)),
        Box::new(|| adaptive_dark(&inverted)),
        Box::new(|| img.data.iter().map(|&v| v <= otsu).collect()),
        Box::new(|| img.data.iter().map(|&v| v > otsu).collect()),
    ];
    for strategy in strategies {
        let found = detect_in_mask(&strategy(), img.width, img.height);
        if !found.is_empty() {
            return found;
        }
    }
    Vec::new()
}

/// Bradley–Roth adaptive threshold: dark if more than 15% below the local mean.
fn adaptive_dark(img: &GrayImage) -> Vec<bool> {
    let (w, h) = (img.width, img.height);
    let mut integral = vec![0u32; (w + 1) * (h + 1)];
    for y in 0..h {
        let mut row = 0u32;
        for x in 0..w {
            row += img.data[y * w + x] as u32;
            integral[(y + 1) * (w + 1) + x + 1] = integral[y * (w + 1) + x + 1] + row;
        }
    }
    let r = (w.min(h) / 10).max(4);
    let mut mask = vec![false; w * h];
    for y in 0..h {
        let (y0, y1) = (y.saturating_sub(r), (y + r + 1).min(h));
        for x in 0..w {
            let (x0, x1) = (x.saturating_sub(r), (x + r + 1).min(w));
            let sum = integral[y1 * (w + 1) + x1] + integral[y0 * (w + 1) + x0]
                - integral[y0 * (w + 1) + x1]
                - integral[y1 * (w + 1) + x0];
            let count = ((x1 - x0) * (y1 - y0)) as u64;
            mask[y * w + x] = img.data[y * w + x] as u64 * count * 100 < sum as u64 * 85;
        }
    }
    mask
}

fn otsu_threshold(img: &GrayImage) -> u8 {
    let mut hist = [0u64; 256];
    for &v in &img.data {
        hist[v as usize] += 1;
    }
    let total = img.data.len() as f64;
    let sum_all: f64 = hist.iter().enumerate().map(|(i, &c)| i as f64 * c as f64).sum();
    let (mut best, mut best_var) = (127u8, -1.0);
    let (mut w0, mut sum0) = (0.0, 0.0);
    for (t, &count) in hist.iter().enumerate() {
        w0 += count as f64;
        sum0 += t as f64 * count as f64;
        let w1 = total - w0;
        if w0 == 0.0 || w1 == 0.0 {
            continue;
        }
        let between = w0 * w1 * (sum0 / w0 - (sum_all - sum0) / w1).powi(2);
        if between > best_var {
            best_var = between;
            best = t as u8;
        }
    }
    best
}

struct Component {
    area: usize,
    x0: usize,
    y0: usize,
    x1: usize,
    y1: usize,
}

fn find(parent: &mut [u32], mut i: u32) -> u32 {
    while parent[i as usize] != i {
        parent[i as usize] = parent[parent[i as usize] as usize];
        i = parent[i as usize];
    }
    i
}

/// Two-pass 4-connected labelling. Returns per-pixel component ids (0 =
/// background, components numbered from 1) and the components.
fn label(mask: &[bool], w: usize, h: usize) -> (Vec<u32>, Vec<Component>) {
    let mut labels = vec![0u32; w * h];
    let mut parent: Vec<u32> = vec![0];
    for y in 0..h {
        for x in 0..w {
            let i = y * w + x;
            if !mask[i] {
                continue;
            }
            let left = if x > 0 { labels[i - 1] } else { 0 };
            let up = if y > 0 { labels[i - w] } else { 0 };
            labels[i] = match (left, up) {
                (0, 0) => {
                    parent.push(parent.len() as u32);
                    parent.len() as u32 - 1
                }
                (l, 0) | (0, l) => l,
                (l, u) => {
                    let (rl, ru) = (find(&mut parent, l), find(&mut parent, u));
                    let root = rl.min(ru);
                    parent[rl.max(ru) as usize] = root;
                    root
                }
            };
        }
    }
    let mut compact = vec![0u32; parent.len()];
    let mut comps: Vec<Component> = Vec::new();
    for y in 0..h {
        for x in 0..w {
            let i = y * w + x;
            if labels[i] == 0 {
                continue;
            }
            let root = find(&mut parent, labels[i]) as usize;
            if compact[root] == 0 {
                comps.push(Component { area: 0, x0: x, y0: y, x1: x, y1: y });
                compact[root] = comps.len() as u32;
            }
            let id = compact[root];
            labels[i] = id;
            let c = &mut comps[id as usize - 1];
            c.area += 1;
            c.x0 = c.x0.min(x);
            c.x1 = c.x1.max(x);
            c.y1 = y;
        }
    }
    (labels, comps)
}

/// A component that looks like a single gapped ring.
struct Ring {
    ellipse: Ellipse,
    points: Vec<(f64, f64)>,
}

const MIN_RING_AREA: usize = 40;
const MAX_CANDIDATES: usize = 300;
const MAX_POINTS: usize = 6000;

fn stride_sample(points: &[(f64, f64)], limit: usize) -> Vec<(f64, f64)> {
    let step = points.len().div_ceil(limit).max(1);
    points.iter().step_by(step).copied().collect()
}

/// Quantile of an unsorted slice (sorts it).
fn quantile(values: &mut [f64], q: f64) -> f64 {
    values.sort_by(f64::total_cmp);
    values[((values.len() - 1) as f64 * q).round() as usize]
}

/// Largest angular gap between sorted angles (degrees in [0, 360)).
/// Returns (gap centre, gap width, second-largest gap width).
fn largest_gap(angles: &mut [f64]) -> (f64, f64, f64) {
    angles.sort_by(f64::total_cmp);
    let (mut best, mut second, mut centre) = (0.0, 0.0, 0.0);
    for i in 0..angles.len() {
        let (a, b) = if i + 1 < angles.len() { (angles[i], angles[i + 1]) } else { (angles[i], angles[0] + 360.0) };
        let gap = b - a;
        if gap > best {
            second = best;
            best = gap;
            centre = (a + gap / 2.0).rem_euclid(360.0);
        } else if gap > second {
            second = gap;
        }
    }
    (centre, best, second)
}

/// Angle of a unit-frame point, degrees clockwise from 12 o'clock.
fn clock_angle(u: f64, v: f64) -> f64 {
    u.atan2(-v).to_degrees().rem_euclid(360.0)
}

fn analyze_ring(points: Vec<(f64, f64)>) -> Option<Ring> {
    let ellipse = ellipse::fit(&stride_sample(&points, 1500))?;
    if ellipse.radius() < 4.0 || ellipse.a.min(ellipse.b) / ellipse.a.max(ellipse.b) < 0.3 {
        return None;
    }
    let points = stride_sample(&points, MAX_POINTS);
    let mut rhos = Vec::with_capacity(points.len());
    let mut angles = Vec::with_capacity(points.len());
    for &(x, y) in &points {
        let (u, v) = ellipse.to_unit(x, y);
        rhos.push(u.hypot(v));
        angles.push(clock_angle(u, v));
    }
    let (p10, p50, p90) = (quantile(&mut rhos, 0.1), quantile(&mut rhos, 0.5), quantile(&mut rhos, 0.9));
    if !(0.85..=1.15).contains(&p50) || p90 - p10 > 0.7 {
        return None;
    }
    let (_, gap, second) = largest_gap(&mut angles);
    if !(3.0..=90.0).contains(&gap) || second > gap * 0.5 {
        return None;
    }
    Some(Ring { ellipse, points })
}

/// A normalised view of the image around a candidate code: translated and
/// scaled so the outer ring is about unit size, with perspective removed by
/// sending the vanishing line `l0 x + l1 y + 1 = 0` to infinity. In this view
/// the rings are concentric ellipses related to circles by an affine map.
struct View {
    cx: f64,
    cy: f64,
    scale: f64,
    l0: f64,
    l1: f64,
}

impl View {
    /// No perspective correction.
    fn affine(outer: &Ellipse) -> View {
        View { cx: outer.cx, cy: outer.cy, scale: outer.radius(), l0: 0.0, l1: 0.0 }
    }

    /// Perspective correction from the images of two concentric circles.
    ///
    /// For circles `diag(1, 1, -r²)` the pencil `C1 - λ C2` contains the
    /// rank-1 conic `l lᵀ` where `l` is the line at infinity; this survives
    /// any homography, so the same combination of the imaged conics yields the
    /// imaged vanishing line. `λ` is the double root of `det(C1 - λ C2)`.
    fn from_concentric(outer: &Ellipse, inner: &Ellipse) -> Option<View> {
        let mut view = View::affine(outer);
        let normalise = |e: &Ellipse| Ellipse {
            cx: (e.cx - view.cx) / view.scale,
            cy: (e.cy - view.cy) / view.scale,
            a: e.a / view.scale,
            b: e.b / view.scale,
            angle: e.angle,
        };
        let (c1, c2) = (normalise(outer).conic(), normalise(inner).conic());
        let n = ellipse::mul(&ellipse::inverse(&c2)?, &c1);
        let tr = n[0][0] + n[1][1] + n[2][2];
        let minors = n[0][0] * n[1][1] - n[0][1] * n[1][0] + n[0][0] * n[2][2] - n[0][2] * n[2][0] + n[1][1] * n[2][2]
            - n[1][2] * n[2][1];
        let roots = ellipse::cubic_roots(-tr, minors, -ellipse::det(&n));
        let lambda = match roots[..] {
            // Noise split the double root into a complex pair; take its real part.
            [simple] => (tr - simple) / 2.0,
            [a, b, c] => {
                let pairs = [(a, b), (a, c), (b, c)];
                let (p, q) = pairs.into_iter().min_by(|x, y| (x.0 - x.1).abs().total_cmp(&(y.0 - y.1).abs()))?;
                (p + q) / 2.0
            }
            _ => return None,
        };
        let mut m = c1;
        for (i, row) in m.iter_mut().enumerate() {
            for (j, v) in row.iter_mut().enumerate() {
                *v -= lambda * c2[i][j];
            }
        }
        // Dominant eigenvector of the (nearly rank-1, symmetric) combination.
        let mut l = (0..3).map(|j| [m[0][j], m[1][j], m[2][j]]).max_by(|a, b| norm2(a).total_cmp(&norm2(b)))?;
        for _ in 0..20 {
            let next = [0, 1, 2].map(|i| (0..3).map(|k| m[i][k] * l[k]).sum::<f64>());
            let len = norm2(&next).sqrt();
            if len == 0.0 {
                return None;
            }
            l = next.map(|v| v / len);
        }
        if l[2].abs() < 1e-9 {
            return None;
        }
        (view.l0, view.l1) = (l[0] / l[2], l[1] / l[2]);
        // A vanishing line within ~1.7 radii of the centre is not a plausible view of a flat code.
        if view.l0.hypot(view.l1) > 0.6 {
            return None;
        }
        Some(view)
    }

    fn forward(&self, x: f64, y: f64) -> (f64, f64) {
        let (u, v) = ((x - self.cx) / self.scale, (y - self.cy) / self.scale);
        let w = self.l0 * u + self.l1 * v + 1.0;
        (u / w, v / w)
    }

    fn backward(&self, u: f64, v: f64) -> (f64, f64) {
        let w = 1.0 / (1.0 - self.l0 * u - self.l1 * v);
        (self.cx + u * w * self.scale, self.cy + v * w * self.scale)
    }

    /// Whether the correction is large enough to matter (vanishing line
    /// closer than ~30 radii).
    fn corrects_perspective(&self) -> bool {
        self.l0.hypot(self.l1) > 0.03
    }
}

fn norm2(v: &[f64; 3]) -> f64 {
    v.iter().map(|x| x * x).sum()
}

/// A ring considered as part of a code, with its points in the view.
struct Member {
    ring: usize,
    points: Vec<(f64, f64)>,
    /// Median radius in the code frame (outer ring = 1).
    rho: f64,
    /// 10–90% radius spread in the code frame.
    spread: f64,
}

fn radius_stats(frame: &Ellipse, points: &[(f64, f64)]) -> (f64, f64) {
    let mut rhos: Vec<f64> = points
        .iter()
        .map(|&(x, y)| {
            let (u, v) = frame.to_unit(x, y);
            u.hypot(v)
        })
        .collect();
    let (p10, p50, p90) = (quantile(&mut rhos, 0.1), quantile(&mut rhos, 0.5), quantile(&mut rhos, 0.9));
    (p50, p90 - p10)
}

fn detect_in_mask(mask: &[bool], w: usize, h: usize) -> Vec<Detection> {
    let (labels, comps) = label(mask, w, h);

    // Hollow, roughly round components are ring candidates.
    let mut candidate_ids: Vec<usize> = (0..comps.len())
        .filter(|&i| {
            let c = &comps[i];
            let (bw, bh) = (c.x1 - c.x0 + 1, c.y1 - c.y0 + 1);
            c.area >= MIN_RING_AREA
                && bw.min(bh) >= 8
                && bw.max(bh) <= 4 * bw.min(bh)
                && (c.area as f64) <= 0.6 * (bw * bh) as f64
        })
        .collect();
    candidate_ids.sort_by_key(|&i| std::cmp::Reverse(comps[i].area));
    candidate_ids.truncate(MAX_CANDIDATES);

    let mut slot = vec![usize::MAX; comps.len() + 1];
    for (k, &i) in candidate_ids.iter().enumerate() {
        slot[i + 1] = k;
    }
    let mut pixels: Vec<Vec<(f64, f64)>> = candidate_ids.iter().map(|&i| Vec::with_capacity(comps[i].area)).collect();
    for y in 0..h {
        for x in 0..w {
            let k = slot[labels[y * w + x] as usize];
            if k != usize::MAX {
                pixels[k].push((x as f64, y as f64));
            }
        }
    }
    let mut rings: Vec<Ring> = pixels.into_iter().filter_map(analyze_ring).collect();
    rings.sort_by(|a, b| b.ellipse.radius().total_cmp(&a.ellipse.radius()));

    let mut used = vec![false; rings.len()];
    let mut found: Vec<Detection> = Vec::new();
    for o in 0..rings.len() {
        if used[o] {
            continue;
        }
        let outer = rings[o].ellipse;
        // Rings loosely concentric with `o` (perspective shifts inner centres), largest first.
        let inside: Vec<usize> = (0..rings.len())
            .filter(|&c| {
                let ratio = rings[c].ellipse.radius() / outer.radius();
                let (ux, uy) = outer.to_unit(rings[c].ellipse.cx, rings[c].ellipse.cy);
                c != o && !used[c] && (0.35..0.97).contains(&ratio) && ux.hypot(uy) < 0.3
            })
            .collect();
        if inside.is_empty() {
            continue;
        }
        // Try perspective correction against the two largest inner rings, then none.
        let views = inside
            .iter()
            .take(2)
            .filter_map(|&c| View::from_concentric(&outer, &rings[c].ellipse))
            .chain(std::iter::once(View::affine(&outer)));
        for view in views {
            if let Some((detection, members)) = decode_view(&view, o, &inside, &rings) {
                for ring in members {
                    used[ring] = true;
                }
                found.push(detection);
                break;
            }
        }
    }
    found
}

/// Tries to read a code whose outer ring is `o`, in the given view. Returns
/// the detection and the rings it used.
fn decode_view(view: &View, o: usize, inside: &[usize], rings: &[Ring]) -> Option<(Detection, Vec<usize>)> {
    let to_view =
        |ring: usize| -> Vec<(f64, f64)> { rings[ring].points.iter().map(|&(x, y)| view.forward(x, y)).collect() };
    let outer_points = to_view(o);
    let frame = ellipse::fit(&stride_sample(&outer_points, 1500))?;
    // Normalise so the outer ring's own median radius is exactly 1.
    let (norm, outer_spread) = radius_stats(&frame, &outer_points);
    let mut members = vec![Member { ring: o, points: outer_points, rho: 1.0, spread: outer_spread / norm }];
    for &c in inside {
        let points = to_view(c);
        let Some(fit) = ellipse::fit(&stride_sample(&points, 1500)) else { continue };
        let (ux, uy) = frame.to_unit(fit.cx, fit.cy);
        if ux.hypot(uy) > 0.12 {
            continue;
        }
        let (rho, spread) = radius_stats(&frame, &points);
        members.push(Member { ring: c, points, rho: rho / norm, spread: spread / norm });
    }
    let matched = match_ladder(&members)?;
    let detection = read_code(view, &frame, norm, rings, &members, &matched)?;
    Some((detection, matched.iter().map(|&m| members[m].ring).collect()))
}

/// Finds the largest ring count whose radius ladder and ring thickness the
/// members fit. Returns member indices from inner to outer.
fn match_ladder(members: &[Member]) -> Option<Vec<usize>> {
    'counts: for n in (spec::MIN_RINGS..=spec::MAX_RINGS.min(members.len())).rev() {
        let tol = 0.22 * spec::pitch(n);
        let expected_t = spec::thickness(n);
        let mut matched = Vec::with_capacity(n);
        for i in 0..n {
            let want = spec::ring_radius(n, i);
            let best = (0..members.len())
                .filter(|&m| (members[m].rho - want).abs() < tol)
                .min_by(|&a, &b| (members[a].rho - want).abs().total_cmp(&(members[b].rho - want).abs()));
            let Some(m) = best else { continue 'counts };
            // A uniform band of thickness t has a 10–90% spread of 0.8 t.
            let t = members[m].spread / 0.8;
            if !(0.45 * expected_t..=2.2 * expected_t).contains(&t) {
                continue 'counts;
            }
            matched.push(m);
        }
        return Some(matched);
    }
    None
}

fn read_code(
    view: &View,
    frame: &Ellipse,
    norm: f64,
    rings: &[Ring],
    members: &[Member],
    matched: &[usize],
) -> Option<Detection> {
    let n = matched.len();
    let mut gaps = Vec::with_capacity(n);
    for (i, &m) in matched.iter().enumerate() {
        let mut angles: Vec<f64> = members[m]
            .points
            .iter()
            .map(|&(x, y)| {
                let (u, v) = frame.to_unit(x, y);
                clock_angle(u, v)
            })
            .collect();
        let (centre, width, second) = largest_gap(&mut angles);
        let expected = spec::gap_width_deg(n, i);
        if !(0.35 * expected..=3.0 * expected).contains(&width) || second > width * 0.5 {
            return None;
        }
        gaps.push((centre, width));
    }

    // The common rotation is the circular mean of the gap angles taken modulo one slot.
    let k = spec::SLOTS as f64;
    let (s, c) = gaps.iter().fold((0.0, 0.0), |(s, c), &(g, _)| {
        let t = (g * k).to_radians();
        (s + t.sin(), c + t.cos())
    });
    let rotation = s.atan2(c).to_degrees() / k;

    let mut readings = Vec::with_capacity(n);
    let mut text = String::with_capacity(n);
    let mut worst: f64 = 0.0;
    for (&(centre, width), &m) in gaps.iter().zip(matched) {
        let slot = ((centre - rotation) / spec::SLOT_DEG).round().rem_euclid(k) as u8;
        let residual = (centre - rotation - spec::slot_angle_deg(slot) + 180.0).rem_euclid(360.0) - 180.0;
        worst = worst.max(residual.abs());
        let letter = spec::slot_letter(slot);
        text.push(letter);
        let r = members[m].rho * norm;
        let t = centre.to_radians();
        let (gx, gy) = frame.from_unit(r * t.sin(), -r * t.cos());
        readings.push(RingReading {
            ellipse: rings[members[m].ring].ellipse,
            gap_deg: centre,
            gap_width_deg: width,
            gap_point: view.backward(gx, gy),
            letter,
            residual_deg: residual,
        });
    }
    if worst > 4.5 {
        return None;
    }
    Some(Detection {
        text,
        rings: readings,
        rotation_deg: rotation,
        max_residual_deg: worst,
        perspective: view.corrects_perspective(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn largest_gap_wraps_around() {
        let mut a: Vec<f64> = (10..=350).map(|d| d as f64).collect();
        let (centre, width, _) = largest_gap(&mut a);
        assert!((centre - 0.0).abs() < 1e-9 || (centre - 360.0).abs() < 1e-9);
        assert!((width - 20.0).abs() < 1e-9);
    }

    #[test]
    fn labels_separate_components() {
        #[rustfmt::skip]
        let mask = [
            true, true, false, true,
            false, true, false, true,
            true, false, false, true,
        ].to_vec();
        let (_, comps) = label(&mask, 4, 3);
        let mut areas: Vec<usize> = comps.iter().map(|c| c.area).collect();
        areas.sort();
        assert_eq!(areas, vec![1, 3, 3]);
    }

    #[test]
    fn recovers_the_vanishing_line_of_concentric_circles() {
        // Image of circles r = 1 and r = 0.6 under the homography
        // (x, y) -> (x, y) / (0.2 x + 0.1 y + 1): the vanishing line is
        // 0.2 x + 0.1 y + 1 = 0.
        let project = |x: f64, y: f64| {
            let w = 0.2 * x + 0.1 * y + 1.0;
            (100.0 + 80.0 * x / w, 120.0 + 80.0 * y / w)
        };
        let circle = |r: f64| -> Vec<(f64, f64)> {
            (0..360).map(|d| (d as f64).to_radians()).map(|t| project(r * t.cos(), r * t.sin())).collect()
        };
        let outer = ellipse::fit(&circle(1.0)).unwrap();
        let inner = ellipse::fit(&circle(0.6)).unwrap();
        let view = View::from_concentric(&outer, &inner).unwrap();
        // In the rectified view, both images must be concentric circles up to an affine map,
        // so the ratio of their radii measured in the outer frame is constant.
        let rect =
            |pts: Vec<(f64, f64)>| -> Vec<(f64, f64)> { pts.into_iter().map(|(x, y)| view.forward(x, y)).collect() };
        let frame = ellipse::fit(&rect(circle(1.0))).unwrap();
        for (x, y) in rect(circle(0.6)) {
            let (u, v) = frame.to_unit(x, y);
            assert!((u.hypot(v) - 0.6).abs() < 1e-6, "radius {}", u.hypot(v));
        }
    }
}
