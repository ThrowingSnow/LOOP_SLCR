//! Varispeed by band-limited resampling.
//!
//! Pure resampling: pitch and tempo move together, tape style. That is the
//! whole reason a perfect loop survives it — a time-preserving pitch shift
//! would smear artefacts across exactly the seam this tool exists to clean.
//!
//! # Two things this does that a general resampler does not
//!
//! **The length is an input, not an output.** The caller passes the target frame
//! count, derived from the exact bar mathematics at the new tempo
//! ([`Grid::resampled_length`](crate::timing::Grid::resampled_length)). The
//! effective ratio is then defined by the two integer lengths. Asking for a
//! nominal ratio and accepting whatever length falls out would round a second
//! time, and a loop one sample off drifts.
//!
//! **The kernel reads circularly.** A loop *is* periodic, so a sample before
//! the start is the sample near the end. Treating the outside as zero — what
//! every general-purpose resampler does, because it cannot know — would drain
//! energy from the first and last few dozen samples and put a dip exactly at the
//! seam. This is why the resampler is hand-rolled rather than delegated.
//!
//! # Anti-aliasing
//!
//! Pitching up decimates, and decimation without a lowpass folds everything
//! above the new Nyquist back down as aliasing. The kernel cutoff therefore
//! scales as `0.5 / max(1, step)`, and the kernel widens by the same factor so
//! it keeps the same number of zero crossings — a narrower filter needs a longer
//! impulse response, and skipping that is how resamplers end up with audible
//! aliasing.

// Filtering is the sample domain.
#![allow(clippy::float_arithmetic)]

use std::f64::consts::PI;

use crate::buffer::AudioBuffer;
use crate::error::{Error, Result};

/// What lies outside the buffer.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
pub enum Edge {
    /// The buffer is one period of a loop: index past the end wraps to the
    /// start. Correct for anything this tool produces, and the default.
    #[default]
    Wrap,
    /// Silence outside. For material that is not a loop — a one-shot, or a
    /// region still being auditioned.
    Zero,
}

/// Anything that can change a buffer's length by resampling.
///
/// A trait because a second implementation is coming: live preview needs a
/// per-sample rate change and no allocation, which is a different problem from
/// offline quality.
pub trait Resampler {
    /// Resamples `input` to exactly `target_frames` frames.
    fn resample(&self, input: &AudioBuffer, target_frames: usize) -> Result<AudioBuffer>;
}

/// Windowed-sinc interpolation with a Blackman–Harris window.
#[derive(Copy, Clone, Debug)]
pub struct SincResampler {
    /// Sinc zero crossings each side of centre, at the kernel's cutoff.
    ///
    /// The quality knob. 32 gives a stopband around −90 dB, which is below the
    /// noise floor of 16-bit output; the cost is linear in this number.
    pub zero_crossings: usize,
    pub edge: Edge,
}

impl Default for SincResampler {
    fn default() -> Self {
        SincResampler {
            zero_crossings: 32,
            edge: Edge::Wrap,
        }
    }
}

impl SincResampler {
    /// Fewer taps, for previewing where latency beats a −90 dB stopband.
    pub fn fast() -> Self {
        SincResampler {
            zero_crossings: 8,
            ..Self::default()
        }
    }

    pub fn with_edge(self, edge: Edge) -> Self {
        SincResampler { edge, ..self }
    }

    /// Kernel geometry for a given rate: cutoff, half-width, and integer reach.
    ///
    /// `step` is input samples advanced per output sample. Above 1 the output is
    /// sparser than the input — pitching up — and needs the lowpass.
    fn kernel(&self, step: f64) -> (f64, f64, isize) {
        let cutoff = 0.5 / step.max(1.0);
        // Half-width in input samples. The kernel stretches as the cutoff
        // narrows so the zero-crossing count stays put.
        let half = self.zero_crossings as f64 / (2.0 * cutoff);
        (cutoff, half, half.ceil() as isize)
    }

    /// One band-limited read of `source` at fractional position `centre`.
    ///
    /// Exposed because two callers drive the position themselves rather than
    /// stepping it uniformly: the tape wobble, whose read position wanders by a
    /// fraction of a sample, and eventually the live preview. `step` describes
    /// the *local* rate, and only sets how much anti-aliasing the read needs —
    /// pass 1.0 when the position merely wobbles around unity.
    pub fn read(&self, source: &[f64], centre: f64, step: f64) -> f64 {
        let (cutoff, half, reach) = self.kernel(step);
        self.read_with(source, centre, cutoff, half, reach)
    }

