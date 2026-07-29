//! The whole chain on the real reference file: read, take the region the bar
//! grid names, write it back out, read it again.
//!
//! Skipped when the file is absent — it is 11 MB of audio and not repository
//! content. Point `LOOPSLCR_REFERENCE` at it, or leave it in the repository
//! root where it will be found automatically.
//!
//! This is the test the synthetic ones cannot replace: a file built by
//! `test_support` is a file that agrees with our assumptions, and it was a real
//! Caustic export that exposed the RIFF size field costing audio.

#![allow(clippy::float_arithmetic)]

use std::path::PathBuf;

use loopslcr_core::timing::{Align, Grid, Tempo, TimeSignature};
use loopslcr_core::wav::{write, BitDepth, Metadata, WriteSpec};
use loopslcr_core::Wav;

const NAME: &str = "103 29Jul26 1Punkt1 Cstc.wav";

fn reference() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("LOOPSLCR_REFERENCE") {
        let p = PathBuf::from(p);
        return p.is_file().then_some(p);
    }
    // `CARGO_MANIFEST_DIR` is the crate, not the workspace.
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..").join(NAME);
    root.is_file().then_some(root)
}

#[test]
fn the_reference_file_cuts_and_writes_back() {
    let Some(path) = reference() else {
        eprintln!("{NAME} not found — skipping reference cut");
        return;
    };
    let bytes = std::fs::read(&path).expect("reference unreadable");

    let source = Wav::parse(&bytes).expect("reference did not parse");
    assert_eq!(source.sample_rate(), 44_100);
    assert_eq!(source.channel_count(), 2);
    assert_eq!(source.format().bits_per_sample, 24);
    // The frame count the short RIFF size field used to cost eight frames of.
    assert_eq!(source.frames(), 1_875_540);
    assert!(!source.has_partial_frame());

    let grid = Grid::new(Tempo::bpm(103).unwrap(), TimeSignature::FOUR_FOUR, 44_100);
    let region = grid.region(8, 8, Align::Loop);
    assert_eq!((region.start, region.end), (822_058, 1_644_116));

    // `Region` counts in `u64` so the grid never has to care how wide a
    // `usize` is; the sample domain is where that gets narrowed. `ops::cut`
    // will own this conversion.
    let cut = source
        .decode_range(region.start as usize, region.end as usize)
        .unwrap();
    assert_eq!(cut.frames(), 822_058);
    // A settled bar of a rendered loop is not silence; a wrong offset would
    // land in the tail.
    assert!(cut.peak() > 0.1, "cut region is near-silent: peak {}", cut.peak());

    // 32 beats of 4/4 at 103 BPM, looping the whole file.
    let spec = WriteSpec::new(BitDepth::Int24).with_metadata(
        Metadata::for_loop(103.0, 32, 4, 4, cut.frames() as u32)
            .with_comment("LOOP_SLCR: 103 BPM, 8 bars, 4/4"),
    );
    let out = write(&cut, &spec).expect("write failed");

    let round = Wav::parse(&out).expect("our own output did not parse");
    assert_eq!(round.frames(), 822_058, "frames lost on write");
    assert!(!round.has_partial_frame());
    assert_eq!(round.format().bits_per_sample, 24);
    assert_eq!(round.sample_rate(), 44_100);
    assert_eq!(round.tags().declared_tempo(), Some(103.0));
    assert_eq!(round.tags().smpl.as_ref().unwrap().loops[0].frame_count(822_058), 822_058);

    // 24-bit in, 24-bit out: every sample identical, not merely close.
    let back = round.decode().unwrap();
    for c in 0..2 {
        assert_eq!(back.channel(c), cut.channel(c), "channel {c} changed");
    }

    // Every byte accounted for: envelope, four chunks, no slack.
    let comment_len = "LOOP_SLCR: 103 BPM, 8 bars, 4/4".len() + 1; // NUL-terminated
    let expected = 12                          // RIFF + size + WAVE
        + (8 + 16)                             // fmt
        + (8 + 822_058 * 6)                    // data, 24-bit stereo
        + (8 + 24)                             // acid
        + (8 + 36 + 24)                        // smpl, one loop
        + (8 + 4 + 8 + comment_len); // LIST/INFO/ICMT
    assert_eq!(out.len(), expected, "unexpected file size");
    assert_eq!(comment_len % 2, 0, "comment would need a pad byte");

    // Until the CLI can cut, this is how the result gets somewhere it can be
    // listened to — the only check that matters in the end.
    if let Some(dest) = std::env::var_os("LOOPSLCR_CUT_OUT") {
        std::fs::write(&dest, &out).expect("could not write the cut");
        eprintln!("wrote {} bytes to {}", out.len(), PathBuf::from(dest).display());
    }
}
