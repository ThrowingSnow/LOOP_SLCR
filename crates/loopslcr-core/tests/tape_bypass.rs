//! The bypass guarantee, end to end on the real file.
//!
//! The unit test in `ops::tape` checks that `apply` leaves a buffer alone. This
//! checks the thing that actually matters to a user: that inserting the character
//! stage into the full pipeline changes **the bytes of the written file** not at
//! all while it is switched off. Those are different claims — a stage can be a
//! no-op on the buffer and still cost a rounding, a clone, or a channel order,
//! and this is the only test that would catch it.
//!
//! Skipped when the reference file is absent, like `reference_cut`.

#![allow(clippy::float_arithmetic)]

use std::path::PathBuf;

use loopslcr_core::ops::resample::{Edge, Resampler, SincResampler};
use loopslcr_core::ops::{cut, tape, Fade, TapeParams};
use loopslcr_core::timing::{Align, Grid, Ratio, Tempo, TimeSignature};
use loopslcr_core::wav::{write, BitDepth, Metadata, WriteSpec};
use loopslcr_core::{AudioBuffer, Wav};

const NAME: &str = "103 29Jul26 1Punkt1 Cstc.wav";

fn reference() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("LOOPSLCR_REFERENCE") {
        let p = PathBuf::from(p);
        return p.is_file().then_some(p);
    }
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..").join(NAME);
    root.is_file().then_some(root)
}

/// The pipeline in its binding order, with the character stage optional.
///
/// `tape: None` is the v0.2 chain verbatim — the stage is not merely disabled,
/// it is not called — so a byte-for-byte match against `Some(bypassed)` proves
/// the branch and not just the parameters.
fn pipeline(source: &AudioBuffer, grid: &Grid, bars: u64, tape: Option<&TapeParams>) -> Vec<u8> {
    let region = grid.region(bars, bars, Align::Loop);
    let mut buffer = cut::cut(source, region).buffer;

    Fade::micro(buffer.sample_rate()).apply_both(&mut buffer);

    let ratio = Ratio::from_tempi(grid.tempo, Tempo::bpm(90).unwrap()).unwrap();
    let target = grid.resampled_length(bars, ratio) as usize;
    buffer = SincResampler::default()
        .with_edge(Edge::Wrap)
        .resample(&buffer, target)
        .expect("resample failed");

    if let Some(params) = tape {
        let report = tape::apply(&mut buffer, params, ratio);
        assert_eq!(
            report.is_noop(),
            !params.enabled,
            "the report disagrees with the bypass"
        );
    }

    write::write(
        &buffer,
        &WriteSpec {
            depth: BitDepth::Int24,
            metadata: Metadata::default(),
        },
    )
    .expect("write failed")
}

#[test]
fn a_bypassed_character_stage_changes_no_byte_of_the_output() {
    let Some(path) = reference() else {
        eprintln!("{NAME} not found — skipping tape bypass check");
        return;
    };
    let bytes = std::fs::read(&path).expect("reference unreadable");
    let wav = Wav::parse(&bytes).expect("reference did not parse");
    let source = wav.decode().expect("reference did not decode");
    let grid = Grid::new(
        Tempo::bpm(103).unwrap(),
        TimeSignature::default(),
        source.sample_rate(),
    );

    let clean = pipeline(&source, &grid, 8, None);
    let bypassed = pipeline(&source, &grid, 8, Some(&TapeParams::default()));

    assert_eq!(clean.len(), bypassed.len(), "output length changed");
    assert!(
        clean == bypassed,
        "the bypassed character stage altered the output"
    );

    // And the guard against the test proving nothing: the same pipeline with the
    // character switched on must differ, or a byte-identical result would only
    // mean `apply` was never reached.
    let with = pipeline(&source, &grid, 8, Some(&TapeParams::on()));
    assert_eq!(with.len(), clean.len(), "character changed the length");
    assert_ne!(with, clean, "the character stage did nothing when switched on");
}