    /// The inner loop, with the kernel geometry already computed.
    ///
    /// Split out so a run over thousands of samples at one rate pays for
    /// [`Self::kernel`] once instead of once per sample.
    fn read_with(&self, source: &[f64], centre: f64, cutoff: f64, half: f64, reach: isize) -> f64 {
        let first = centre.floor() as isize - reach;

        let mut sum = 0.0;
        let mut weight = 0.0;
        for i in first..=(first + 2 * reach) {
            let offset = centre - i as f64;
            if offset.abs() > half {
                continue;
            }
            let tap = 2.0 * cutoff * sinc(2.0 * cutoff * offset) * blackman_harris(offset / half);
            // Accumulate the tap weight even where the sample is taken as zero,
            // or the normalisation below would boost the edges.
            weight += tap;
            if let Some(x) = pick(source, i, self.edge) {
                sum += tap * x;
            }
        }

        // Normalise to unity DC gain. Windowing and a fractional centre leave
        // the tap sum a little off 1, which unaddressed shows up as a faint
        // ripple across the whole file.
        if weight.abs() > 1e-12 {
            sum / weight
        } else {
            0.0
        }
    }
}

impl Resampler for SincResampler {
    fn resample(&self, input: &AudioBuffer, target_frames: usize) -> Result<AudioBuffer> {
        let source_frames = input.frames();
        if source_frames == 0 {
            return Err(Error::EmptyResampleInput);
        }
        // Unity is the identity, bit for bit. Filtering a buffer only to hand
        // back the same length would cost quality for nothing, and it would
        // break the promise that the clean path reproduces byte-identically.
        if target_frames == source_frames {
            return Ok(input.clone());
        }
        if target_frames == 0 {
            return Err(Error::EmptyResampleOutput);
        }

        let step = source_frames as f64 / target_frames as f64;
        let (cutoff, half, reach) = self.kernel(step);

        let mut out = vec![vec![0.0f64; target_frames]; input.channel_count()];
        for (channel, target) in out.iter_mut().enumerate() {
            let source = input.channel(channel);
            for (j, sample) in target.iter_mut().enumerate() {
                *sample = self.read_with(source, j as f64 * step, cutoff, half, reach);
            }
        }

        Ok(AudioBuffer::new(out, input.sample_rate()))
    }
}

/// Reads `source[i]`, resolving indices outside the buffer per `edge`.
fn pick(source: &[f64], i: isize, edge: Edge) -> Option<f64> {
    let n = source.len() as isize;
    match edge {
        Edge::Wrap => Some(source[i.rem_euclid(n) as usize]),
        Edge::Zero => (0..n).contains(&i).then(|| source[i as usize]),
    }
}

fn sinc(x: f64) -> f64 {
    if x == 0.0 {
        1.0
    } else {
        let pi_x = PI * x;
        pi_x.sin() / pi_x
    }
}

/// Blackman–Harris, 4-term, over `x` in `[-1, 1]`.
///
/// A cosine sum rather than a Kaiser window: the sidelobes are about −92 dB,
/// which is enough to sit under 16-bit noise, and it needs no Bessel function.
fn blackman_harris(x: f64) -> f64 {
    let t = (x + 1.0) / 2.0;
    0.35875 - 0.48829 * (2.0 * PI * t).cos() + 0.14128 * (4.0 * PI * t).cos()
        - 0.01168 * (6.0 * PI * t).cos()
}

