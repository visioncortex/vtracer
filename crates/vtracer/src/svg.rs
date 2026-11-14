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
        // Pre-size output buffer roughly from command count to avoid repeated reallocs.
        let est_cmds: usize = doc
            .shapes
            .iter()
            .flat_map(|s| &s.path.subpaths)
            .map(|sp| sp.commands.len())
            .sum();
        let mut out = String::with_capacity(256 + est_cmds * 18);
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

        let mut emitter = Emitter::new(self.relative, self.shorthands, self.precision);
        if self.shorthands {
            self.write_grouped(&mut out, &mut emitter, &doc.shapes);
        } else {
            for shape in &doc.shapes {
                self.write_path(&mut out, &mut emitter, shape, true);
            }
        }

        out.push_str("</svg>\n");
        out
    }

    /// Emit shapes, grouping maximal runs of consecutive same-fill shapes into
    /// a single `<g fill>` (preserving paint order).
    fn write_grouped(&self, out: &mut String, emitter: &mut Emitter, shapes: &[Shape]) {
        let mut i = 0;
        while i < shapes.len() {
            let paint = shapes[i].paint;
            let mut j = i + 1;
            while j < shapes.len() && shapes[j].paint == paint {
                j += 1;
            }
            let run = &shapes[i..j];
            if run.len() > 1 {
                out.push_str("<g fill=\"");
                write_paint_hex(out, paint);
                out.push_str("\">\n");
                for shape in run {
                    self.write_path(out, emitter, shape, false);
                }
                out.push_str("</g>\n");
            } else {
                self.write_path(out, emitter, &run[0], true);
            }
            i = j;
        }
    }

    fn write_path(&self, out: &mut String, emitter: &mut Emitter, shape: &Shape, with_fill: bool) {
        emitter.reset();
        for sub in &shape.path.subpaths {
            emitter.subpath(sub);
        }
        if emitter.out.is_empty() {
            return;
        }
        out.push_str("<path d=\"");
        out.push_str(&emitter.out);
        if with_fill {
            out.push_str("\" fill=\"");
            write_paint_hex(out, shape.paint);
            out.push_str("\"/>\n");
        } else {
            out.push_str("\"/>\n");
        }
    }

    #[cfg(test)]
    fn encode_path(&self, shape: &Shape) -> String {
        let mut emitter = Emitter::new(self.relative, self.shorthands, self.precision);
        for sub in &shape.path.subpaths {
            emitter.subpath(sub);
        }
        emitter.finish()
    }
}

#[inline]
fn write_paint_hex(out: &mut String, paint: Paint) {
    match paint {
        Paint::Solid(c) => {
            const HEX: &[u8; 16] = b"0123456789abcdef";
            let buf = [
                b'#',
                HEX[(c.r >> 4) as usize],
                HEX[(c.r & 0x0f) as usize],
                HEX[(c.g >> 4) as usize],
                HEX[(c.g & 0x0f) as usize],
                HEX[(c.b >> 4) as usize],
                HEX[(c.b & 0x0f) as usize],
            ];
            // SAFETY: buf only contains valid ASCII bytes.
            out.push_str(unsafe { std::str::from_utf8_unchecked(&buf) });
        }
    }
}

/// Fixed-capacity stack buffer for formatting a single SVG command candidate
/// without heap allocation. A cubic command has at most 1 + 6 * 24 + 5 = 150 bytes.
struct SmallBuf {
    bytes: [u8; 256],
    len: usize,
}

impl SmallBuf {
    #[inline]
    fn new(op: u8) -> Self {
        let mut bytes = [0u8; 256];
        bytes[0] = op;
        Self { bytes, len: 1 }
    }

    #[inline]
    fn as_str(&self) -> &str {
        // SAFETY: only ASCII characters are ever written into `bytes`.
        unsafe { std::str::from_utf8_unchecked(&self.bytes[..self.len]) }
    }

