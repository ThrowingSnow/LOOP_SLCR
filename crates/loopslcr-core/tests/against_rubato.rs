//! Checks the hand-rolled resampler against `rubato`.
//!
//! Two separate questions, and it is worth keeping them apart:
//!
//! 1. **Is the kernel right?** In the interior of a long buffer, where edge
//!    handling cannot matter, our output and rubato's must agree closely. If
//!    they do not, one of us has a filter bug.
//! 2. **Is the circular edge better?** At the seam they must *dis*agree, and
//!    ours must be the one matching the analytic answer. That difference is the
//!    entire reason this is not delegated to rubato.
//!
//! rubato is a dev-dependency only. It cannot resample circularly, which is what
//! a loop needs.

#![allow(clippy::float_arithmetic)]

use std::f64::consts::PI;

use audioadapter_buffers::direct::SequentialSliceOfVecs;
use loopslcr_core::ops::resample::{resample, Edge, Resampler, SincResampler};
use loopslcr_core::AudioBuffer;
use rubato::{
    Async, FixedAsync, Resampler as _, SincInterpolationParameters, SincInterpolationType,
    WindowFunction,
};

/// A sine of `cycles` per buffer with a phase offset, so the buffer is exactly
/// one loop period. `PI / 2` puts the seam at full scale.
fn periodic_sine(frames: usize, cycles: usize, amplitude: f64, phase: f64) -> Vec<f64> {
    (0..frames)
        .map(|i| amplitude * (2.0 * PI * cycles as f64 * i as f64 / frames as f64 + phase).sin())
        .collect()
}

/// Resamples with rubato, one channel, returning at least `target` frames.
fn rubato_resample(input: &[f64], target: usize) -> Vec<f64> {
    let ratio = target as f64 / input.len() as f64;
    let params = SincInterpolationParameters {
        sinc_len: 256,
        f_cutoff: 0.95,
        oversampling_factor: 256,
        interpolation: SincInterpolationType::Cubic,
        window: WindowFunction::BlackmanHarris2,
    };
    let mut r = Async::<f64>::new_sinc(ratio, 1.1, &params, 1024, 1, FixedAsync::Input)
        .expect("rubato construction failed");

    let needed = r.process_all_needed_output_len(input.len());
    let input_vec = vec![input.to_vec()];
    let adapter = SequentialSliceOfVecs::new(&input_vec, 1, input.len()).expect("input adapter");
    let mut out_vec = vec![vec![0.0f64; needed]];
    let mut out = SequentialSliceOfVecs::new_mut(&mut out_vec, 1, needed).expect("output adapter");

    let (_, written) = r
        .process_all_into_buffer(&adapter, &mut out, input.len(), None)
        .expect("rubato processing failed");
    assert!(written > 0, "rubato produced nothing");

    let mut result = out_vec.pop().unwrap();
    result.truncate(written);
    result
}

/// Largest absolute difference over `range`.
fn worst_diff(a: &[f64], b: &[f64], range: std::ops::Range<usize>) -> f64 {
    range.map(|i| (a[i] - b[i]).abs()).fold(0.0, f64::max)
}

