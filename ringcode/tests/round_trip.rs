//! Render codes under various distortions and check they decode.

use ringcode::render::{rasterize, Affine};
use ringcode::{decode, parse_word, GrayImage};

/// Small deterministic PRNG so failures are reproducible.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> f64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 11) as f64 / (1u64 << 53) as f64
    }

    fn range(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (hi - lo) * self.next()
    }
}

fn add_noise(img: &mut GrayImage, rng: &mut Rng, amplitude: f64) {
    for v in &mut img.data {
        *v = (*v as f64 + rng.range(-amplitude, amplitude)).clamp(0.0, 255.0) as u8;
    }
}

/// Darkens one side of the image to mimic uneven lighting.
fn add_gradient(img: &mut GrayImage, strength: f64) {
    let w = img.width as f64;
    for y in 0..img.height {
        for x in 0..img.width {
            let v = &mut img.data[y * img.width + x];
            *v = (*v as f64 * (1.0 - strength * x as f64 / w)) as u8;
        }
    }
}

fn texts(img: &GrayImage) -> Vec<String> {
    decode(img, 1024).into_iter().map(|d| d.text).collect()
}

#[test]
fn decodes_upright_codes() {
    for word in ["JSC", "AB", "ZZ", "QRS", "HELLO", "RUST", "WASM", "AZAZA"] {
        let slots = parse_word(word).unwrap();
        let img = rasterize(&slots, 400, 300, &Affine::place(200.0, 150.0, 110.0, 0.0), 20, 235);
        assert_eq!(texts(&img), vec![word.to_string()], "word {word}");
    }
}

#[test]
fn decodes_under_rotation_scale_shear_noise_and_lighting() {
    let words = ["JSC", "AB", "CODE", "MAGIC", "XY", "PIE", "ZERO", "QUART"];
    let mut rng = Rng(0x9e3779b97f4a7c15);
    let mut failures = Vec::new();
    for trial in 0..120 {
        let word = words[trial % words.len()];
        let slots = parse_word(word).unwrap();
        let (w, h) = (480usize, 360usize);
        let radius = rng.range(55.0, 150.0);
        let place = Affine::place(
            rng.range(radius + 15.0, w as f64 - radius - 15.0),
            rng.range(radius + 15.0, (h as f64 - radius - 15.0).max(radius + 15.0)),
            radius,
            rng.range(-5.0, 5.0),
        );
        // Foreshortening along a random axis, as from a tilted camera.
        let squash = rng.range(0.8, 1.0);
        let axis = rng.range(0.0, std::f64::consts::PI);
        let (s, c) = axis.sin_cos();
        let (cx, cy) = place.apply(0.0, 0.0);
        let k = squash - 1.0;
        let tilt = Affine([1.0 + k * c * c, k * c * s, 0.0, k * c * s, 1.0 + k * s * s, 0.0]);
        let to_image =
            place.then(&Affine([1.0, 0.0, -cx, 0.0, 1.0, -cy])).then(&tilt).then(&Affine([1.0, 0.0, cx, 0.0, 1.0, cy]));
        let dark = trial % 5 == 4;
        let (fg, bg) = if dark { (225, 30) } else { (rng.range(0.0, 60.0) as u8, rng.range(190.0, 255.0) as u8) };
        let mut img = rasterize(&slots, w, h, &to_image, fg, bg);
        add_gradient(&mut img, rng.range(0.0, 0.4));
        add_noise(&mut img, &mut rng, 25.0);
        let got = texts(&img);
        if got != vec![word.to_string()] {
            failures.push(format!("trial {trial}: {word} -> {got:?} (r={radius:.0}, squash={squash:.2}, dark={dark})"));
        }
    }
    assert!(failures.is_empty(), "{} failures:\n{}", failures.len(), failures.join("\n"));
}

type ImageToCode = Box<dyn Fn(f64, f64) -> (f64, f64)>;