    #[inline]
    fn push_byte(&mut self, b: u8) {
        self.bytes[self.len] = b;
        self.len += 1;
    }

    #[inline]
    fn push_slice(&mut self, s: &[u8]) {
        self.bytes[self.len..self.len + s.len()].copy_from_slice(s);
        self.len += s.len();
    }

    /// Append a formatted number, inserting a `,` separator if `not_first` is
    /// true and the formatted number does not start with `-`.
    #[inline]
    fn push_num(&mut self, v: f64, precision: Option<u32>, not_first: bool) {
        let mut nbuf = [0u8; 64];
        let nslice = fmt_num_into(v, precision, &mut nbuf);
        if not_first && nslice.first() != Some(&b'-') {
            self.push_byte(b',');
        }
        self.push_slice(nslice);
    }

    #[inline]
    fn push_coord(&mut self, p: PointF64, precision: Option<u32>) {
        self.push_num(p.x, precision, false);
        self.push_num(p.y, precision, true);
    }

    #[inline]
    fn push_coord_list(&mut self, pts: &[PointF64], precision: Option<u32>) {
        for (i, p) in pts.iter().enumerate() {
            self.push_num(p.x, precision, i > 0);
            self.push_num(p.y, precision, true);
        }
    }

    #[inline]
    fn push_delta_list(&mut self, pts: &[PointF64], cur: PointF64, precision: Option<u32>) {
        for (i, p) in pts.iter().enumerate() {
            self.push_num(p.x - cur.x, precision, i > 0);
            self.push_num(p.y - cur.y, precision, true);
        }
    }
}

