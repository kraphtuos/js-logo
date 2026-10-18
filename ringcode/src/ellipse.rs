//! Ellipse model and direct least-squares ellipse fitting.

/// An ellipse with semi-axis `a` along direction `angle` (radians, image
/// coordinates with y down) and semi-axis `b` perpendicular to it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ellipse {
    pub cx: f64,
    pub cy: f64,
    pub a: f64,
    pub b: f64,
    pub angle: f64,
}

impl Ellipse {
    /// Geometric-mean radius.
    pub fn radius(&self) -> f64 {
        (self.a * self.b).sqrt()
    }

    /// Maps an image point into the frame where this ellipse is the unit
    /// circle. The map is the symmetric (rotation-free) stretch, so "up" in
    /// the image stays "up" in the unit frame.
    pub fn to_unit(&self, x: f64, y: f64) -> (f64, f64) {
        let (s, c) = self.angle.sin_cos();
        let (dx, dy) = (x - self.cx, y - self.cy);
        let (u, v) = ((c * dx + s * dy) / self.a, (-s * dx + c * dy) / self.b);
        (c * u - s * v, s * u + c * v)
    }

    /// Inverse of [`Ellipse::to_unit`].
    pub fn from_unit(&self, x: f64, y: f64) -> (f64, f64) {
        let (s, c) = self.angle.sin_cos();
        let (u, v) = ((c * x + s * y) * self.a, (-s * x + c * y) * self.b);
        (self.cx + c * u - s * v, self.cy + s * u + c * v)
    }

    /// The same ellipse scaled about its centre.
    pub fn scaled(&self, k: f64) -> Ellipse {
        Ellipse { a: self.a * k, b: self.b * k, ..*self }
    }

    /// Symmetric conic matrix `C` with `pᵀ C p = 0` for homogeneous points
    /// `p = (x, y, 1)` on the ellipse.
    pub(crate) fn conic(&self) -> M3 {
        let (s, c) = self.angle.sin_cos();
        let (ia, ib) = (1.0 / (self.a * self.a), 1.0 / (self.b * self.b));
        // Q = R diag(1/a², 1/b²) Rᵀ
        let q00 = c * c * ia + s * s * ib;
        let q01 = c * s * (ia - ib);
        let q11 = s * s * ia + c * c * ib;
        let (x, y) = (self.cx, self.cy);
        let (qx, qy) = (q00 * x + q01 * y, q01 * x + q11 * y);
        [[q00, q01, -qx], [q01, q11, -qy], [-qx, -qy, x * qx + y * qy - 1.0]]
    }
}

pub(crate) type M3 = [[f64; 3]; 3];

pub(crate) fn mul(a: &M3, b: &M3) -> M3 {
    let mut out = [[0.0; 3]; 3];
    for i in 0..3 {
        for j in 0..3 {
            out[i][j] = (0..3).map(|k| a[i][k] * b[k][j]).sum();
        }
    }
    out
}

fn transpose(a: &M3) -> M3 {
    let mut out = [[0.0; 3]; 3];
    for i in 0..3 {
        for j in 0..3 {
            out[i][j] = a[j][i];
        }
    }
    out
}

pub(crate) fn det(m: &M3) -> f64 {
    m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1]) - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
        + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0])
}