#[test]
fn the_interior_agrees_with_rubato() {
    // Long enough that 256-tap kernels are nowhere near the ends.
    let (frames, target, cycles) = (20_000, 30_000, 97);
    let source = periodic_sine(frames, cycles, 0.5, PI / 2.0);

    let ours = resample(&AudioBuffer::new(vec![source.clone()], 44_100), target).unwrap();
    let theirs = rubato_resample(&source, target);

    // rubato's output length is its own business; compare where both exist,
    // well away from either end.
    let overlap = ours.frames().min(theirs.len());
    assert!(overlap > 20_000, "not enough overlap: {overlap}");
    let range = 5_000..overlap - 5_000;

    // Compared at the best whole-sample alignment. rubato's own group delay is
    // trimmed by `process_all_into_buffer` to the nearest frame, so a residual
    // offset of a sample or so is expected and says nothing about the filters.
    // On a 97-cycle sine one sample is worth 0.0102 of amplitude, which would
    // otherwise swamp the difference actually being looked for.
    let (shift, error) = (-4isize..=4)
        .map(|shift| {
            let diff = range
                .clone()
                .filter(|&i| {
                    let j = i as isize + shift;
                    j >= 0 && (j as usize) < theirs.len()
                })
                .map(|i| (ours.channel(0)[i] - theirs[(i as isize + shift) as usize]).abs())
                .fold(0.0f64, f64::max);
            (shift, diff)
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .unwrap();

    assert!(
        error < 1e-3,
        "interior disagrees with rubato by {error} at its best alignment \
         (shift {shift}) — one of the two kernels is wrong"
    );
    assert!(shift.abs() <= 2, "alignment is off by {shift} samples");
}

/// Both resamplers against the closed-form answer. A periodic sine resampled to
/// any length is exactly a sine with the same cycle count, so "right" is known.
#[test]
fn ours_is_more_accurate_at_the_seam_and_no_worse_inside() {
    let (frames, target, cycles, amp) = (8_000, 12_000, 31, 0.5);
    let phase = PI / 2.0; // seam at full scale, where edge handling shows
    let source = periodic_sine(frames, cycles, amp, phase);
    let ideal: Vec<f64> = periodic_sine(target, cycles, amp, phase);

    let ours = resample(&AudioBuffer::new(vec![source.clone()], 44_100), target).unwrap();
    let theirs = rubato_resample(&source, target);
    assert!(theirs.len() >= target, "rubato returned {} frames", theirs.len());

    // Interior: both should be accurate, and ours must not be worse.
    let inside = 3_000..9_000;
    let our_inside = worst_diff(ours.channel(0), &ideal, inside.clone());
    let their_inside = worst_diff(&theirs, &ideal, inside);
    assert!(our_inside < 1e-4, "ours is off by {our_inside} inside");
    assert!(
        our_inside <= their_inside * 4.0,
        "ours ({our_inside}) is much worse than rubato ({their_inside}) inside"
    );

    // The seam: ours reads circularly and stays accurate; rubato cannot, and
    // loses level over the first kernel-width of samples.
    let head = 0..256;
    let our_head = worst_diff(ours.channel(0), &ideal, head.clone());
    let their_head = worst_diff(&theirs, &ideal, head);
    assert!(our_head < 1e-4, "our seam is off by {our_head}");
    assert!(
        their_head > our_head * 10.0,
        "rubato's seam error ({their_head}) is not the problem this assumed \
         (ours {our_head}) — re-examine whether the circular edge is needed"
    );
}

/// With the loop assumption dropped, ours should behave like rubato does — the
/// difference has to come from the edge policy, not from a different filter.
#[test]
fn our_zero_edge_reproduces_rubatos_seam_behaviour() {
    let (frames, target, cycles, amp) = (8_000, 12_000, 31, 0.5);
    let phase = PI / 2.0;
    let source = periodic_sine(frames, cycles, amp, phase);
    let ideal: Vec<f64> = periodic_sine(target, cycles, amp, phase);

    let zeroed = SincResampler::default()
        .with_edge(Edge::Zero)
        .resample(&AudioBuffer::new(vec![source.clone()], 44_100), target)
        .unwrap();
    let theirs = rubato_resample(&source, target);

    // Both lose the seam. The amount differs with kernel length, so this checks
    // the direction rather than the magnitude.
    let head = 0..64;
    assert!(
        worst_diff(zeroed.channel(0), &ideal, head.clone()) > 0.01,
        "zero edge unexpectedly kept the seam"
    );
    assert!(
        worst_diff(&theirs, &ideal, head) > 0.01,
        "rubato unexpectedly kept the seam"
    );
}
