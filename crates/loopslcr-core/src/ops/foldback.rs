//! Folding an FX tail back onto the head of the loop.
//!
//! The idea in one line: a loop that repeats forever is *circular*, so the
//! reverb that spills past the end of bar N is exactly the reverb that should
//! already be present at the start of bar 1. Summing the tail back in modulo
//! the loop length turns a linear render into the circular one:
//!
//! ```text
//! out[i % loopLen] += tail[i]
//! ```
//!
//! This is the same identity that says circular convolution is linear
//! convolution wrapped — which is why it is exact rather than a crossfade
//! approximation. A tail longer than the loop simply wraps more than once, and
//! each wrap is summed in turn.
//!
//! Never combined with path A. A warmup render already contains the settled
//! state in bars N+1..2N; folding its tail in as well would add the reverb
//! twice.

// Summation is the sample domain.
#![allow(clippy::float_arithmetic)]

use crate::buffer::AudioBuffer;
use crate::error::{Error, Result};

/// What a foldback did.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Foldback {
    pub loop_frames: usize,
    pub tail_frames: usize,
    /// How many times the tail wrapped around the loop. A tail shorter than
    /// the loop wraps once (partially); a tail 2.5 loops long wraps three
    /// times, the last one partially.
    pub wraps: usize,
    pub peak_before: f64,
    pub peak_after: f64,
}

impl Foldback {
    /// True when summing the tail in pushed the signal past full scale.
    ///
    /// Not an error — a float output holds it fine, and the fix is the user's
    /// choice between `--normalize` and re-rendering quieter. But it must be
    /// reported, because quantising to integer would clip it.
    pub fn overshoots(&self) -> bool {
        self.peak_after > 1.0
    }

    /// How much louder the fold made the loudest sample, as a ratio.
    pub fn gain(&self) -> f64 {
        if self.peak_before > 0.0 {
            self.peak_after / self.peak_before
        } else {
            1.0
        }
    }
}

