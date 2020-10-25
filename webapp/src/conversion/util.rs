use visioncortex::PathSimplifyMode;

pub fn path_simplify_mode(s: &str) -> PathSimplifyMode {
	match s {
		"polygon" => PathSimplifyMode::Polygon,
		"spline" => PathSimplifyMode::Spline,
		"none" => PathSimplifyMode::None,
		_ => panic!("unknown PathSimplifyMode {}", s),
	}
}