use std::path::PathBuf;
use std::{fs::File, io::Write};

use visioncortex::Color;
use visioncortex::{ColorName, color_clusters::RunnerConfig};
use visioncortex::{ColorImage, color_clusters::Runner};
use super::config::{Config, ConverterConfig};
use super::svg::SvgFile;

pub fn convert_image_to_svg(config: Config) -> Result<(), String> {
    let config = config.into_converter_config();
    if config.color_mode == "color" {
        color_image_to_svg(config)
    } else if config.color_mode == "binary" {
        binary_image_to_svg(config)
    } else {
        Err(format!("Unknown color mode: {}", config.color_mode))
    }
}

pub fn color_image_to_svg(config: ConverterConfig) -> Result<(), String> {
    let (img, width, height);
    match read_image(config.input_path) {
        Ok(values) => {
            img = values.0;
            width = values.1;
            height = values.2;
        },
        Err(msg) => return Err(msg),
    }

    let runner = Runner::new(RunnerConfig {
        batch_size: 25600,
        good_min_area: config.filter_speckle_area,
        good_max_area: (width * height),
        is_same_color_a: config.color_precision_loss,
        is_same_color_b: 1,
        deepen_diff: config.layer_difference,
        hollow_neighbours: 1,
    }, img);

    let clusters = runner.run();

    let view = clusters.view();

    let mut svg = SvgFile::new(width, height);
    for &cluster_index in view.clusters_output.iter().rev() {
        let cluster = view.get_cluster(cluster_index);
        let svg_path = cluster.to_svg_path(
            &view,
            false,
            config.mode,
            config.corner_threshold,
            config.length_threshold,
            config.max_iterations,
            config.splice_threshold
        );
        svg.add_path(svg_path, cluster.residue_color());
    }

    let out_file = File::create(config.output_path);
    let mut out_file = match out_file {
        Ok(file) => file,
        Err(_) => return Err(String::from("Cannot create output file.")),
    };
    
    out_file.write_all(&svg.to_svg_file().as_bytes()).unwrap();

    Ok(())
}

pub fn binary_image_to_svg(config: ConverterConfig) -> Result<(), String> {
    
    let (img, width, height);
    match read_image(config.input_path) {
        Ok(values) => {
            img = values.0;
            width = values.1;
            height = values.2;
        },
        Err(msg) => return Err(msg),
    }
    let img = img.to_binary_image(|x| x.r < 128);

    let clusters = img.to_clusters(false);
    
    let mut svg = SvgFile::new(width, height);
    for i in 0..clusters.len() {
        let cluster = clusters.get_cluster(i);
        if cluster.size() >= config.filter_speckle_area {
            let svg_path = cluster.to_svg_path(
                config.mode,
                config.corner_threshold,
                config.length_threshold,
                config.max_iterations,
                config.splice_threshold,
            );
            let color = Color::color(&ColorName::Black);
            svg.add_path(svg_path, color);
        }
    }

    write_svg(svg, config.output_path)
}

fn read_image(input_path: PathBuf) -> Result<(ColorImage, usize, usize), String> {
    let img = image::open(input_path);
    let img = match img {
        Ok(file) => file.to_rgba(),
        Err(_) => return Err(String::from("No image file found at specified input path")),
    };

    let (width, height) = (img.width() as usize, img.height() as usize);
    let img = ColorImage {pixels: img.as_raw().to_vec(), width, height};

    Ok((img, width, height))
}

fn write_svg(svg: SvgFile, output_path: PathBuf) -> Result<(), String> {
    let out_file = File::create(output_path);
    let mut out_file = match out_file {
        Ok(file) => file,
        Err(_) => return Err(String::from("Cannot create output file.")),
    };
    
    out_file.write_all(&svg.to_svg_file().as_bytes()).unwrap();

    Ok(())
}