pub(crate) fn inverse(m: &M3) -> Option<M3> {
    let d = det(m);
    if d.abs() < 1e-300 {
        return None;
    }
    let mut out = [[0.0; 3]; 3];
    for (i, row) in out.iter_mut().enumerate() {
        for (j, v) in row.iter_mut().enumerate() {
            // Adjugate via cyclic cofactors (signs are built in).
            let (j1, j2, i1, i2) = ((j + 1) % 3, (j + 2) % 3, (i + 1) % 3, (i + 2) % 3);
            *v = (m[j1][i1] * m[j2][i2] - m[j1][i2] * m[j2][i1]) / d;
        }
    }
    Some(out)
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

/// Real roots of `x^3 + p2 x^2 + p1 x + p0`.
pub(crate) fn cubic_roots(p2: f64, p1: f64, p0: f64) -> Vec<f64> {
    let shift = p2 / 3.0;
    let p = p1 - p2 * p2 / 3.0;
    let q = 2.0 * p2 * p2 * p2 / 27.0 - p2 * p1 / 3.0 + p0;
    let disc = (q / 2.0).powi(2) + (p / 3.0).powi(3);
    if disc > 0.0 {
        let sq = disc.sqrt();
        vec![(-q / 2.0 + sq).cbrt() + (-q / 2.0 - sq).cbrt() - shift]
    } else {
        let r = (-p / 3.0).sqrt();
        if r == 0.0 {
            return vec![-shift];
        }
        let phi = (-q / (2.0 * r * r * r)).clamp(-1.0, 1.0).acos();
        (0..3).map(|k| 2.0 * r * ((phi - std::f64::consts::TAU * k as f64) / 3.0).cos() - shift).collect()
    }
}

/// Eigenvectors of a general 3×3 matrix for its real eigenvalues.
fn eigenvectors(m: &M3) -> Vec<[f64; 3]> {
    let tr = m[0][0] + m[1][1] + m[2][2];
    let minors = m[0][0] * m[1][1] - m[0][1] * m[1][0] + m[0][0] * m[2][2] - m[0][2] * m[2][0] + m[1][1] * m[2][2]
        - m[1][2] * m[2][1];
    cubic_roots(-tr, minors, -det(m))
        .into_iter()
        .map(|lambda| {
            let mut s = *m;
            for (i, row) in s.iter_mut().enumerate() {
                row[i] -= lambda;
            }
            // The null vector is perpendicular to every row; take the best-conditioned cross product.
            [cross(s[0], s[1]), cross(s[0], s[2]), cross(s[1], s[2])]
                .into_iter()
                .max_by(|a, b| norm2(a).total_cmp(&norm2(b)))
                .unwrap()
        })
        .collect()
}

fn norm2(v: &[f64; 3]) -> f64 {
    v.iter().map(|x| x * x).sum()
}

/// Fits an ellipse to points by direct least squares (Fitzgibbon et al.,
/// in the numerically stable form of Halíř & Flusser). Returns `None` for
/// degenerate input.
pub fn fit(points: &[(f64, f64)]) -> Option<Ellipse> {
    if points.len() < 6 {
        return None;
    }
    // Normalise for conditioning.
    let n = points.len() as f64;
    let mx = points.iter().map(|p| p.0).sum::<f64>() / n;
    let my = points.iter().map(|p| p.1).sum::<f64>() / n;
    let scale = (points.iter().map(|p| (p.0 - mx).powi(2) + (p.1 - my).powi(2)).sum::<f64>() / n).sqrt();
    if scale < 1e-9 {
        return None;
    }

    let mut s1 = [[0.0; 3]; 3];
    let mut s2 = [[0.0; 3]; 3];
    let mut s3 = [[0.0; 3]; 3];
    for &(px, py) in points {
        let (x, y) = ((px - mx) / scale, (py - my) / scale);
        let d1 = [x * x, x * y, y * y];
        let d2 = [x, y, 1.0];
        for i in 0..3 {
            for j in 0..3 {
                s1[i][j] += d1[i] * d1[j];
                s2[i][j] += d1[i] * d2[j];
                s3[i][j] += d2[i] * d2[j];
            }
        }
    }
    let t = mul(&inverse(&s3)?, &transpose(&s2)).map(|row| row.map(|v| -v));
    let m = {
        let st = mul(&s2, &t);
        let mut m = s1;
        for i in 0..3 {
            for j in 0..3 {
                m[i][j] += st[i][j];
            }
        }
        // Premultiply by the inverse of the constraint matrix.
        [m[2].map(|v| v / 2.0), m[1].map(|v| -v), m[0].map(|v| v / 2.0)]
    };

    let a1 = eigenvectors(&m).into_iter().filter(|v| 4.0 * v[0] * v[2] - v[1] * v[1] > 0.0).max_by(|a, b| {
        let ca = (4.0 * a[0] * a[2] - a[1] * a[1]) / norm2(a);
        let cb = (4.0 * b[0] * b[2] - b[1] * b[1]) / norm2(b);
        ca.total_cmp(&cb)
    })?;
    let a2: Vec<f64> = (0..3).map(|i| (0..3).map(|k| t[i][k] * a1[k]).sum()).collect();
    let e = conic_to_ellipse([a1[0], a1[1], a1[2], a2[0], a2[1], a2[2]])?;
    Some(Ellipse { cx: e.cx * scale + mx, cy: e.cy * scale + my, a: e.a * scale, b: e.b * scale, angle: e.angle })
}

/// Converts `A x² + B xy + C y² + D x + E y + F = 0` to ellipse parameters.
fn conic_to_ellipse([a, b, c, d, e, f]: [f64; 6]) -> Option<Ellipse> {
    let den = b * b - 4.0 * a * c;
    if den >= 0.0 {
        return None;
    }
    let cx = (2.0 * c * d - b * e) / den;
    let cy = (2.0 * a * e - b * d) / den;
    let f0 = a * cx * cx + b * cx * cy + c * cy * cy + d * cx + e * cy + f;
    let mid = (a + c) / 2.0;
    let rad = (((a - c) / 2.0).powi(2) + (b / 2.0).powi(2)).sqrt();
    let (l1, l2) = (mid + rad, mid - rad);
    let (sa, sb) = (-f0 / l1, -f0 / l2);
    if !(sa > 0.0 && sb > 0.0 && sa.is_finite() && sb.is_finite()) {
        return None;
    }
    Some(Ellipse { cx, cy, a: sa.sqrt(), b: sb.sqrt(), angle: 0.5 * b.atan2(a - c) })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(e: &Ellipse, from_deg: f64, to_deg: f64, count: usize) -> Vec<(f64, f64)> {
        (0..count)
            .map(|i| {
                let t = (from_deg + (to_deg - from_deg) * i as f64 / count as f64).to_radians();
                e.from_unit(t.cos(), t.sin())
            })
            .collect()
    }

    #[test]
    fn recovers_an_ellipse_from_a_partial_arc() {
        let truth = Ellipse { cx: 120.0, cy: 80.0, a: 60.0, b: 35.0, angle: 0.4 };
        let fit = fit(&sample(&truth, 30.0, 350.0, 200)).unwrap();
        assert!((fit.cx - truth.cx).abs() < 1e-6 && (fit.cy - truth.cy).abs() < 1e-6);
        // The axes may come back swapped; compare as an unordered pair.
        let (mut got, mut want) = ([fit.a, fit.b], [truth.a, truth.b]);
        got.sort_by(f64::total_cmp);
        want.sort_by(f64::total_cmp);
        assert!((got[0] - want[0]).abs() < 1e-6 && (got[1] - want[1]).abs() < 1e-6);
        let (x, y) = fit.to_unit(truth.from_unit(0.6, -0.8).0, truth.from_unit(0.6, -0.8).1);
        assert!((x - 0.6).abs() < 1e-6 && (y + 0.8).abs() < 1e-6);
    }

    #[test]
    fn unit_map_round_trips() {
        let e = Ellipse { cx: 3.0, cy: -2.0, a: 7.0, b: 4.0, angle: 1.1 };
        let (u, v) = e.to_unit(10.0, 5.0);
        let (x, y) = e.from_unit(u, v);
        assert!((x - 10.0).abs() < 1e-9 && (y - 5.0).abs() < 1e-9);
    }

    #[test]
    fn inverse_is_correct() {
        let m = [[4.0, 1.0, 2.0], [0.5, 3.0, 1.0], [2.0, 1.0, 5.0]];
        let p = mul(&m, &inverse(&m).unwrap());
        for (i, row) in p.iter().enumerate() {
            for (j, v) in row.iter().enumerate() {
                assert!((v - if i == j { 1.0 } else { 0.0 }).abs() < 1e-12);
            }
        }
    }
}
