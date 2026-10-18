//! Decode codes in image files: `cargo run -p ringcode --example decode_file -- photo.jpg ...`

fn main() {
    let paths: Vec<String> = std::env::args().skip(1).collect();
    if paths.is_empty() {
        eprintln!("usage: decode_file <image>...");
        std::process::exit(2);
    }
    for path in paths {
        let img = match image::open(&path) {
            Ok(img) => img.to_rgba8(),
            Err(e) => {
                eprintln!("{path}: {e}");
                continue;
            }
        };
        let (w, h) = (img.width() as usize, img.height() as usize);
        let start = std::time::Instant::now();
        let found = ringcode::decode_rgba(img.as_raw(), w, h);
        let ms = start.elapsed().as_secs_f64() * 1000.0;
        if found.is_empty() {
            println!("{path}: no code found ({ms:.1} ms)");
        }
        for d in found {
            let outer = d.outer();
            println!(
                "{path}: {} at ({:.0}, {:.0}) r={:.0}px, rotation {:+.1}°, worst residual {:.1}°{} ({ms:.1} ms)",
                d.text,
                outer.cx,
                outer.cy,
                outer.radius(),
                d.rotation_deg,
                d.max_residual_deg,
                if d.perspective { ", perspective-corrected" } else { "" }
            );
            for (i, r) in d.rings.iter().enumerate() {
                println!(
                    "  ring {i}: gap {:6.2}° (width {:5.1}°) -> {} (residual {:+.2}°)",
                    r.gap_deg, r.gap_width_deg, r.letter, r.residual_deg
                );
            }
        }
    }
}
