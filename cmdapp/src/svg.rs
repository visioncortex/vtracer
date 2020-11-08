use std::fmt;
use visioncortex::{Color, CompoundPath, PointF64};

pub struct SvgFile {
    pub paths: Vec<SvgPath>,
    pub width: usize,
    pub height: usize,
}

pub struct SvgPath {
    pub path: CompoundPath,
    pub color: Color,
}

impl SvgFile {
    pub fn new(width: usize, height: usize) -> Self {
        SvgFile {
            paths: vec![],
            width,
            height,
        }
    }

    pub fn add_path(&mut self, path: CompoundPath, color: Color) {
        self.paths.push(SvgPath {
            path,
            color,
        })
    }
}

impl fmt::Display for SvgFile {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        writeln!(f, r#"<?xml version="1.0" encoding="UTF-8"?>"#)?;
        writeln!(f,
            r#"<svg version="1.1" xmlns="http://www.w3.org/2000/svg" width="{}" height="{}">"#,
            self.width, self.height
        )?;

        for path in &self.paths {
            path.fmt(f)?;
        };

        writeln!(f, "</svg>")
    }
}

impl fmt::Display for SvgPath {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let (string, offset) = self.path.to_svg_string(true, PointF64::default());
        writeln!(
            f, "<path d=\"{}\" fill=\"{}\" transform=\"translate({},{})\"/>",
            string, self.color.to_hex_string(),
            offset.x, offset.y
        )
    }
}