/// Streaming SVG-path encoder that tracks the current point and reuses its
/// output buffer across shapes.
struct Emitter {
    relative: bool,
    shorthands: bool,
    precision: Option<u32>,
    out: String,
    cur: PointF64,
    /// Start of the current subpath; `cur` returns here after `Z`.
    subpath_start: PointF64,
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
            out: String::with_capacity(512),
            cur: PointF64::default(),
            subpath_start: PointF64::default(),
            started: false,
            prev_cubic_c2: None,
        }
    }

    #[inline]
    fn reset(&mut self) {
        self.out.clear();
        self.cur = PointF64::default();
        self.subpath_start = PointF64::default();
        self.started = false;
        self.prev_cubic_c2 = None;
    }

    #[cfg(test)]
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
                    // SVG resets the current point to the subpath's start after
                    // Z; a following relative `m`/`l` is measured from there.
                    self.cur = self.subpath_start;
                    self.prev_cubic_c2 = None;
                }
            }
        }
    }

    fn move_to(&mut self, p: PointF64) {
        let mut abs = SmallBuf::new(b'M');
        abs.push_coord(p, self.precision);
        if !self.started || !self.relative {
            self.out.push_str(abs.as_str());
            self.started = true;
        } else {
            let mut rel = SmallBuf::new(b'm');
            rel.push_coord(
                PointF64 {
                    x: p.x - self.cur.x,
                    y: p.y - self.cur.y,
                },
                self.precision,
            );
            if rel.len < abs.len {
                self.out.push_str(rel.as_str());
            } else {
                self.out.push_str(abs.as_str());
            }
        }
        self.cur = p;
        self.subpath_start = p;
        self.prev_cubic_c2 = None;
    }

    fn line_to(&mut self, p: PointF64) {
        let mut best: Option<SmallBuf> = None;
        let mut consider = |cand: SmallBuf| {
            if best.as_ref().map_or(true, |b| cand.len < b.len) {
                best = Some(cand);
            }
        };

        // Axis-aligned shorthands.
        if self.shorthands {
            if p.y == self.cur.y {
                let mut c = SmallBuf::new(b'H');
                c.push_num(p.x, self.precision, false);
                consider(c);
                if self.relative {
                    let mut c = SmallBuf::new(b'h');
                    c.push_num(p.x - self.cur.x, self.precision, false);
                    consider(c);
                }
            }
            if p.x == self.cur.x {
                let mut c = SmallBuf::new(b'V');
                c.push_num(p.y, self.precision, false);
                consider(c);
                if self.relative {
                    let mut c = SmallBuf::new(b'v');
                    c.push_num(p.y - self.cur.y, self.precision, false);
                    consider(c);
                }
            }
        }

        let mut c = SmallBuf::new(b'L');
        c.push_coord(p, self.precision);
        consider(c);
        if self.relative {
            let mut c = SmallBuf::new(b'l');
            c.push_coord(
                PointF64 {
                    x: p.x - self.cur.x,
                    y: p.y - self.cur.y,
                },
                self.precision,
            );
            consider(c);
        }

        if let Some(b) = best {
            self.out.push_str(b.as_str());
        }
        self.cur = p;
        self.prev_cubic_c2 = None;
    }

    fn cubic_to(&mut self, c1: PointF64, c2: PointF64, e: PointF64) {
        let mut best: Option<SmallBuf> = None;
        let mut consider = |cand: SmallBuf| {
            if best.as_ref().map_or(true, |b| cand.len < b.len) {
                best = Some(cand);
            }
        };

        // Smooth continuation: c1 is the reflection of the previous cubic's c2.
        if self.shorthands {
            if let Some(prev_c2) = self.prev_cubic_c2 {
                let reflection = PointF64 {
                    x: 2.0 * self.cur.x - prev_c2.x,
                    y: 2.0 * self.cur.y - prev_c2.y,
                };
                if approx(reflection, c1) {
                    let mut c = SmallBuf::new(b'S');
                    c.push_coord_list(&[c2, e], self.precision);
                    consider(c);
                    if self.relative {
                        let mut c = SmallBuf::new(b's');
                        c.push_delta_list(&[c2, e], self.cur, self.precision);
                        consider(c);
                    }
                }
            }
        }

        let mut c = SmallBuf::new(b'C');
        c.push_coord_list(&[c1, c2, e], self.precision);
        consider(c);
        if self.relative {
            let mut c = SmallBuf::new(b'c');
            c.push_delta_list(&[c1, c2, e], self.cur, self.precision);
            consider(c);
        }

        if let Some(b) = best {
            self.out.push_str(b.as_str());
        }
        self.cur = e;
        self.prev_cubic_c2 = Some(c2);
    }
}

fn approx(a: PointF64, b: PointF64) -> bool {
    (a.x - b.x).abs() < 1e-6 && (a.y - b.y).abs() < 1e-6
}

/// Join formatted numbers with the minimal separators SVG allows: a comma,
/// except that a leading `-` is self-separating.
#[cfg(test)]
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

/// Format an integer `u64` in base 10 into `buf` starting at `pos`, returning
/// the new position.
#[inline]
fn write_u64(mut n: u64, buf: &mut [u8], mut pos: usize) -> usize {
    if n == 0 {
        buf[pos] = b'0';
        return pos + 1;
    }
    let start = pos;
    while n > 0 {
        buf[pos] = b'0' + (n % 10) as u8;
        n /= 10;
        pos += 1;
    }
    buf[start..pos].reverse();
    pos
}