/// Resamples to `target_frames` at default quality, treating the input as a loop.
pub fn resample(input: &AudioBuffer, target_frames: usize) -> Result<AudioBuffer> {
    SincResampler::default().resample(input, target_frames)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A sine of `cycles` per buffer — exactly periodic, so the buffer really is
    /// one loop period.
    fn sine(frames: usize, cycles: usize, amplitude: f64) -> AudioBuffer {
        phased_sine(frames, cycles, amplitude, 0.0)
    }

    /// The same with a phase offset. `PI / 2` gives a cosine, whose value at both
    /// ends is full scale — the worst case for edge handling, where a plain sine
    /// is the best case because its seam sits on a zero crossing.
    fn phased_sine(frames: usize, cycles: usize, amplitude: f64, phase: f64) -> AudioBuffer {
        let data = (0..frames)
            .map(|i| {
                amplitude * (2.0 * PI * cycles as f64 * i as f64 / frames as f64 + phase).sin()
            })
            .collect();
        AudioBuffer::new(vec![data], 44_100)
    }

    fn rms(samples: &[f64]) -> f64 {
        (samples.iter().map(|s| s * s).sum::<f64>() / samples.len() as f64).sqrt()
    }

    /// Energy at `cycles` per buffer, by direct correlation — enough of a
    /// spectrum analyser to tell a tone from its alias.
    fn magnitude_at(samples: &[f64], cycles: f64) -> f64 {
        let n = samples.len() as f64;
        let (mut re, mut im) = (0.0, 0.0);
        for (i, &s) in samples.iter().enumerate() {
            let phase = 2.0 * PI * cycles * i as f64 / n;
            re += s * phase.cos();
            im += s * phase.sin();
        }
        2.0 * (re * re + im * im).sqrt() / n
    }

    #[test]
    fn unity_length_is_the_identity() {
        let source = sine(1000, 10, 0.5);
        let out = resample(&source, 1000).unwrap();
        assert_eq!(out, source, "unity resampling altered the buffer");
    }

    #[test]
    fn the_output_has_exactly_the_length_asked_for() {
        let source = sine(1000, 10, 0.5);
        for target in [1, 2, 333, 999, 1001, 1500, 4000] {
            let out = resample(&source, target).unwrap();
            assert_eq!(out.frames(), target, "asked for {target}");
            assert_eq!(out.sample_rate(), 44_100);
            assert_eq!(out.channel_count(), 1);
        }
    }

    #[test]
    fn a_tone_keeps_its_pitch_relative_to_the_loop() {
        // Resampling a loop stretches time and frequency together, so a tone
        // that fitted 10 cycles into the loop still fits 10 cycles into it.
        let source = sine(2000, 10, 0.5);
        for target in [1000, 1500, 3000, 4000] {
            let out = resample(&source, target).unwrap();
            let ours = magnitude_at(out.channel(0), 10.0);
            assert!(
                (ours - 0.5).abs() < 0.01,
                "{target} frames: amplitude {ours} at 10 cycles"
            );
            // And nothing appreciable anywhere else.
            for other in [5.0, 9.0, 11.0, 20.0] {
                let stray = magnitude_at(out.channel(0), other);
                assert!(stray < 0.005, "{target} frames: {stray} at {other} cycles");
            }
        }
    }

    #[test]
    fn pitching_up_does_not_alias() {
        // A tone at 0.3 of the sample rate, sped up by 2×, would land at 0.6 —
        // past Nyquist — and fold back to 0.4 if the lowpass were missing.
        let frames = 4096;
        let cycles = (0.3 * frames as f64) as usize; // 1228
        let source = sine(frames, cycles, 0.5);

        let out = resample(&source, frames / 2).unwrap();
        // In the halved buffer the alias would sit at (frames - cycles) / 2.
        let alias_cycles = (frames - cycles) as f64 / 2.0;
        let alias = magnitude_at(out.channel(0), alias_cycles);
        assert!(alias < 1e-4, "alias at {alias_cycles} cycles: {alias}");

        // The tone itself is above the new Nyquist and must be filtered away,
        // not folded down: the output should be near silent.
        assert!(rms(out.channel(0)) < 1e-3, "rms {}", rms(out.channel(0)));
    }

    #[test]
    fn a_tone_below_the_new_nyquist_survives_being_pitched_up() {
        // 0.1 of the rate doubled is 0.2 — still well inside band.
        let frames = 4096;
        let cycles = 410; // ≈ 0.1
        let out = resample(&sine(frames, cycles, 0.5), frames / 2).unwrap();
        let kept = magnitude_at(out.channel(0), cycles as f64);
        assert!((kept - 0.5).abs() < 0.01, "amplitude {kept}");
    }

    /// The reason for `Edge::Wrap`, measured at the seam.
    ///
    /// Against the analytic answer, not against a neighbouring window: the RMS
    /// of a 64-frame slice of a sine depends on where in its cycle the slice
    /// falls, so comparing slices would measure phase, not edge losses. A
    /// periodic sine resampled to any length is exactly a sine with the same
    /// cycle count, so the ideal output is known in closed form.
    ///
    /// Deliberately a cosine: its seam sits at full scale. A plain sine starts
    /// and ends at zero, so the samples an ignorant kernel reads as zero really
    /// are near zero and the whole effect nearly vanishes — the test would pass
    /// while measuring nothing.
    #[test]
    fn wrapping_keeps_the_edges_intact() {
        let (source_frames, target, cycles, amp) = (2000, 3000, 7, 0.5);
        let phase = PI / 2.0;
        let source = phased_sine(source_frames, cycles, amp, phase);
        let ideal = |j: usize| {
            amp * (2.0 * PI * cycles as f64 * j as f64 / target as f64 + phase).sin()
        };

        let worst = |b: &AudioBuffer, range: std::ops::Range<usize>| {
            range
                .map(|j| (b.channel(0)[j] - ideal(j)).abs())
                .fold(0.0f64, f64::max)
        };

        let wrapped = SincResampler::default().resample(&source, target).unwrap();
        let zeroed = SincResampler::default()
            .with_edge(Edge::Zero)
            .resample(&source, target)
            .unwrap();

        // Wrapping is right everywhere, edges included.
        for (label, range) in [
            ("head", 0..64),
            ("middle", target / 2..target / 2 + 64),
            ("tail", target - 64..target),
        ] {
            let error = worst(&wrapped, range);
            assert!(error < 1e-4, "wrapped {label}: off by {error}");
        }

        // Reading zeros is visibly wrong at both ends — the seam — while
        // agreeing in the middle, which is exactly the damage being avoided.
        assert!(worst(&zeroed, target / 2..target / 2 + 64) < 1e-4);
        assert!(
            worst(&zeroed, 0..64) > 0.01,
            "zero edge head is unexpectedly accurate: {}",
            worst(&zeroed, 0..64)
        );
        assert!(
            worst(&zeroed, target - 64..target) > 0.01,
            "zero edge tail is unexpectedly accurate: {}",
            worst(&zeroed, target - 64..target)
        );
    }

    #[test]
    fn dc_survives_at_unity_gain() {
        // The normalisation check: a constant must come back as the same
        // constant, at every length, or the whole file carries a ripple.
        let source = AudioBuffer::new(vec![vec![0.5; 1000]], 44_100);
        for target in [500, 777, 1333, 2000] {
            let out = resample(&source, target).unwrap();
            for (i, &s) in out.channel(0).iter().enumerate() {
                assert!(
                    (s - 0.5).abs() < 1e-9,
                    "{target} frames: sample {i} is {s}, not 0.5"
                );
            }
        }
    }

    #[test]
    fn silence_stays_silent() {
        let out = resample(&AudioBuffer::silence(2, 500, 44_100), 800).unwrap();
        assert_eq!(out.frames(), 800);
        assert!(out.channels().iter().all(|c| c.iter().all(|&s| s == 0.0)));
    }

    #[test]
    fn channels_are_resampled_independently() {
        let a = sine(1000, 5, 0.5).into_channels().pop().unwrap();
        let b = sine(1000, 13, 0.25).into_channels().pop().unwrap();
        let source = AudioBuffer::new(vec![a, b], 44_100);
        let out = resample(&source, 1500).unwrap();

        assert!((magnitude_at(out.channel(0), 5.0) - 0.5).abs() < 0.01);
        assert!((magnitude_at(out.channel(1), 13.0) - 0.25).abs() < 0.01);
        // No bleed between them.
        assert!(magnitude_at(out.channel(0), 13.0) < 0.005);
        assert!(magnitude_at(out.channel(1), 5.0) < 0.005);
    }

    #[test]
    fn empty_input_or_output_is_refused() {
        let source = sine(100, 3, 0.5);
        assert_eq!(resample(&source, 0).err(), Some(Error::EmptyResampleOutput));
        assert_eq!(
            resample(&AudioBuffer::silence(1, 0, 44_100), 100).err(),
            Some(Error::EmptyResampleInput)
        );
        // Zero to zero is still the identity, not an error.
        let empty = AudioBuffer::silence(1, 0, 44_100);
        assert_eq!(resample(&empty, 0).err(), Some(Error::EmptyResampleInput));
    }

    #[test]
    fn the_fast_preset_is_cheaper_and_still_correct() {
        let fast = SincResampler::fast();
        let out = fast.resample(&sine(1000, 8, 0.5), 1500).unwrap();
        assert_eq!(out.frames(), 1500);
        // Looser tolerance than the default preset, but the tone is still there.
        assert!((magnitude_at(out.channel(0), 8.0) - 0.5).abs() < 0.02);
        assert!(fast.zero_crossings < SincResampler::default().zero_crossings);
    }
}
