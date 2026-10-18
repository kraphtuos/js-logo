//! Generate the web app's icons (icon.svg and icons/*.png):
//! `cargo run -p ringcode --example web_icons -- web`

use ringcode::render::{rasterize, Affine};
use ringcode::spec;

const WORD: &str = "RC";
const BACKGROUND: [u8; 3] = [0xff, 0xff, 0xff];
const RINGS: [u8; 3] = [0x00, 0x00, 0x00];

/// How an icon is laid out, as fractions of its size.
struct Layout {
    /// Radius of the code's outer edge.
    code: f64,
    /// Corner radius of the background, or `None` for full bleed.
    corner: Option<f64>,
}

/// Anti-aliased coverage of a rounded square filling the image.
fn rounded_square(size: usize, corner: f64, x: usize, y: usize) -> f64 {
    const SS: usize = 4;
    let (s, r) = (size as f64, corner * size as f64);
    let mut hits = 0;
    for sy in 0..SS {
        for sx in 0..SS {
            let px = x as f64 + (sx as f64 + 0.5) / SS as f64;
            let py = y as f64 + (sy as f64 + 0.5) / SS as f64;
            // Distance into the nearest corner's quarter circle, if in a corner region.
            let dx = (r - px).max(px - (s - r)).max(0.0);
            let dy = (r - py).max(py - (s - r)).max(0.0);
            hits += (dx * dx + dy * dy <= r * r) as usize;
        }
    }
    hits as f64 / (SS * SS) as f64
}

fn icon(size: usize, layout: &Layout) -> image::RgbaImage {
    let slots = spec::parse_word(WORD).expect("valid word");
    let half = size as f64 / 2.0;
    let radius = layout.code * size as f64 / spec::outer_edge(slots.len());
    // Coverage of the rings: 255 = fully covered.
    let rings = rasterize(&slots, size, size, &Affine::place(half - 0.5, half - 0.5, radius, 0.0), 255, 0);
    image::RgbaImage::from_fn(size as u32, size as u32, |x, y| {
        let (x, y) = (x as usize, y as usize);
        let bg = layout.corner.map_or(1.0, |c| rounded_square(size, c, x, y));
        let ring = rings.get(x, y) as f64 / 255.0;
        let rgb: Vec<u8> =
            (0..3).map(|i| (BACKGROUND[i] as f64 * (1.0 - ring) + RINGS[i] as f64 * ring).round() as u8).collect();
        image::Rgba([rgb[0], rgb[1], rgb[2], (bg * 255.0).round() as u8])
    })
}

/// Corner radius of the rounded icons, as a fraction of their size.
const CORNER: f64 = 0.22;

fn main() {
    let web = std::path::PathBuf::from(std::env::args().nth(1).unwrap_or_else(|| "web".into()));
    let dir = web.join("icons");
    std::fs::create_dir_all(&dir).expect("create output directory");

    // Favicon / bookmark icon, rounded like the "any" PNGs.
    let options = ringcode::SvgOptions { corner_radius: CORNER, ..ringcode::SvgOptions::default() };
    let path = web.join("icon.svg");
    std::fs::write(&path, ringcode::svg(WORD, &options).expect("valid word") + "\n").expect("write icon.svg");
    println!("wrote {}", path.display());

    let any = Layout { code: 0.36, corner: Some(CORNER) };
    // Maskable icons may be cropped to a circle of radius 0.4; keep the code well inside it.
    let maskable = Layout { code: 0.3, corner: None };
    // iOS rounds the corners itself and does not want transparency.
    let apple = Layout { code: 0.34, corner: None };
    for (name, size, layout) in [
        ("icon-192.png", 192, &any),
        ("icon-512.png", 512, &any),
        ("icon-maskable-192.png", 192, &maskable),
        ("icon-maskable-512.png", 512, &maskable),
        ("apple-touch-icon.png", 180, &apple),
    ] {
        let path = dir.join(name);
        icon(size, layout).save(&path).expect("write icon");
        println!("wrote {}", path.display());
    }
}
