//! Color fitters: rewrite layer paints before compositing.
//!
//! * [`Identity`] — keep the frontend's mean colors (0.6.x behavior).
//! * [`FixedPalette`] — snap each paint to the nearest entry of a fixed
//!   palette, measured in OKLab.
//! * [`AutoQuantize`] — reduce the palette to at most `max_colors` via
//!   area-weighted median cut.
//! * [`MergeAdjacent`] — union consecutive layers that share a paint, cutting
//!   shape count for free.

mod merge;
mod oklab;
mod palette;
mod quantize;

pub use merge::MergeAdjacent;
pub use palette::FixedPalette;
pub use quantize::AutoQuantize;

use crate::ir::Segmentation;

/// A color fitter rewrites the paints of a segmentation in place.
pub trait ColorFitter {
    fn fit(&self, seg: &mut Segmentation);
}

/// No-op fitter: paints keep the frontend's mean cluster colors.
#[derive(Debug, Clone, Default)]
pub struct Identity;

impl ColorFitter for Identity {
    fn fit(&self, _seg: &mut Segmentation) {}
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{Layer, Paint, RegionMask};
    use visioncortex::{BinaryImage, Color, PointI32};

    fn layer(color: Color) -> Layer {
        let mut image = BinaryImage::new_w_h(1, 1);
        image.set_pixel(0, 0, true);
        Layer {
            paint: Paint::Solid(color),
            mask: RegionMask::new(image, PointI32 { x: 0, y: 0 }),
        }
    }

    #[test]
    fn fixed_palette_snaps_to_nearest_oklab() {
        let mut seg = Segmentation::new(1, 1);
        seg.layers.push(layer(Color::new(250, 10, 10))); // near red
        seg.layers.push(layer(Color::new(10, 10, 250))); // near blue

        let palette = FixedPalette::new(vec![Color::new(255, 0, 0), Color::new(0, 0, 255)]);
        palette.fit(&mut seg);

        assert_eq!(seg.layers[0].paint, Paint::Solid(Color::new(255, 0, 0)));
        assert_eq!(seg.layers[1].paint, Paint::Solid(Color::new(0, 0, 255)));
    }

    #[test]
    fn merge_adjacent_unions_same_paint_runs() {
        let mut seg = Segmentation::new(2, 1);
        seg.layers.push(layer(Color::new(0, 0, 0)));
        seg.layers.push(layer(Color::new(0, 0, 0)));
        seg.layers.push(layer(Color::new(255, 255, 255)));

        MergeAdjacent.fit(&mut seg);

        assert_eq!(seg.layers.len(), 2);
        assert_eq!(seg.layers[0].paint, Paint::Solid(Color::new(0, 0, 0)));
    }
}
