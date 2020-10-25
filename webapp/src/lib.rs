use wasm_bindgen::prelude::*;

mod conversion;
mod canvas;
mod common;
mod svg;
mod utils;

#[wasm_bindgen(start)]
pub fn main() {
    utils::set_panic_hook();
    console_log::init().unwrap();
}
