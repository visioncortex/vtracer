use web_sys::Element;
use visioncortex::{Color, CompoundPath, PointF64};
use super::common::document;

pub struct Svg {
    element: Element,
}

impl Svg {
    pub fn new_from_id(svg_id: &str) -> Self {
        let element = document().get_element_by_id(svg_id).unwrap();

        Self { element }
    }

    pub fn prepend_path(&mut self, paths: &CompoundPath, color: &Color, precision: Option<u32>) {
        let path = document()
            .create_element_ns(Some("http://www.w3.org/2000/svg"), "path")
            .unwrap();
        let (string, offset) = paths.to_svg_string(true, PointF64::default(), precision);
        path.set_attribute("d", &string).unwrap();
        path.set_attribute(
            "transform",
            format!("translate({},{})", offset.x, offset.y).as_str(),
        )
        .unwrap();
        path.set_attribute(
            "style",
            format!("fill: {};", color.to_hex_string()).as_str(),
        )
        .unwrap();
        self.element.prepend_with_node_1(&path).unwrap();
    }
}
