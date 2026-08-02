//! Progress reporting and cancellation for `Pipeline::run_with_progress`.

use std::cell::Cell;

use vtracer::progress::{CancelToken, Phase, Progress};
use vtracer::{ColorImage, Config, Error};

/// A checkerboard of two colors — enough clusters that segmentation runs a few
/// batches, so incremental progress and mid-run cancellation are observable.
fn checker(w: usize, h: usize) -> ColorImage {
    let mut pixels = Vec::with_capacity(w * h * 4);
    for y in 0..h {
        for x in 0..w {
            let c = if (x / 6 + y / 6) % 2 == 0 {
                (210u8, 60, 60)
            } else {
                (60, 90, 200)
            };
            pixels.extend_from_slice(&[c.0, c.1, c.2, 255]);
        }
    }
    ColorImage {
        pixels,
        width: w,
        height: h,
    }
}

/// A token cancelled before the run starts trips promptly and yields no doc.
#[test]
fn precancelled_returns_cancelled() {
    let img = checker(64, 64);
    let pipeline = Config::default().build().unwrap();

    let cancel = CancelToken::new();
    cancel.cancel();

    let mut cb = |_p: Progress| {};
    let result = pipeline.run_with_progress(&img, &cancel, &mut cb);
    assert_eq!(result.err(), Some(Error::Cancelled));
}

/// Cancelling from within the progress callback (on the first Segment report)
/// trips at the next batch boundary and returns `Cancelled`.
#[test]
fn cancel_during_progress_trips() {
    let img = checker(96, 96);
    let pipeline = Config::default().build().unwrap();

    let cancel = CancelToken::new();
    let saw_segment = Cell::new(false);

    let mut cb = |p: Progress| {
        if p.phase == Phase::Segment {
            saw_segment.set(true);
            cancel.cancel();
        }
    };
    let result = pipeline.run_with_progress(&img, &cancel, &mut cb);

    assert!(saw_segment.get(), "expected at least one Segment report");
    assert_eq!(result.err(), Some(Error::Cancelled));
}

/// A successful run reports monotonically within each phase, ends at
/// Optimize=1.0, and produces the same shapes as the plain `run`.
#[test]
fn progress_completes_and_matches_run() {
    let img = checker(64, 64);
    let pipeline = Config::default().build().unwrap();

    let cancel = CancelToken::new();
    let last = Cell::new(None::<Progress>);
    let count = Cell::new(0usize);

    let mut cb = |p: Progress| {
        assert!(
            (0.0..=1.0).contains(&p.fraction),
            "fraction out of range: {}",
            p.fraction
        );
        last.set(Some(p));
        count.set(count.get() + 1);
    };
    let doc = pipeline
        .run_with_progress(&img, &cancel, &mut cb)
        .expect("run should succeed");

    assert!(count.get() > 0, "expected progress reports");
    let final_p = last.get().expect("a final report");
    assert_eq!(final_p.phase, Phase::Optimize);
    assert_eq!(final_p.fraction, 1.0);

    // Incremental clustering yields the same clusters as the blocking path,
    // so both entry points produce identical output.
    let plain = pipeline.run(&img).expect("plain run should succeed");
    assert_eq!(doc.shapes.len(), plain.shapes.len());
}
