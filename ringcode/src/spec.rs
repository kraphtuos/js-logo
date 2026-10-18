//! The ringcode format.
//!
//! A code is `n` concentric rings (2 ≤ n ≤ 5). Each ring carries one letter,
//! read from the innermost ring outwards. A ring's letter is given by the
//! angular position of the single gap cut into it: the alphabet is laid out
//! clockwise around the circle with `A` at 12 o'clock and one slot every
//! 360°/26 ≈ 13.85°.
//!
//! All lengths are relative to the centreline radius of the outermost ring:
//!
//! * the innermost ring always sits at [`INNER_RADIUS`]; the others are evenly
//!   spaced between it and the outer ring, `pitch = (1 - INNER_RADIUS) / (n - 1)`;
//! * every ring is `THICKNESS * pitch` thick;
//! * every gap is a radial cut whose arc length on the ring's centreline is
//!   `GAP_ARC * pitch`.
//!
//! Fixing the inner/outer ratio (rather than the pitch) means that no proper
//! subset of a code's rings is itself a valid code, so a ring that goes
//! undetected makes a scan fail instead of silently decoding a shorter word.
//!
//! The constants are measured from the Jane Street logo, which is the
//! three-ring code `JSC` (Jane Street Capital).

/// Number of letter slots around the circle.
pub const SLOTS: usize = 26;
/// Angular size of one letter slot, in degrees.
pub const SLOT_DEG: f64 = 360.0 / SLOTS as f64;
/// Fewest rings in a code.
pub const MIN_RINGS: usize = 2;
/// Most rings in a code.
pub const MAX_RINGS: usize = 5;
/// Centreline radius of the innermost ring (outermost ring = 1).
pub const INNER_RADIUS: f64 = 7.08 / 13.04;
/// Ring thickness as a fraction of the pitch.
pub const THICKNESS: f64 = 1.92 / 2.98;
/// Gap arc length (on the ring centreline) as a fraction of the pitch.
pub const GAP_ARC: f64 = 1.1;

/// Errors from validating a word to encode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WordError {
    TooShort,
    TooLong,
    InvalidChar(char),
}

impl std::fmt::Display for WordError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WordError::TooShort => write!(f, "use at least {MIN_RINGS} letters"),
            WordError::TooLong => write!(f, "use at most {MAX_RINGS} letters"),
            WordError::InvalidChar(c) => write!(f, "'{c}' is not a letter A–Z"),
        }
    }
}

impl std::error::Error for WordError {}

/// Validates `word` and converts it to slot indices (`A` = 0). Case-insensitive.
pub fn parse_word(word: &str) -> Result<Vec<u8>, WordError> {
    let slots = word
        .chars()
        .map(|c| match c.to_ascii_uppercase() {
            u @ 'A'..='Z' => Ok(u as u8 - b'A'),
            _ => Err(WordError::InvalidChar(c)),
        })
        .collect::<Result<Vec<_>, _>>()?;
    match slots.len() {
        n if n < MIN_RINGS => Err(WordError::TooShort),
        n if n > MAX_RINGS => Err(WordError::TooLong),
        _ => Ok(slots),
    }
}

/// The letter for a slot index.
pub fn slot_letter(slot: u8) -> char {
    (b'A' + slot % SLOTS as u8) as char
}

/// Ring spacing for an `n`-ring code.
pub fn pitch(n: usize) -> f64 {
    (1.0 - INNER_RADIUS) / (n - 1) as f64
}

/// Centreline radius of ring `i` (0 = innermost) of an `n`-ring code.
pub fn ring_radius(n: usize, i: usize) -> f64 {
    INNER_RADIUS + i as f64 * pitch(n)
}

/// Ring thickness for an `n`-ring code.
pub fn thickness(n: usize) -> f64 {
    THICKNESS * pitch(n)
}

/// Angular width of ring `i`'s gap, in degrees.
pub fn gap_width_deg(n: usize, i: usize) -> f64 {
    (GAP_ARC * pitch(n) / ring_radius(n, i)).to_degrees()
}

/// Radius of the outer edge of the outermost ring.
pub fn outer_edge(n: usize) -> f64 {
    1.0 + thickness(n) / 2.0
}

/// Gap centre angle for a slot, in degrees clockwise from 12 o'clock.
pub fn slot_angle_deg(slot: u8) -> f64 {
    slot as f64 * SLOT_DEG
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn three_rings_match_the_jane_street_logo() {
        // Logo units: outer centreline radius 13.04, rings 1.92 thick.
        let scale = 13.04;
        assert!((ring_radius(3, 0) * scale - 7.08).abs() < 0.01);
        assert!((ring_radius(3, 1) * scale - 10.06).abs() < 0.01);
        assert!((thickness(3) * scale - 1.92).abs() < 0.01);
        assert!((outer_edge(3) * scale - 14.0).abs() < 0.01);
    }

    #[test]
    fn parses_words() {
        assert_eq!(parse_word("jsc"), Ok(vec![9, 18, 2]));
        assert_eq!(parse_word("A"), Err(WordError::TooShort));
        assert_eq!(parse_word("ABCDEF"), Err(WordError::TooLong));
        assert_eq!(parse_word("A1"), Err(WordError::InvalidChar('1')));
    }
}
