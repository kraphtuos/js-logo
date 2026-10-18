//! Explanatory drawing: a code inside its letter dial, with a spoke from each
//! gap to the letter it selects.

use ringcode::spec;
use ringcode::SvgOptions;
use std::fmt::Write;

const TICK_INNER: f64 = 120.0;
const TICK_OUTER: f64 = 128.0;
const LETTER_RADIUS: f64 = 139.0;

/// Point at `radius` and `deg` clockwise from 12 o'clock.
fn polar(radius: f64, deg: f64) -> (f64, f64) {
    let t = deg.to_radians();
    (radius * t.sin(), -radius * t.cos())
}

/// SVG of `word` on its dial. Colours come from CSS (`currentColor` and
/// `var(--accent)`), so it must be inlined into the page. `word` must be valid.
pub fn svg(word: &str) -> String {
    let slots = spec::parse_word(word).expect("valid word");
    let n = slots.len();
    let mut out = String::new();
    let _ = write!(
        out,
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="-152 -152 304 304" role="img" aria-label="{word} on the letter dial">"#
    );

    // Dial: circle, slot boundary ticks and letters.
    let _ = write!(out, r#"<g fill="none" stroke="currentColor" stroke-opacity="0.35" stroke-width="1">"#);
    let _ = write!(out, r#"<circle r="{TICK_INNER}"/><path d=""#);
    for k in 0..spec::SLOTS {
        let deg = (k as f64 + 0.5) * spec::SLOT_DEG;
        let ((x0, y0), (x1, y1)) = (polar(TICK_INNER, deg), polar(TICK_OUTER, deg));
        let _ = write!(out, "M{x0:.2} {y0:.2}L{x1:.2} {y1:.2}");
    }
    out.push_str(r#""/></g>"#);
    let _ = write!(
        out,
        r#"<g font-family="system-ui, sans-serif" font-size="13" text-anchor="middle" dominant-baseline="central">"#
    );
    for k in 0..spec::SLOTS as u8 {
        let (x, y) = polar(LETTER_RADIUS, spec::slot_angle_deg(k));
        let style = if slots.contains(&k) {
            r#"fill="var(--accent)" font-weight="700""#
        } else {
            r#"fill="currentColor" fill-opacity="0.55""#
        };
        let _ = write!(out, r#"<text x="{x:.2}" y="{y:.2}" {style}>{}</text>"#, spec::slot_letter(k));
    }
    out.push_str("</g>");

    // The code itself, at outer ring radius 100.
    let edge = spec::outer_edge(n) * 100.0;
    let code = ringcode::svg(word, &SvgOptions { foreground: "currentColor".into(), background: None, margin: 0.0 })
        .expect("valid word");
    let code = code.replacen(
        "<svg ",
        &format!(r#"<svg x="{:.3}" y="{:.3}" width="{:.3}" height="{:.3}" "#, -edge, -edge, 2.0 * edge, 2.0 * edge),
        1,
    );
    out.push_str(&code);

    // Spokes from each gap out to its letter.
    let _ = write!(
        out,
        r#"<g stroke="var(--accent)" stroke-width="2" stroke-dasharray="4 3" stroke-linecap="round" fill="var(--accent)">"#
    );
    for (i, &slot) in slots.iter().enumerate() {
        let deg = spec::slot_angle_deg(slot);
        let ((x0, y0), (x1, y1)) = (polar(spec::ring_radius(n, i) * 100.0, deg), polar(TICK_INNER - 2.0, deg));
        let _ = write!(
            out,
            r#"<line x1="{x0:.2}" y1="{y0:.2}" x2="{x1:.2}" y2="{y1:.2}"/><circle cx="{x0:.2}" cy="{y0:.2}" r="3.5" stroke="none"/>"#
        );
    }
    out.push_str("</g></svg>");
    out
}
