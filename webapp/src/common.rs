pub fn window() -> web_sys::Window {
    web_sys::window().unwrap()
}

pub fn document() -> web_sys::Document {
    window().document().unwrap()
}