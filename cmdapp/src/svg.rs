use visioncortex::Color;

pub struct SvgPath {
    path: String,
    color: Color,
}

pub struct SvgFile {
    patches: Vec<SvgPath>,
    width: usize,
    height: usize,
}

impl SvgFile {
    pub fn new(width: usize, height: usize) -> Self {
        SvgFile {
            patches: vec![],
            width,
            height,
        }
    }

    pub fn add_path(&mut self, path: String, color: Color) {
        self.patches.push(SvgPath {
            path,
            color
        })
    }

    pub fn to_svg_file(&self) -> String {
        let mut result: Vec<String> = vec![format!(r#"<?xml version="1.0" encoding="UTF-8"?>
<svg version="1.1" xmlns="http://www.w3.org/2000/svg" width="{}" height="{}">"#, self.width, self.height)];

        for patch in &self.patches {
            let color = patch.color;
            result.push(format!("<path d=\"{}\" fill=\"#{:02x}{:02x}{:02x}\"/>\n", patch.path, color.r, color.g, color.b));
        };

        result.push(String::from("</svg>"));
        result.concat()
    }
}