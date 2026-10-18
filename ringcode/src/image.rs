/// An 8-bit grayscale image, row-major.
#[derive(Clone, Debug)]
pub struct GrayImage {
    pub width: usize,
    pub height: usize,
    pub data: Vec<u8>,
}

impl GrayImage {
    pub fn new(width: usize, height: usize, fill: u8) -> Self {
        GrayImage { width, height, data: vec![fill; width * height] }
    }

    /// Converts RGBA pixels (as from a canvas `ImageData`) to luma.
    pub fn from_rgba(rgba: &[u8], width: usize, height: usize) -> Self {
        assert_eq!(rgba.len(), width * height * 4, "RGBA buffer size mismatch");
        let (pixels, _) = rgba.as_chunks::<4>();
        let data = pixels
            .iter()
            .map(|p| {
                // Composite onto white so transparent pixels read as background.
                let a = p[3] as u32;
                let luma = (77 * p[0] as u32 + 150 * p[1] as u32 + 29 * p[2] as u32) >> 8;
                ((luma * a + 255 * (255 - a)) / 255) as u8
            })
            .collect();
        GrayImage { width, height, data }
    }

    /// Expands to opaque RGBA.
    pub fn to_rgba(&self) -> Vec<u8> {
        self.data.iter().flat_map(|&g| [g, g, g, 255]).collect()
    }

    pub fn get(&self, x: usize, y: usize) -> u8 {
        self.data[y * self.width + x]
    }

    /// Box-filter downscale by an integer factor.
    pub fn downscale(&self, factor: usize) -> GrayImage {
        if factor <= 1 {
            return self.clone();
        }
        let (w, h) = (self.width / factor, self.height / factor);
        let mut out = GrayImage::new(w, h, 0);
        let area = (factor * factor) as u32;
        for y in 0..h {
            for x in 0..w {
                let mut sum = 0u32;
                for yy in y * factor..(y + 1) * factor {
                    let row = &self.data[yy * self.width + x * factor..][..factor];
                    sum += row.iter().map(|&v| v as u32).sum::<u32>();
                }
                out.data[y * w + x] = (sum / area) as u8;
            }
        }
        out
    }

    pub fn inverted(&self) -> GrayImage {
        GrayImage { width: self.width, height: self.height, data: self.data.iter().map(|&v| 255 - v).collect() }
    }
}
