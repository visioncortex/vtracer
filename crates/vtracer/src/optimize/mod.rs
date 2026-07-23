//! Optimizer passes over the [`VectorDoc`] before serialization.
//!
//! * [`QuantizePass`] — round every coordinate once, in document space. Doing
//!   it here (rather than at write time) lets [`SimplifyPass`] act on the
//!   rounded geometry, and it bakes offsets into coordinates so the writer
//!   never needs a per-path `translate`.
//! * [`SimplifyPass`] — drop zero-length and collinear-redundant segments that
//!   quantization may have created.

use visioncortex::PointF64;

use crate::ir::{MultiPath, PathCmd, SubPath, VectorDoc};

/// An optimizer pass rewrites the document in place.
pub trait OptimizerPass {
    fn run(&self, doc: &mut VectorDoc);
}

/// Round all coordinates to `precision` decimal places.
#[derive(Debug, Clone, Copy)]
pub struct QuantizePass {
    pub precision: u32,
}

impl QuantizePass {
    pub fn new(precision: u32) -> Self {
        Self { precision }
    }

    fn round(&self, v: f64) -> f64 {
        let factor = 10f64.powi(self.precision as i32);
        (v * factor).round() / factor
    }

    fn round_pt(&self, p: PointF64) -> PointF64 {
        PointF64 {
            x: self.round(p.x),
            y: self.round(p.y),
        }
    }
}

impl OptimizerPass for QuantizePass {
    fn run(&self, doc: &mut VectorDoc) {
        for shape in &mut doc.shapes {
            for sub in &mut shape.path.subpaths {
                for cmd in &mut sub.commands {
                    *cmd = match *cmd {
                        PathCmd::MoveTo(p) => PathCmd::MoveTo(self.round_pt(p)),
                        PathCmd::LineTo(p) => PathCmd::LineTo(self.round_pt(p)),
                        PathCmd::CubicTo(c1, c2, e) => PathCmd::CubicTo(
                            self.round_pt(c1),
                            self.round_pt(c2),
                            self.round_pt(e),
                        ),
                        PathCmd::Close => PathCmd::Close,
                    };
                }
            }
        }
    }
}

/// Remove zero-length segments and collinear-redundant line vertices.
#[derive(Debug, Clone, Copy, Default)]
pub struct SimplifyPass;

/// Tolerance for treating two points as coincident.
const COINCIDENT_EPS: f64 = 1e-6;
/// Perpendicular-distance tolerance for treating three points as collinear.
const COLLINEAR_EPS: f64 = 1e-4;

fn approx_eq(a: PointF64, b: PointF64) -> bool {
    (a.x - b.x).abs() < COINCIDENT_EPS && (a.y - b.y).abs() < COINCIDENT_EPS
}

/// Perpendicular distance of `b` from the line through `a` and `c`.
fn collinear(a: PointF64, b: PointF64, c: PointF64) -> bool {
    let cross = (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x);
    let base = ((c.x - a.x).powi(2) + (c.y - a.y).powi(2)).sqrt();
    if base < COINCIDENT_EPS {
        return true;
    }
    (cross.abs() / base) < COLLINEAR_EPS
}

fn simplify_subpath(sub: &SubPath) -> SubPath {
    let mut out = SubPath::new();
    // `prev` is the point active before the last emitted command; `last` is the
    // current point after it. Both are needed to test collinearity of a run.
    let mut prev = PointF64::default();
    let mut last = PointF64::default();

    for cmd in &sub.commands {
        match *cmd {
            PathCmd::MoveTo(p) => {
                out.commands.push(PathCmd::MoveTo(p));
                prev = p;
                last = p;
            }
            PathCmd::LineTo(p) => {
                if approx_eq(last, p) {
                    continue; // zero-length
                }
                if let Some(PathCmd::LineTo(_)) = out.commands.last() {
                    if collinear(prev, last, p) {
                        *out.commands.last_mut().unwrap() = PathCmd::LineTo(p);
                        last = p; // anchor `prev` unchanged
                        continue;
                    }
                }
                out.commands.push(PathCmd::LineTo(p));
                prev = last;
                last = p;
            }
            PathCmd::CubicTo(c1, c2, e) => {
                out.commands.push(PathCmd::CubicTo(c1, c2, e));
                prev = last;
                last = e;
            }
            PathCmd::Close => {
                out.commands.push(PathCmd::Close);
            }
        }
    }

    out
}

impl OptimizerPass for SimplifyPass {
    fn run(&self, doc: &mut VectorDoc) {
        for shape in &mut doc.shapes {
            let mut subpaths = Vec::with_capacity(shape.path.subpaths.len());
            for sub in &shape.path.subpaths {
                let simplified = simplify_subpath(sub);
                // Keep only subpaths with real geometry (a MoveTo plus at least
                // one drawing command beyond Close).
                let draws = simplified
                    .commands
                    .iter()
                    .filter(|c| matches!(c, PathCmd::LineTo(_) | PathCmd::CubicTo(..)))
                    .count();
                if draws > 0 {
                    subpaths.push(simplified);
                }
            }
            shape.path = MultiPath { subpaths };
        }
        doc.shapes.retain(|s| !s.path.is_empty());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{MultiPath, Paint, Shape};
    use visioncortex::Color;

    fn pt(x: f64, y: f64) -> PointF64 {
        PointF64 { x, y }
    }

    fn doc_with(commands: Vec<PathCmd>) -> VectorDoc {
        let mut doc = VectorDoc::new(100, 100);
        doc.shapes.push(Shape {
            paint: Paint::Solid(Color::new(0, 0, 0)),
            path: MultiPath {
                subpaths: vec![SubPath { commands }],
            },
        });
        doc
    }

    #[test]
    fn quantize_rounds_coordinates() {
        let mut doc = doc_with(vec![
            PathCmd::MoveTo(pt(1.234, 5.678)),
            PathCmd::LineTo(pt(9.876, 0.001)),
            PathCmd::Close,
        ]);
        QuantizePass::new(1).run(&mut doc);
        let cmds = &doc.shapes[0].path.subpaths[0].commands;
        assert_eq!(cmds[0], PathCmd::MoveTo(pt(1.2, 5.7)));
        assert_eq!(cmds[1], PathCmd::LineTo(pt(9.9, 0.0)));
    }

    #[test]
    fn simplify_drops_collinear_and_zero_length() {
        // A straight run of colinear points plus a duplicate should collapse.
        let mut doc = doc_with(vec![
            PathCmd::MoveTo(pt(0.0, 0.0)),
            PathCmd::LineTo(pt(1.0, 0.0)),
            PathCmd::LineTo(pt(2.0, 0.0)), // collinear with previous run
            PathCmd::LineTo(pt(2.0, 0.0)), // zero-length
            PathCmd::LineTo(pt(2.0, 5.0)),
            PathCmd::Close,
        ]);
        SimplifyPass.run(&mut doc);
        let cmds = &doc.shapes[0].path.subpaths[0].commands;
        // MoveTo, one merged horizontal LineTo, one vertical LineTo, Close.
        assert_eq!(cmds.len(), 4);
        assert_eq!(cmds[0], PathCmd::MoveTo(pt(0.0, 0.0)));
        assert_eq!(cmds[1], PathCmd::LineTo(pt(2.0, 0.0)));
        assert_eq!(cmds[2], PathCmd::LineTo(pt(2.0, 5.0)));
        assert_eq!(cmds[3], PathCmd::Close);
    }
}
