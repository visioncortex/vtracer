//! Reproducible performance benchmark across sample images and pipeline modes.
//!
//! Run with:
//!   cargo run --release -p vtracer --example bench_perf

use std::path::PathBuf;
use std::time::Instant;

use vtracer::{Clustering, ColorImage, Config, FitMode, Hierarchical};

fn load_sample(name: &str) -> (String, ColorImage) {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.push("../../docs/assets/samples");
    p.push(name);
    let img = image::open(&p).expect("sample image").to_rgba8();
    let (w, h) = (img.width() as usize, img.height() as usize);
    (
        format!("{name} ({w}x{h})"),
        ColorImage {
            pixels: img.into_raw(),
            width: w,
            height: h,
        },
    )
}

fn bench_case(label: &str, img: &ColorImage, cfg: &Config) -> f64 {
    let pipeline = cfg.build().unwrap();

    let t0 = Instant::now();
    let mut seg = pipeline.frontend.segment(img).unwrap();
    let d_seg = t0.elapsed().as_secs_f64() * 1000.0;

    let t1 = Instant::now();
    for fitter in &pipeline.color_fitters {
        fitter.fit(&mut seg);
    }
    let d_col = t1.elapsed().as_secs_f64() * 1000.0;

    let t2 = Instant::now();
    let mut doc = pipeline.compositing.compose(&seg, &pipeline.curve_passes);
    let d_comp = t2.elapsed().as_secs_f64() * 1000.0;

    let t3 = Instant::now();
    for pass in &pipeline.optimizers {
        pass.run(&mut doc);
    }
    let d_opt = t3.elapsed().as_secs_f64() * 1000.0;

    let t4 = Instant::now();
    let svg = pipeline.writer.write(&doc);
    let d_svg = t4.elapsed().as_secs_f64() * 1000.0;

    let total = d_seg + d_col + d_comp + d_opt + d_svg;
    let n_shapes = doc.shapes.len();
    let n_cmds: usize = doc
        .shapes
        .iter()
        .flat_map(|s| &s.path.subpaths)
        .map(|sp| sp.commands.len())
        .sum();

    println!(
        "  {:<42} | Total: {:>7.1} ms | Seg: {:>7.1} | Comp+Fit: {:>6.1} | Opt: {:>4.1} | Svg: {:>6.1} | {:>4} shapes, {:>6} cmds, {:>6.1} KB",
        label,
        total,
        d_seg,
        d_comp,
        d_opt,
        d_svg,
        n_shapes,
        n_cmds,
        svg.len() as f64 / 1024.0
    );
    total
}

fn main() {
    let samples = [
        "Gum Tree Vector.jpg",
        "Cityscape Sunset_DFM3-01.jpg",
        "K1_drawing.jpg",
        "angel-luciano-LATYeZyw88c-unsplash-s.jpg",
    ];

    let mut grand_total = 0.0;
    for name in samples {
        let (desc, img) = load_sample(name);
        println!("\n=== {desc} ===");

        grand_total += bench_case("ColorCluster + Stacked + Spline", &img, &Config::default());
        grand_total += bench_case(
            "ColorCluster + Stacked + Simplify(1.5)",
            &img,
            &Config {
                simplify: Some(1.5),
                ..Config::default()
            },
        );
        grand_total += bench_case(
            "ColorCluster + Cutout (Mosaic) + Spline",
            &img,
            &Config {
                hierarchical: Hierarchical::Cutout,
                ..Config::default()
            },
        );
        grand_total += bench_case(
            "Watershed(128) + Stacked + Spline",
            &img,
            &Config {
                clustering: Clustering::Watershed,
                ..Config::default()
            },
        );
        grand_total += bench_case(
            "Watershed(128) + Cutout + Simplify(1.5)",
            &img,
            &Config {
                clustering: Clustering::Watershed,
                hierarchical: Hierarchical::Cutout,
                simplify: Some(1.5),
                ..Config::default()
            },
        );
        grand_total += bench_case(
            "Binary(Adaptive) + Stacked + Spline",
            &img,
            &Config {
                clustering: Clustering::Binary,
                binary_adaptive: true,
                mode: FitMode::Spline,
                ..Config::default()
            },
        );
    }
    println!("\n>>> GRAND TOTAL ACROSS ALL BENCHMARKS: {:.1} ms <<<", grand_total);
}
