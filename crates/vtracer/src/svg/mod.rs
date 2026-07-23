//! Serialize a [`VectorDoc`] to an SVG string.
//!
//! The writer makes the encoding choices that shrink output without changing
//! geometry:
//!
//! * per segment, the shorter of absolute vs. relative deltas (`L`/`l`, `C`/`c`);
//! * `H`/`V` (`h`/`v`) for axis-aligned lines and `S`/`s` for smooth cubic
//!   continuations;
//! * compact number formatting (trimmed zeros, leading-dot decimals, no
//!   separator before a negative);
//! * optional `<g fill>` grouping of consecutive same-fill shapes.
//!
//! Coordinates are assumed to already be in absolute document space (the
//! [`crate::optimize::QuantizePass`] bakes in any offset), so no per-path
//! `transform` is emitted.

use std::fmt::Write as _;

use visioncortex::PointF64;

use crate::ir::{Paint, PathCmd, Shape, SubPath, VectorDoc};

/// SVG serializer configuration.
#[derive(Debug, Clone, Copy)]
pub struct SvgWriter {
    /// Allow relative commands where they serialize shorter.
    pub relative: bool,
    /// Allow `H`/`V`/`S` shorthands and `<g fill>` grouping.
    pub shorthands: bool,
    /// Decimal places for coordinates (`None` = full precision).
    pub precision: Option<u32>,
}

impl Default for SvgWriter {
    fn default() -> Self {
        Self {
            relative: true,
            shorthands: true,
            precision: Some(2),
        }
    }
}

impl SvgWriter {
    pub fn write(&self, doc: &VectorDoc) -> String {
        let mut out = String::new();
        out.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
        let _ = writeln!(
            out,
            "<!-- Generator: visioncortex VTracer {} -->",
            env!("CARGO_PKG_VERSION")
        );
        let _ = writeln!(
            out,
            "<svg version=\"1.1\" xmlns=\"http://www.w3.org/2000/svg\" width=\"{}\" height=\"{}\">",
            doc.width, doc.height
        );

        if self.shorthands {
            self.write_grouped(&mut out, &doc.shapes);
        } else {
            for shape in &doc.shapes {
                self.write_path(&mut out, shape, true);
            }
        }

        out.push_str("</svg>\n");
        out
    }

    /// Emit shapes, grouping maximal runs of consecutive same-fill shapes into
    /// a single `<g fill>` (preserving paint order).
    fn write_grouped(&self, out: &mut String, shapes: &[Shape]) {
        let mut i = 0;
        while i < shapes.len() {
            let fill = shape_fill(&shapes[i]);
            let mut j = i + 1;
            while j < shapes.len() && shape_fill(&shapes[j]) == fill {
                j += 1;
            }
            let run = &shapes[i..j];
            if run.len() > 1 {
                let _ = writeln!(out, "<g fill=\"{}\">", fill);
                for shape in run {
                    self.write_path(out, shape, false);
                }
                out.push_str("</g>\n");
            } else {
                self.write_path(out, &run[0], true);
            }
            i = j;
        }
    }

    fn write_path(&self, out: &mut String, shape: &Shape, with_fill: bool) {
        let d = self.encode_path(shape);
        if d.is_empty() {
            return;
        }
        if with_fill {
            let _ = writeln!(
                out,
                "<path d=\"{}\" fill=\"{}\"/>",
                d,
                shape_fill(shape)
            );
        } else {
            let _ = writeln!(out, "<path d=\"{}\"/>", d);
        }
    }

    fn encode_path(&self, shape: &Shape) -> String {
        let mut emitter = Emitter::new(self.relative, self.shorthands, self.precision);
        for sub in &shape.path.subpaths {
            emitter.subpath(sub);
        }
        emitter.finish()
    }
}

fn shape_fill(shape: &Shape) -> String {
    match shape.paint {
        Paint::Solid(c) => c.to_hex_string(),
    }
}

/// Streaming SVG-path encoder that tracks the current point.
struct Emitter {
    relative: bool,
    shorthands: bool,
    precision: Option<u32>,
    out: String,
    cur: PointF64,
    started: bool,
    /// Absolute second control point of the previous cubic, for `S` detection.
    prev_cubic_c2: Option<PointF64>,
}

