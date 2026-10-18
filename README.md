# Ringcode

Short words written as concentric gapped rings, scanned from a camera like a QR code.
The Jane Street logo is the three-ring code for **JSC** (Jane Street Capital).

Everything runs in the browser: the detector is Rust compiled to WebAssembly, and
images never leave the device.

## The format

- 2–5 concentric rings, one letter each, read from the innermost ring outwards.
- Each ring has one radial gap. The gap's angle picks the letter on a dial of 26
  slots: `A` at 12 o'clock, then clockwise every 360°/26 ≈ 13.85°.
- Geometry, relative to the outer ring's centreline radius: the inner ring sits at
  0.543, the rest are evenly spaced, rings are 0.644 × pitch thick and gaps are
  1.1 × pitch long. These numbers are measured from the Jane Street logo.

Because the inner/outer ratio is fixed, no subset of a code's rings is itself a
valid code, so a ring that goes undetected makes a scan fail instead of reading a
shorter word.

Like a clock face, codes have to be roughly upright: anything within half a slot
(about ±6.9°) snaps to the right letters.

## How detection works

`ringcode/src/detect.rs`, per binarisation (adaptive then Otsu, dark then light rings):

1. Label connected components and keep hollow, roughly round ones.
2. Fit an ellipse (direct least squares) to each and keep those shaped like a ring
   with exactly one gap.
3. For each candidate outer ring, remove perspective: images of two concentric
   circles determine the vanishing line, because `C1 − λC2` is rank one (`l lᵀ`) at the
   pencil's double root. Then normalise the outer ellipse to a unit circle.
4. Check the concentric rings against the radius ladder and thickness for 5, 4, 3 or 2
   rings.
5. Measure every gap's angle, take the single rotation offset that best aligns all gaps
   with the slots, and snap each gap to its letter.

## Layout

| Path | What |
| --- | --- |
| `ringcode/` | Core library: format spec, SVG and raster rendering, detector. No dependencies. |
| `web/` | Browser app in Rust (`web-sys`), built with [trunk](https://trunkrs.dev). |
| `.github/workflows/pages.yml` | Tests, builds and deploys to GitHub Pages. |

## Development

```sh
cargo test -p ringcode                                   # unit + round-trip tests
cargo run -p ringcode --example decode_file -- photo.jpg # decode images from the CLI
cargo run -p ringcode --example svg -- HELLO > hello.svg # make a code

cd web && trunk serve --open                             # app at http://127.0.0.1:8080
```

The camera needs a secure context, which `localhost` and GitHub Pages both provide.