/// A camera looking at the code from `tilt_deg` off-axis (rotated about an
/// in-plane axis at `axis_deg`), at `distance` code radii with a pinhole of
/// focal length `focal` px. Returns the image→code map and the code's image
/// bounds.
fn camera(
    tilt_deg: f64,
    axis_deg: f64,
    distance: f64,
    focal: f64,
    centre: (f64, f64),
) -> (ImageToCode, Vec<(f64, f64)>) {
    let (st, ct) = tilt_deg.to_radians().sin_cos();
    let (sa, ca) = axis_deg.to_radians().sin_cos();
    // Rotation about the in-plane axis (ca, sa, 0) by the tilt (Rodrigues).
    let r = [
        [ct + ca * ca * (1.0 - ct), ca * sa * (1.0 - ct), sa * st],
        [ca * sa * (1.0 - ct), ct + sa * sa * (1.0 - ct), -ca * st],
        [-sa * st, ca * st, ct],
    ];
    // Homography code plane (x, y) -> image: K [r1 r2 t] with t = (0, 0, distance).
    let h = [
        [focal * r[0][0] + centre.0 * r[2][0], focal * r[0][1] + centre.0 * r[2][1], centre.0 * distance],
        [focal * r[1][0] + centre.1 * r[2][0], focal * r[1][1] + centre.1 * r[2][1], centre.1 * distance],
        [r[2][0], r[2][1], distance],
    ];
    let forward = |x: f64, y: f64| {
        let w = h[2][0] * x + h[2][1] * y + h[2][2];
        ((h[0][0] * x + h[0][1] * y + h[0][2]) / w, (h[1][0] * x + h[1][1] * y + h[1][2]) / w)
    };
    let e = 1.2;
    let bounds = vec![forward(-e, -e), forward(e, -e), forward(-e, e), forward(e, e)];
    // Inverse via the adjugate.
    let m = h;
    let inv = [
        [
            m[1][1] * m[2][2] - m[1][2] * m[2][1],
            m[0][2] * m[2][1] - m[0][1] * m[2][2],
            m[0][1] * m[1][2] - m[0][2] * m[1][1],
        ],
        [
            m[1][2] * m[2][0] - m[1][0] * m[2][2],
            m[0][0] * m[2][2] - m[0][2] * m[2][0],
            m[0][2] * m[1][0] - m[0][0] * m[1][2],
        ],
        [
            m[1][0] * m[2][1] - m[1][1] * m[2][0],
            m[0][1] * m[2][0] - m[0][0] * m[2][1],
            m[0][0] * m[1][1] - m[0][1] * m[1][0],
        ],
    ];
    let to_code = move |x: f64, y: f64| {
        let w = inv[2][0] * x + inv[2][1] * y + inv[2][2];
        ((inv[0][0] * x + inv[0][1] * y + inv[0][2]) / w, (inv[1][0] * x + inv[1][1] * y + inv[1][2]) / w)
    };
    (Box::new(to_code), bounds)
}

#[test]
fn decodes_under_strong_perspective() {
    // Close-up, wide-angle views at up to 45° of tilt, where the images of the
    // rings are visibly not concentric.
    let mut failures = Vec::new();
    for (i, word) in ["JSC", "HELLO", "AB", "RUST"].iter().enumerate() {
        for (tilt, axis) in [(30.0, 0.0), (40.0, 70.0), (45.0, 135.0), (35.0, 250.0)] {
            let slots = parse_word(word).unwrap();
            let (to_code, bounds) = camera(tilt, axis + 20.0 * i as f64, 3.0, 400.0, (320.0, 240.0));
            let img = ringcode::render::rasterize_with(&slots, 640, 480, &bounds, to_code, 10, 240);
            let got = texts(&img);
            if got != vec![word.to_string()] {
                failures.push(format!("{word} tilt {tilt} axis {axis}: {got:?}"));
            }
        }
    }
    assert!(failures.is_empty(), "{} failures:\n{}", failures.len(), failures.join("\n"));
}

#[test]
fn manual_turns_recover_rotated_codes() {
    let slots = parse_word("JSC").unwrap();
    let slot_deg = 360.0 / 26.0;

    // Turned exactly three slots clockwise: reads as the shifted word, and
    // three manual turns undo it.
    let img = rasterize(&slots, 300, 300, &Affine::place(150.0, 150.0, 120.0, 3.0 * slot_deg + 2.0), 0, 255);
    let found = decode(&img, 1024);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].text, "MVF");
    assert_eq!(found[0].reading(3), "JSC");
    assert_eq!(found[0].reading(0), found[0].text);

    // A quarter turn sits between slots; the right word is still among the readings.
    let img = rasterize(&slots, 300, 300, &Affine::place(150.0, 150.0, 120.0, 90.0), 0, 255);
    let found = decode(&img, 1024);
    assert_eq!(found.len(), 1);
    let readings = found[0].readings();
    assert_eq!(readings.len(), 26);
    let turns = readings.iter().position(|w| w == "JSC").expect("JSC among readings");
    assert!(turns == 6 || turns == 7, "quarter turn is 6.5 slots, got {turns}");
}