/// Zero-allocation compact number formatter into a stack buffer `buf`.
///
/// Matches the exact behavior of `fmt_num`: rounds to `precision` decimal
/// places, trims trailing zeros, and strips leading `0` before `.` for
/// magnitudes below 1 (`0.5` -> `.5`, `-0.5` -> `-.5`).
#[inline]
fn fmt_num_into(v: f64, precision: Option<u32>, buf: &mut [u8; 64]) -> &[u8] {
    const POW10: [u64; 10] = [
        1,
        10,
        100,
        1_000,
        10_000,
        100_000,
        1_000_000,
        10_000_000,
        100_000_000,
        1_000_000_000,
    ];

    // Fast integer-scaled path for the common case: `Some(p)` with `p <= 9`
    // and values that fit comfortably in a scaled `i64`.
    if let Some(p) = precision {
        if (p as usize) < POW10.len() {
            let scale = POW10[p as usize];
            let scaled_f = (v * scale as f64).round();
            if scaled_f.is_finite() && scaled_f.abs() < (i64::MAX / 2) as f64 {
                let scaled = scaled_f as i64;
                if scaled == 0 {
                    buf[0] = b'0';
                    return &buf[..1];
                }
                let neg = scaled < 0;
                let abs = scaled.unsigned_abs();
                let int_part = abs / scale;
                let mut frac_part = abs % scale;

                let mut pos = 0;
                if neg {
                    buf[pos] = b'-';
                    pos += 1;
                }
                if frac_part == 0 {
                    pos = write_u64(int_part, buf, pos);
                    return &buf[..pos];
                }
                if int_part > 0 {
                    pos = write_u64(int_part, buf, pos);
                }
                buf[pos] = b'.';
                pos += 1;

                // Emit exactly `p` fractional digits, then strip trailing zeros.
                let mut width = p as usize;
                while frac_part % 10 == 0 && width > 0 {
                    frac_part /= 10;
                    width -= 1;
                }
                let frac_end = pos + width;
                let mut cur = frac_end;
                while cur > pos {
                    cur -= 1;
                    buf[cur] = b'0' + (frac_part % 10) as u8;
                    frac_part /= 10;
                }
                return &buf[..frac_end];
            }
        }
    }

    // Fallback path for `precision = None` or extreme exponents.
    let v = match precision {
        Some(p) => {
            let factor = 10f64.powi(p as i32);
            (v * factor).round() / factor
        }
        None => v,
    };
    if v == 0.0 {
        buf[0] = b'0';
        return &buf[..1];
    }

    struct SliceWriter<'a> {
        buf: &'a mut [u8],
        pos: usize,
    }
    impl std::fmt::Write for SliceWriter<'_> {
        fn write_str(&mut self, s: &str) -> std::fmt::Result {
            let b = s.as_bytes();
            self.buf[self.pos..self.pos + b.len()].copy_from_slice(b);
            self.pos += b.len();
            Ok(())
        }
    }
    let mut w = SliceWriter { buf, pos: 0 };
    match precision {
        Some(p) => {
            let _ = write!(w, "{:.*}", p as usize, v);
        }
        None => {
            let _ = write!(w, "{v}");
        }
    }
    let mut len = w.pos;
    if buf[..len].contains(&b'.') {
        while len > 0 && buf[len - 1] == b'0' {
            len -= 1;
        }
        if len > 0 && buf[len - 1] == b'.' {
            len -= 1;
        }
    }
    if len >= 2 && &buf[..2] == b"0." {
        buf.copy_within(1..len, 0);
        len -= 1;
    } else if len >= 3 && &buf[..3] == b"-0." {
        buf.copy_within(2..len, 1);
        len -= 1;
    }
    &buf[..len]
}