/// Folds everything past `loop_frames` back onto the loop.
///
/// `source` is the loop *and* its tail: one contiguous buffer starting exactly
/// at the loop's first frame. Passing the loop and tail separately would invite
/// an off-by-one at the join, which is precisely where a click would appear.
///
/// Returns the loop alone, `loop_frames` long, with the tail summed in.
pub fn foldback(source: &AudioBuffer, loop_frames: usize) -> Result<(AudioBuffer, Foldback)> {
    if loop_frames == 0 {
        return Err(Error::EmptyLoop);
    }
    if source.frames() < loop_frames {
        // Zero-padding to make up the difference would be inventing audio, and
        // the resulting loop would be short — which drifts.
        return Err(Error::SourceShorterThanLoop {
            have: source.frames(),
            need: loop_frames,
        });
    }

    let tail_frames = source.frames() - loop_frames;
    let mut out = source.slice(0, loop_frames);
    let peak_before = out.peak();

    for (channel, target) in out.channels_mut().iter_mut().enumerate() {
        let tail = &source.channel(channel)[loop_frames..];
        for (i, &s) in tail.iter().enumerate() {
            // The modulo is the whole trick. It is taken on the tail's own
            // index, so a tail of any length lands correctly without the
            // caller having to chunk it.
            target[i % loop_frames] += s;
        }
    }

    let peak_after = out.peak();
    Ok((
        out,
        Foldback {
            loop_frames,
            tail_frames,
            wraps: tail_frames.div_ceil(loop_frames),
            peak_before,
            peak_after,
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A buffer from explicit values, mono unless a second channel is given.
    fn buf(channels: Vec<Vec<f64>>) -> AudioBuffer {
        AudioBuffer::new(channels, 44_100)
    }

    #[test]
    fn a_tail_shorter_than_the_loop_lands_on_the_head() {
        // Loop of 4, tail of 2.
        let source = buf(vec![vec![1.0, 2.0, 3.0, 4.0, 10.0, 20.0]]);
        let (out, report) = foldback(&source, 4).unwrap();
        assert_eq!(out.channel(0), &[11.0, 22.0, 3.0, 4.0]);
        assert_eq!(report.tail_frames, 2);
        assert_eq!(report.wraps, 1);
    }

    #[test]
    fn no_tail_is_the_identity() {
        // Path A hands us exactly the loop; folding must change nothing.
        let source = buf(vec![vec![1.0, -2.0, 3.0, -4.0]]);
        let (out, report) = foldback(&source, 4).unwrap();
        assert_eq!(out, source);
        assert_eq!(report.tail_frames, 0);
        assert_eq!(report.wraps, 0);
        assert_eq!(report.gain(), 1.0);
    }

    #[test]
    fn a_tail_longer_than_the_loop_wraps_more_than_once() {
        // Loop of 3, tail of 7 — two full wraps and a partial third.
        let source = buf(vec![vec![
            0.0, 0.0, 0.0, // loop
            1.0, 2.0, 3.0, // wrap 1
            10.0, 20.0, 30.0, // wrap 2
            100.0, // wrap 3, partial
        ]]);
        let (out, report) = foldback(&source, 3).unwrap();
        assert_eq!(out.channel(0), &[111.0, 22.0, 33.0]);
        assert_eq!(report.tail_frames, 7);
        assert_eq!(report.wraps, 3);
    }

    #[test]
    fn a_tail_an_exact_multiple_of_the_loop_wraps_evenly() {
        let source = buf(vec![vec![0.0, 0.0, 1.0, 2.0, 3.0, 4.0]]);
        let (out, report) = foldback(&source, 2).unwrap();
        assert_eq!(out.channel(0), &[4.0, 6.0]);
        assert_eq!(report.wraps, 2);
    }

    #[test]
    fn channels_are_folded_independently() {
        let source = buf(vec![
            vec![1.0, 0.0, 5.0, 0.0],
            vec![0.0, 1.0, 0.0, 7.0],
        ]);
        let (out, _) = foldback(&source, 2).unwrap();
        assert_eq!(out.channel(0), &[6.0, 0.0]);
        assert_eq!(out.channel(1), &[0.0, 8.0]);
    }

    #[test]
    fn folding_reports_overshoot_instead_of_clipping_it() {
        // Two loud halves summing past unity: the report says so, the samples
        // are left intact for the caller to normalize or refuse.
        let source = buf(vec![vec![0.8, 0.8, 0.5, 0.5]]);
        let (out, report) = foldback(&source, 2).unwrap();
        assert_eq!(out.channel(0), &[1.3, 1.3]);
        assert!(report.overshoots());
        assert_eq!(report.peak_before, 0.8);
        assert_eq!(report.peak_after, 1.3);
        assert!((report.gain() - 1.625).abs() < 1e-12);
    }

    #[test]
    fn a_quiet_fold_does_not_report_overshoot() {
        let source = buf(vec![vec![0.2, 0.2, 0.1]]);
        let (_, report) = foldback(&source, 2).unwrap();
        assert!(!report.overshoots());
    }

    #[test]
    fn a_source_shorter_than_the_loop_is_refused() {
        let source = buf(vec![vec![1.0, 2.0]]);
        assert_eq!(
            foldback(&source, 5).err(),
            Some(Error::SourceShorterThanLoop { have: 2, need: 5 })
        );
        assert_eq!(foldback(&source, 0).err(), Some(Error::EmptyLoop));
    }

    /// The property that justifies the whole approach: after folding, the loop
    /// played twice equals the linear render summed with itself one loop later.
    /// If that held only approximately, foldback would be a crossfade.
    #[test]
    fn the_fold_is_exact_against_a_hand_summed_reference() {
        let loop_len = 7;
        let source: Vec<f64> = (0..20).map(|i| (i * 3 % 11) as f64).collect();
        let (out, _) = foldback(&buf(vec![source.clone()]), loop_len).unwrap();

        // The same sum, computed the obvious slow way.
        let mut expected = vec![0.0f64; loop_len];
        for (i, &s) in source.iter().enumerate() {
            expected[i % loop_len] += s;
        }
        assert_eq!(out.channel(0), expected.as_slice());
    }

    /// Silence folded onto silence stays silent — no DC offset creeps in.
    #[test]
    fn silence_stays_silent() {
        let (out, report) = foldback(&AudioBuffer::silence(2, 100, 44_100), 40).unwrap();
        assert!(out.channels().iter().all(|c| c.iter().all(|&s| s == 0.0)));
        assert_eq!(report.gain(), 1.0);
    }
}
