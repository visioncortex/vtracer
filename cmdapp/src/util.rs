use visioncortex::PathSimplifyMode;
use super::Preset;

pub fn path_simplify_mode(s: &str) -> PathSimplifyMode {
    match s {
        "polygon" => PathSimplifyMode::Polygon,
        "spline" => PathSimplifyMode::Spline,
        "none" => PathSimplifyMode::None,
        _ => panic!("unknown PathSimplifyMode {}", s),
    }
}

pub fn preset(s: &str) -> Preset {
    match s {
        "bw" => Preset::Bw,
        "poster" => Preset::Poster,
        "photo" => Preset::Photo,
        _ => panic!("unknown Preset {}", s),
    }
}

pub fn deg2rad(deg: i32) -> f64 {
    deg as f64 / 180.0 * std::f64::consts::PI
}