impl Emitter {
    fn new(relative: bool, shorthands: bool, precision: Option<u32>) -> Self {
        Self {
            relative,
            shorthands,
            precision,
            out: String::new(),
            cur: PointF64::default(),
            started: false,
            prev_cubic_c2: None,
        }
    }

    fn finish(self) -> String {
        self.out
    }

    fn subpath(&mut self, sub: &SubPath) {
        for cmd in &sub.commands {
            match *cmd {
                PathCmd::MoveTo(p) => self.move_to(p),
                PathCmd::LineTo(p) => self.line_to(p),
                PathCmd::CubicTo(c1, c2, e) => self.cubic_to(c1, c2, e),
                PathCmd::Close => {
                    self.out.push('Z');
                    self.prev_cubic_c2 = None;
                }
            }
        }
    }

    fn move_to(&mut self, p: PointF64) {
        if !self.started {
            // First move is always absolute.
            let token = format!("M{}", self.coord(p));
            self.out.push_str(&token);
            self.started = true;
        } else {
            let abs = format!("M{}", self.coord(p));
            let token = if self.relative {
                let rel = format!("m{}", self.coord_delta(p));
                shorter(abs, rel)
            } else {
                abs
            };
            self.out.push_str(&token);
        }
        self.cur = p;
        self.prev_cubic_c2 = None;
    }

    fn line_to(&mut self, p: PointF64) {
        let mut candidates: Vec<String> = Vec::new();

        // Axis-aligned shorthands.
        if self.shorthands {
            if p.y == self.cur.y {
                candidates.push(format!("H{}", self.num(p.x)));
                if self.relative {
                    candidates.push(format!("h{}", self.num(p.x - self.cur.x)));
                }
            }
            if p.x == self.cur.x {
                candidates.push(format!("V{}", self.num(p.y)));
                if self.relative {
                    candidates.push(format!("v{}", self.num(p.y - self.cur.y)));
                }
            }
        }

        candidates.push(format!("L{}", self.coord(p)));
        if self.relative {
            candidates.push(format!("l{}", self.coord_delta(p)));
        }

        self.out.push_str(&shortest(candidates));
        self.cur = p;
        self.prev_cubic_c2 = None;
    }

    fn cubic_to(&mut self, c1: PointF64, c2: PointF64, e: PointF64) {
        let mut candidates: Vec<String> = Vec::new();

        // Smooth continuation: c1 is the reflection of the previous cubic's c2.
        if self.shorthands {
            if let Some(prev_c2) = self.prev_cubic_c2 {
                let reflection = PointF64 {
                    x: 2.0 * self.cur.x - prev_c2.x,
                    y: 2.0 * self.cur.y - prev_c2.y,
                };
                if approx(reflection, c1) {
                    candidates.push(format!(
                        "S{}",
                        self.coord_list(&[c2, e])
                    ));
                    if self.relative {
                        candidates.push(format!(
                            "s{}",
                            self.delta_list(&[c2, e])
                        ));
                    }
                }
            }
        }

        candidates.push(format!("C{}", self.coord_list(&[c1, c2, e])));
        if self.relative {
            candidates.push(format!("c{}", self.delta_list(&[c1, c2, e])));
        }

        self.out.push_str(&shortest(candidates));
        self.cur = e;
        self.prev_cubic_c2 = Some(c2);
    }

    // --- number/coordinate formatting -------------------------------------

    fn num(&self, v: f64) -> String {
        fmt_num(v, self.precision)
    }

    /// Absolute coordinate pair.
    fn coord(&self, p: PointF64) -> String {
        join_nums(&[self.num(p.x), self.num(p.y)])
    }

    /// Delta coordinate pair relative to the current point.
    fn coord_delta(&self, p: PointF64) -> String {
        join_nums(&[self.num(p.x - self.cur.x), self.num(p.y - self.cur.y)])
    }

    /// Absolute list of points, flattened.
    fn coord_list(&self, pts: &[PointF64]) -> String {
        let mut nums = Vec::with_capacity(pts.len() * 2);
        for p in pts {
            nums.push(self.num(p.x));
            nums.push(self.num(p.y));
        }
        join_nums(&nums)
    }

    /// Delta list of points relative to the current point (all deltas are from
    /// `cur`, matching SVG's relative-command semantics for multi-point ops).
    fn delta_list(&self, pts: &[PointF64]) -> String {
        let mut nums = Vec::with_capacity(pts.len() * 2);
        for p in pts {
            nums.push(self.num(p.x - self.cur.x));
            nums.push(self.num(p.y - self.cur.y));
        }
        join_nums(&nums)
    }
}

