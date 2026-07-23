use visioncortex::{BinaryImage, PointI32};

use super::Paint;

/// A region's pixel coverage: a local binary mask positioned on the canvas.
///
/// Foreground pixels are `true`. Holes (interior background) are already
/// punched out of the mask, so a mask is self-describing for tracing.
#[derive(Debug, Clone)]
pub struct RegionMask {
    /// Local coverage; `true` = inside the region.
    pub image: BinaryImage,
    /// Position of the mask's top-left corner in full-canvas coordinates.
    pub offset: PointI32,
}

impl RegionMask {
    pub fn new(image: BinaryImage, offset: PointI32) -> Self {
        Self { image, offset }
    }

    pub fn width(&self) -> usize {
        self.image.width
    }

    pub fn height(&self) -> usize {
        self.image.height
    }

    /// Number of foreground pixels.
    pub fn area(&self) -> usize {
        let mut count = 0;
        for y in 0..self.image.height {
            for x in 0..self.image.width {
                if self.image.get_pixel(x, y) {
                    count += 1;
                }
            }
        }
        count
    }

    /// Combine two masks into one covering the union of their bounding boxes.
    /// Foreground is the OR of both; this is used by the layer-merge step.
    pub fn union(&self, other: &RegionMask) -> RegionMask {
        let left = self.offset.x.min(other.offset.x);
        let top = self.offset.y.min(other.offset.y);
        let right = (self.offset.x + self.image.width as i32)
            .max(other.offset.x + other.image.width as i32);
        let bottom = (self.offset.y + self.image.height as i32)
            .max(other.offset.y + other.image.height as i32);

        let width = (right - left) as usize;
        let height = (bottom - top) as usize;
        let mut image = BinaryImage::new_w_h(width, height);

        for src in [self, other] {
            for y in 0..src.image.height {
                for x in 0..src.image.width {
                    if src.image.get_pixel(x, y) {
                        let gx = (src.offset.x + x as i32 - left) as usize;
                        let gy = (src.offset.y + y as i32 - top) as usize;
                        image.set_pixel(gx, gy, true);
                    }
                }
            }
        }

        RegionMask::new(image, PointI32 { x: left, y: top })
    }
}

/// A single paint layer. Layers are painted bottom-to-top.
#[derive(Debug, Clone)]
pub struct Layer {
    /// Fill applied to the region. Starts as the cluster's mean color; a
    /// [`crate::colorfit::ColorFitter`] may rewrite it.
    pub paint: Paint,
    /// Pixel coverage of the region.
    pub mask: RegionMask,
}

/// Frontend output: ordered layers over a canvas, in paint order.
#[derive(Debug, Clone)]
pub struct Segmentation {
    pub width: u32,
    pub height: u32,
    /// Bottom-to-top paint order.
    pub layers: Vec<Layer>,
}

impl Segmentation {
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            layers: Vec::new(),
        }
    }
}
