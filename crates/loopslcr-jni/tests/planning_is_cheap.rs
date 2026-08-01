//! The claim that made `Stage::Plan` worth building, kept honest.
//!
//! A UI calls `plan` while a finger is still moving, so the number that matters
//! is not "is it fast" but "how much of the full run did it avoid". On the
//! reference file the full pipeline with varispeed resamples 940 800 frames
//! through a 32-tap windowed sinc; planning does not resample at all.
//!
//! Run against the real file with:
//!
//! ```console
//! $ LOOPSLCR_REFERENCE="103 29Jul26 1Punkt1 Cstc.wav" cargo test -p loopslcr-jni \
//!       --test planning_is_cheap --release -- --nocapture
//! ```
//!
//! Skipped without it, because a timing test that runs in CI on whatever
//! hardware is there measures the machine, not the change.

use std::time::Instant;

#[test]
fn planning_avoids_the_resampling_that_dominates_a_full_run() {
    let Ok(path) = std::env::var("LOOPSLCR_REFERENCE") else {
        eprintln!("LOOPSLCR_REFERENCE not set — skipping the timing comparison");
        return;
    };
    let bytes = std::fs::read(&path).expect("could not read the reference file");
    let name = std::path::Path::new(&path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("loop.wav");
    let params = r#"{"targetBpm":90}"#;

    let started = Instant::now();
    let plan = loopslcr_jni::api::plan(&bytes, name, params).expect("plan");
    let planning = started.elapsed();

    let started = Instant::now();
    let out = loopslcr_jni::api::process(&bytes, name, params).expect("process");
    let full = started.elapsed();

    eprintln!("plan    {planning:?}");
    eprintln!("process {full:?}  ({} bytes)", out.len());

    // Both must agree on the length, or the cheap stage is cheap by being wrong.
    assert!(plan.contains("\"outputFrames\":940800"), "{plan}");

    // The point of the exercise. A factor of ten is a conservative floor for
    // "no longer the thing the user is waiting for"; measured, it is far more.
    assert!(
        planning * 10 < full,
        "planning {planning:?} is not much cheaper than the full run {full:?}"
    );
}