fn approx(a: PointF64, b: PointF64) -> bool {
    (a.x - b.x).abs() < 1e-6 && (a.y - b.y).abs() < 1e-6
}

fn shorter(a: String, b: String) -> String {
    if b.len() < a.len() {
        b
    } else {
        a
    }
}

fn shortest(candidates: Vec<String>) -> String {
    candidates
        .into_iter()
        .min_by_key(|s| s.len())
        .unwrap_or_default()
}

/// Join formatted numbers with the minimal separators SVG allows: a comma,
/// except that a leading `-` is self-separating.
fn join_nums(nums: &[String]) -> String {
    let mut s = String::new();
    for (i, n) in nums.iter().enumerate() {
        if i > 0 && !n.starts_with('-') {
            s.push(',');
        }
        s.push_str(n);
    }
    s
}

/// Compact number formatting: round to precision, trim trailing zeros, use a
/// leading-dot for magnitudes below 1.
fn fmt_num(v: f64, precision: Option<u32>) -> String {
    let v = match precision {
        Some(p) => {
            let factor = 10f64.powi(p as i32);
            (v * factor).round() / factor
        }
        None => v,
    };
    // Normalize -0.0 to 0.
    if v == 0.0 {
        return "0".to_string();
    }

    let mut s = match precision {
        Some(p) => format!("{:.*}", p as usize, v),
        None => format!("{v}"),
    };

    if s.contains('.') {
        while s.ends_with('0') {
            s.pop();
        }
        if s.ends_with('.') {
            s.pop();
        }
    }

    if let Some(rest) = s.strip_prefix("0.") {
        s = format!(".{rest}");
    } else if let Some(rest) = s.strip_prefix("-0.") {
        s = format!("-.{rest}");
    }

    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{MultiPath, Paint, PathCmd, Shape, SubPath};
    use visioncortex::Color;

    #[test]
    fn number_formatting() {
        assert_eq!(fmt_num(0.0, Some(2)), "0");
        assert_eq!(fmt_num(-0.0, Some(2)), "0");
        assert_eq!(fmt_num(1.50, Some(2)), "1.5");
        assert_eq!(fmt_num(0.5, Some(2)), ".5");
        assert_eq!(fmt_num(-0.5, Some(2)), "-.5");
        assert_eq!(fmt_num(2.0, Some(2)), "2");
        assert_eq!(fmt_num(3.14159, Some(2)), "3.14");
    }

    #[test]
    fn join_omits_separator_before_negative() {
        let nums = vec!["1".to_string(), "-2".to_string(), "3".to_string()];
        assert_eq!(join_nums(&nums), "1-2,3");
    }

    fn square_shape() -> Shape {
        use visioncortex::PointF64;
        let p = |x, y| PointF64 { x, y };
        let mut sub = SubPath::new();
        sub.commands = vec![
            PathCmd::MoveTo(p(0.0, 0.0)),
            PathCmd::LineTo(p(10.0, 0.0)),
            PathCmd::LineTo(p(10.0, 10.0)),
            PathCmd::LineTo(p(0.0, 10.0)),
            PathCmd::Close,
        ];
        Shape {
            paint: Paint::Solid(Color::new(255, 0, 0)),
            path: MultiPath { subpaths: vec![sub] },
        }
    }

    #[test]
    fn encodes_axis_aligned_shorthands() {
        let writer = SvgWriter {
            relative: true,
            shorthands: true,
            precision: Some(2),
        };
        let d = writer.encode_path(&square_shape());
        // Horizontal/vertical lines collapse to H/V/h/v; first move is absolute.
        assert!(d.starts_with("M0,0"));
        assert!(d.contains('H') || d.contains('h'));
        assert!(d.contains('V') || d.contains('v'));
        assert!(d.ends_with('Z'));
    }

    #[test]
    fn absolute_mode_uses_no_relative_commands() {
        let writer = SvgWriter {
            relative: false,
            shorthands: false,
            precision: Some(2),
        };
        let d = writer.encode_path(&square_shape());
        assert!(!d.contains('l'));
        assert!(!d.contains('c'));
        assert!(d.contains('L'));
    }
}