#[test]
fn point_at_follows_the_code_into_image_coordinates() {
    // Large enough to be downscaled before detection.
    let slots = parse_word("RUST").unwrap();
    let img = rasterize(&slots, 2400, 1800, &Affine::place(1300.0, 900.0, 500.0, 2.0), 0, 255);
    let found = decode(&img, 1024);
    assert_eq!(found.len(), 1);
    let d = &found[0];
    let outer = d.rings.last().unwrap();
    let (x, y) = d.point_at(outer.gap_deg, 1.0);
    let err = (x - outer.gap_point.0).hypot(y - outer.gap_point.1);
    assert!(err < 1.0, "point_at off by {err:.2}px");
    // 12 o'clock (after removing the measured rotation) is straight above the centre.
    let (tx, ty) = d.point_at(d.rotation_deg, 1.0);
    assert!((tx - 1300.0 - 500.0 * 2f64.to_radians().sin()).abs() < 8.0, "x {tx:.1}");
    assert!((ty - (900.0 - 500.0)).abs() < 8.0, "y {ty:.1}");
}

#[test]
fn reads_several_codes_in_one_image() {
    let mut img = rasterize(&parse_word("ONE").unwrap(), 640, 320, &Affine::place(160.0, 160.0, 120.0, 0.0), 0, 255);
    let two = rasterize(&parse_word("TWO").unwrap(), 640, 320, &Affine::place(480.0, 160.0, 120.0, 0.0), 0, 255);
    for (a, b) in img.data.iter_mut().zip(&two.data) {
        *a = (*a).min(*b);
    }
    let mut got = texts(&img);
    got.sort();
    assert_eq!(got, vec!["ONE", "TWO"]);
}

#[test]
fn tolerates_rotation_up_to_half_a_slot() {
    let slots = parse_word("JSC").unwrap();
    for deg in [-6.0, -3.0, 3.0, 6.0] {
        let img = rasterize(&slots, 300, 300, &Affine::place(150.0, 150.0, 120.0, deg), 0, 255);
        let found = decode(&img, 1024);
        assert_eq!(found.len(), 1, "rotation {deg}");
        assert_eq!(found[0].text, "JSC", "rotation {deg}");
        assert!((found[0].rotation_deg - deg).abs() < 1.0, "rotation {deg}: measured {}", found[0].rotation_deg);
    }
}

#[test]
fn finds_nothing_in_noise_or_plain_circles() {
    let mut rng = Rng(42);
    let mut img = GrayImage::new(320, 240, 200);
    add_noise(&mut img, &mut rng, 80.0);
    assert!(texts(&img).is_empty());

    // Concentric closed circles: no gaps, so no code.
    let mut img = GrayImage::new(300, 300, 255);
    for y in 0..300 {
        for x in 0..300 {
            let r = ((x as f64 - 150.0).powi(2) + (y as f64 - 150.0).powi(2)).sqrt();
            if [60.0, 85.0, 110.0].iter().any(|c: &f64| (r - c).abs() < 8.0) {
                img.data[y * 300 + x] = 0;
            }
        }
    }
    assert!(texts(&img).is_empty());
}

#[test]
fn a_missing_ring_does_not_decode_as_a_shorter_word() {
    // Erase the outer ring of a three-ring code; the remaining two rings have the
    // wrong radius ratio and thickness for a two-ring code.
    let slots = parse_word("JSC").unwrap();
    let mut img = rasterize(&slots, 300, 300, &Affine::place(150.0, 150.0, 120.0, 0.0), 0, 255);
    for y in 0..300 {
        for x in 0..300 {
            let r = ((x as f64 - 150.0).powi(2) + (y as f64 - 150.0).powi(2)).sqrt();
            if r > 105.0 {
                img.data[y * 300 + x] = 255;
            }
        }
    }
    assert!(texts(&img).is_empty(), "got {:?}", texts(&img));
}
