//! Ringcode: short words encoded as concentric gapped rings, readable from a
//! camera like a QR code. The Jane Street logo is the code for `JSC`.
//!
//! See [`spec`] for the format, [`render`] to draw codes and [`detect`] to
//! read them from images.

pub mod detect;
pub mod ellipse;
pub mod image;
pub mod render;
pub mod spec;

pub use detect::{decode, decode_rgba, Detection, RingReading};
pub use ellipse::Ellipse;
pub use image::GrayImage;
pub use render::{render_word, svg, SvgOptions};
pub use spec::{parse_word, WordError};
