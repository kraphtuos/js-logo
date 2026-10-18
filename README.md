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

Like a clock face, a code only reads correctly if you know which way is up. The
scanner assumes upright and tolerates half a slot (about ±6.9°) of error. Because
a code's gaps always sit a whole number of slots apart, rotating a code only
shifts every letter by the same amount, so any rotation reads as one of 26 words.
The app lets you fix the orientation by hand: step the top round one slot at a
time (buttons, `[` `]` or arrow keys), or pick from the list of all 26 readings.
An arrow on the overlay shows where the code's top is assumed to be.

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
| `web/` | Browser app in Rust (`web-sys`), built with [trunk](https://trunkrs.dev); PWA manifest, service worker and icons. |
| `.github/workflows/pages.yml` | Tests, builds and deploys to GitHub Pages. |

## Development

```sh
cargo test -p ringcode                                   # unit + round-trip tests
cargo run -p ringcode --example decode_file -- photo.jpg # decode images from the CLI
cargo run -p ringcode --example svg -- HELLO > hello.svg # make a code

cargo run -p ringcode --example web_icons -- web/icons   # regenerate the app icons

cd web && trunk serve --open                             # app at http://127.0.0.1:8080
```

The camera and the service worker need a secure context, which `localhost` and
GitHub Pages both provide.

The app is an installable PWA (`web/manifest.webmanifest`). `web/sw.js` makes it
work offline after the first visit: pages are fetched network-first, trunk's
content-hashed assets are cached permanently, and assets from older builds are
pruned when a new page arrives.