/// Compact number formatting: round to precision, trim trailing zeros, use a
/// leading-dot for magnitudes below 1.
#[cfg(test)]
fn fmt_num(v: f64, precision: Option<u32>) -> String {
    let mut buf = [0u8; 64];
    let slice = fmt_num_into(v, precision, &mut buf);
    std::str::from_utf8(slice).unwrap().to_string()
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

    /// A shape with a hole (second subpath). Encoded absolute vs relative must
    /// describe the *same* geometry — regression for the bug where the current
    /// point was not reset to the subpath start after `Z`, so the relative `m`
    /// of the hole was measured from the wrong origin.
    fn holed_shape() -> Shape {
        use visioncortex::PointF64;
        let p = |x, y| PointF64 { x, y };
        let outer = SubPath {
            commands: vec![
                PathCmd::MoveTo(p(0.0, 0.0)),
                PathCmd::LineTo(p(30.0, 0.0)),
                PathCmd::LineTo(p(30.0, 30.0)),
                PathCmd::LineTo(p(0.0, 30.0)),
                PathCmd::Close,
            ],
        };
        let hole = SubPath {
            commands: vec![
                PathCmd::MoveTo(p(10.0, 10.0)),
                PathCmd::LineTo(p(20.0, 10.0)),
                PathCmd::LineTo(p(20.0, 20.0)),
                PathCmd::LineTo(p(10.0, 20.0)),
                PathCmd::Close,
            ],
        };
        Shape {
            paint: Paint::Solid(Color::new(0, 0, 0)),
            path: MultiPath {
                subpaths: vec![outer, hole],
            },
        }
    }

    /// Parse an SVG `d` (M/m/L/l/H/h/V/v/Z only) into absolute points.
    fn parse_abs(d: &str) -> Vec<(f64, f64)> {
        let mut toks = Vec::new();
        let mut i = 0;
        let b = d.as_bytes();
        while i < b.len() {
            let c = b[i] as char;
            if c.is_ascii_alphabetic() {
                toks.push(c.to_string());
                i += 1;
            } else if c == '-' || c == '.' || c.is_ascii_digit() {
                let start = i;
                i += 1;
                while i < b.len() && {
                    let d = b[i] as char;
                    d.is_ascii_digit() || d == '.'
                } {
                    i += 1;
                }
                toks.push(d[start..i].to_string());
            } else {
                i += 1;
            }
        }
        let mut out = Vec::new();
        let (mut cx, mut cy, mut sx, mut sy) = (0.0, 0.0, 0.0, 0.0);
        let mut j = 0;
        let mut cmd = ' ';
        let num = |j: &mut usize| -> f64 {
            let v = toks[*j].parse().unwrap();
            *j += 1;
            v
        };
        while j < toks.len() {
            if toks[j].chars().next().unwrap().is_ascii_alphabetic() {
                cmd = toks[j].chars().next().unwrap();
                j += 1;
            }
            let rel = cmd.is_ascii_lowercase();
            match cmd.to_ascii_uppercase() {
                'M' => {
                    let (mut x, mut y) = (num(&mut j), num(&mut j));
                    if rel {
                        x += cx;
                        y += cy;
                    }
                    cx = x;
                    cy = y;
                    sx = x;
                    sy = y;
                    out.push((cx, cy));
                    cmd = if rel { 'l' } else { 'L' };
                }
                'L' => {
                    let (mut x, mut y) = (num(&mut j), num(&mut j));
                    if rel {
                        x += cx;
                        y += cy;
                    }
                    cx = x;
                    cy = y;
                    out.push((cx, cy));
                }
                'H' => {
                    let mut x = num(&mut j);
                    if rel {
                        x += cx;
                    }
                    cx = x;
                    out.push((cx, cy));
                }
                'V' => {
                    let mut y = num(&mut j);
                    if rel {
                        y += cy;
                    }
                    cy = y;
                    out.push((cx, cy));
                }
                'Z' => {
                    cx = sx;
                    cy = sy;
                }
                _ => unreachable!(),
            }
        }
        out
    }

    #[test]
    fn relative_and_absolute_encode_same_geometry() {
        let shape = holed_shape();
        let abs = SvgWriter {
            relative: false,
            shorthands: false,
            precision: Some(2),
        }
        .encode_path(&shape);
        for shorthands in [false, true] {
            let rel = SvgWriter {
                relative: true,
                shorthands,
                precision: Some(2),
            }
            .encode_path(&shape);
            assert_eq!(
                parse_abs(&abs),
                parse_abs(&rel),
                "relative (shorthands={shorthands}) geometry diverges from absolute:\n abs={abs}\n rel={rel}"
            );
        }
    }
}
