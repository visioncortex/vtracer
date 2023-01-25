use vtracer::{Config, convert_image_to_svg};

fn main() {
    let config = Config::from_args();
    let result = convert_image_to_svg(config);
    match result {
        Ok(res) => {
            println!("{}", res);
        },
        Err(msg) => {
            panic!("Conversion failed with error message: {}", msg);
        }
    }
}
