//! TPDF dither, applied immediately before quantisation.
//!
//! Rounding to a lower bit depth without dither correlates the error with the
//! signal, which is heard as distortion rather than as noise — worst on quiet
//! decays, which is exactly what the tail of a loop is. Triangular
//! probability-density noise of ±1 LSB decorrelates it: the noise floor rises
//! slightly and becomes flat and signal-independent.
//!
//! **The seed is fixed, deliberately.** The architecture requires that the same
//! input and parameters produce a byte-identical file forever, and a randomly
//! seeded dither would break that on every run. So this is a deterministic
//! sequence that merely looks like noise — which is all dither has ever needed
//! to be. `--dither-seed` exists for anyone who wants a different draw.
//!
//! Only for integer output. Float has no quantisation step to dither against,
//! and adding noise to it would be pure damage.
//!
//! # Noise shaping
//!
//! Flat dither is honest but not optimal: the ear is far less sensitive above
//! 10 kHz than around 3, so noise spread evenly across the band wastes most of
//! itself where it is heard. [`Dither::Shaped`] feeds the quantisation error back
//! through a filter so the added noise comes out highpass — nulled at DC, rising
//! towards Nyquist.
//!
//! The transfer function is `(1 - z⁻¹)²`, which needs a feedback of
//! `2e[n-1] - e[n-2]`. Second order rather than higher because the trade gets
//! worse fast: the **total** noise power rises by about 7.8 dB, and it is only
//! worth paying because so much of it lands above 15 kHz. Below 5 kHz the noise
//! drops by roughly 10 dB, and that is the band the number describes.
//!
//! Shaping requires the error, so [`Dither::Shaped`] **quantises as well as
//! dithers** — there is no way to feed back an error that has not been made yet.
//! The samples it leaves behind sit exactly on the target grid, so the writer's
//! own rounding is then a no-op and the encoder needs no special case. That is
//! the seam that keeps this from leaking into `wav::write`.

// Noise generation is the sample domain.
#![allow(clippy::float_arithmetic)]

use crate::buffer::AudioBuffer;
use crate::wav::BitDepth;

/// Whether to dither, and how.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
pub enum Dither {
    /// Never. Right for float output, and for integer output that is already on
    /// the target quantisation grid.
    None,
    /// Triangular PDF, ±1 LSB. The default when bit depth is being reduced.
    #[default]
    Tpdf,
    /// TPDF noise plus second-order error feedback, so the added noise is
    /// highpass. Quantises the buffer as a side effect — see the module docs.
    Shaped,
}

impl Dither {
    /// Whether this mode leaves the samples on the target quantisation grid.
    pub fn quantises(self) -> bool {
        matches!(self, Dither::Shaped)
    }
}

/// The error-feedback coefficients for `(1 - z⁻¹)²`.
///
/// `v[n] = x[n] - (2·e[n-1] - e[n-2])`, which puts a double zero at DC in the
/// noise transfer function. Derived rather than tabulated: `1 - H(z)` is the
/// shaping, so `H(z) = 2z⁻¹ - z⁻²` is the only filter that gives it.
const SHAPER: [f64; 2] = [2.0, -1.0];

/// How far the fed-back error may travel, in LSB.
///
/// Without a limit, a passage sitting at full scale clips the quantiser, the
/// clipping error enters the feedback, and the shaper rings on it — a burst of
/// noise where the music is loudest. Two LSB is far above anything the shaper
/// produces in normal use, so this only ever engages on a clipped signal.
const ERROR_LIMIT: f64 = 2.0;

/// The default dither seed: the eight bytes of `loopslcr`.
///
/// Arbitrary, as any seed is, and fixed so that runs reproduce each other.
pub const DEFAULT_SEED: u64 = u64::from_be_bytes(*b"loopslcr");

/// A small deterministic generator: xorshift64*.
///
/// Not cryptographic and not trying to be. What dither needs is a flat spectrum
/// and reproducibility, both of which this has, in nine lines and no dependency.
struct Xorshift64Star(u64);

impl Xorshift64Star {
    fn new(seed: u64) -> Self {
        // Zero is the one state xorshift cannot leave.
        Xorshift64Star(if seed == 0 { 0x9E37_79B9_7F4A_7C15 } else { seed })
    }

    fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// Uniform in `[0, 1)`, from the top 53 bits — the ones an `f64` can hold.
    fn next_f64(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }
}

/// What [`apply`] did.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Applied {
    /// One step of the target grid.
    pub lsb: f64,
    /// True when the samples now sit exactly on that grid, so the writer's
    /// rounding will change nothing.
    pub quantised: bool,
}

/// Dithers `buffer` for output at `depth`.
///
/// Returns what was done, or `None` when nothing was. Applying this to a float
/// output, or with [`Dither::None`], is a no-op rather than an error: the caller
/// decides policy, this decides nothing.
pub fn apply(buffer: &mut AudioBuffer, depth: BitDepth, mode: Dither, seed: u64) -> Option<Applied> {
    if mode == Dither::None || depth.is_float() {
        return None;
    }
    // One LSB of the target grid, matching how the writer scales: full-scale
    // negative is exactly -1.0, so the step is 2^-(bits-1).
    let scale = (1i64 << (depth.bits() - 1)) as f64;
    let lsb = 1.0 / scale;
    // The writer's own limits, so a sample clamped here is a sample the writer
    // would have clamped identically.
    let ceiling = ((1i64 << (depth.bits() - 1)) - 1) as f64;
    let floor = -(1i64 << (depth.bits() - 1)) as f64;

    let mut rng = Xorshift64Star::new(seed);
    for channel in buffer.channels_mut() {
        // Per channel, so the shaper never carries one channel's error into the
        // next and the noise stays uncorrelated across the image.
        let mut history = [0.0f64; SHAPER.len()];
        for s in channel.iter_mut() {
            // The difference of two independent uniforms is triangular, peaking
            // at zero and vanishing at ±1 LSB. One uniform would be flat-topped
            // and leave the error still partly correlated with the signal.
            let noise = (rng.next_f64() - rng.next_f64()) * lsb;

            if mode == Dither::Tpdf {
                *s += noise;
                continue;
            }

            let feedback: f64 = SHAPER
                .iter()
                .zip(history)
                .map(|(h, e)| h * e)
                .sum();
            let wanted = *s - feedback;
            let quantised = (wanted + noise) * scale;
            let quantised = quantised.round().clamp(floor, ceiling) / scale;

            // The error carried forward is everything added at this sample: the
            // dither and the rounding together, which is what makes the *total*
            // added error highpass rather than only the rounding part.
            let error = (quantised - wanted).clamp(-ERROR_LIMIT * lsb, ERROR_LIMIT * lsb);
            history = [error, history[0]];
            *s = quantised;
        }
    }
    Some(Applied {
        lsb,
        quantised: mode.quantises(),
    })
}

/// Whether dither is called for when going from `source` bits to `target`.
///
/// True only when the depth actually drops. Equal or rising depth needs none:
/// the samples already sit on a finer grid than the output, so rounding is
/// either exact or already below the noise floor of the source.
///
/// This is a *depth* question only. Whether the signal was altered — a fade, a
/// foldback, a resample all move samples off the source grid — is a separate one
/// the caller answers, since dithering a whole file for the sake of 23 faded
/// frames at each end is not a trade worth making.
pub fn is_called_for(source_bits: u16, target: BitDepth) -> bool {
    !target.is_float() && target.bits() < source_bits
}

#[cfg(test)]
mod tests {
    use super::*;

    fn buf(frames: usize) -> AudioBuffer {
        // A quiet constant: without dither this quantises to a single value and
        // the error is a pure DC offset, which is the failure dither prevents.
        AudioBuffer::new(vec![vec![0.000_01; frames], vec![-0.000_01; frames]], 44_100)
    }

    #[test]
    fn noise_stays_within_one_lsb() {
        for depth in [BitDepth::Int16, BitDepth::Int24, BitDepth::Int32] {
            let source = buf(4096);
            let mut b = source.clone();
            let lsb = apply(&mut b, depth, Dither::Tpdf, DEFAULT_SEED).unwrap().lsb;
            assert_eq!(lsb, 1.0 / (1i64 << (depth.bits() - 1)) as f64);

            for c in 0..2 {
                for f in 0..4096 {
                    let delta = b.channel(c)[f] - source.channel(c)[f];
                    assert!(delta.abs() <= lsb, "{depth:?}: {delta} exceeds one LSB");
                }
            }
        }
    }

    #[test]
    fn the_same_seed_gives_the_same_file() {
        // The reproducibility invariant: identical input and parameters must
        // produce identical output, forever.
        let mut a = buf(1000);
        let mut b = buf(1000);
        apply(&mut a, BitDepth::Int16, Dither::Tpdf, DEFAULT_SEED);
        apply(&mut b, BitDepth::Int16, Dither::Tpdf, DEFAULT_SEED);
        assert_eq!(a, b);

        // And a different seed genuinely gives a different draw.
        let mut c = buf(1000);
        apply(&mut c, BitDepth::Int16, Dither::Tpdf, DEFAULT_SEED ^ 1);
        assert_ne!(a, c);
    }

    #[test]
    fn the_noise_is_centred_and_triangular() {
        let mut b = AudioBuffer::silence(1, 200_000, 44_100);
        let lsb = apply(&mut b, BitDepth::Int16, Dither::Tpdf, DEFAULT_SEED).unwrap().lsb;
        let n = b.frames() as f64;
        let samples = b.channel(0);

        // Zero mean, or dither would add a DC offset of its own.
        let mean = samples.iter().sum::<f64>() / n;
        assert!(mean.abs() < lsb * 0.01, "mean {mean} is not centred");

        // Variance of a triangular distribution on ±L is L²/6.
        let variance = samples.iter().map(|s| s * s).sum::<f64>() / n;
        let expected = lsb * lsb / 6.0;
        assert!(
            (variance / expected - 1.0).abs() < 0.02,
            "variance {variance} is not triangular (expected {expected})"
        );

        // Triangular, not uniform: values near zero must be commoner than
        // values near the limit. A uniform draw would make these equal.
        let near_zero = samples.iter().filter(|s| s.abs() < lsb * 0.25).count();
        let near_edge = samples.iter().filter(|s| s.abs() > lsb * 0.75).count();
        assert!(
            near_zero > near_edge * 3,
            "not triangular: {near_zero} near zero vs {near_edge} near the edge"
        );
    }

    #[test]
    fn channels_get_independent_noise() {
        // Correlated dither across channels would place its noise dead centre in
        // the stereo image instead of spreading it.
        let mut b = AudioBuffer::silence(2, 10_000, 44_100);
        apply(&mut b, BitDepth::Int16, Dither::Tpdf, DEFAULT_SEED);
        let (l, r) = (b.channel(0), b.channel(1));
        assert!(l.iter().zip(r).any(|(a, b)| a != b), "channels are identical");

        let dot: f64 = l.iter().zip(r).map(|(a, b)| a * b).sum();
        let energy: f64 = l.iter().map(|s| s * s).sum();
        assert!(
            (dot / energy).abs() < 0.05,
            "channels correlate at {}",
            dot / energy
        );
    }

    #[test]
    fn float_output_and_dither_none_do_nothing() {
        let source = buf(100);
        for (depth, mode) in [
            (BitDepth::Float32, Dither::Tpdf),
            (BitDepth::Int16, Dither::None),
        ] {
            let mut b = source.clone();
            assert_eq!(apply(&mut b, depth, mode, DEFAULT_SEED), None);
            assert_eq!(b, source, "{depth:?} {mode:?} altered the buffer");
        }
    }

    #[test]
    fn dither_is_called_for_only_when_the_depth_drops() {
        assert!(is_called_for(24, BitDepth::Int16));
        assert!(is_called_for(32, BitDepth::Int24));
        // Equal or rising depth: rounding is exact, so noise would be pure loss.
        assert!(!is_called_for(24, BitDepth::Int24));
        assert!(!is_called_for(16, BitDepth::Int24));
        assert!(!is_called_for(16, BitDepth::Int32));
        // Float has no grid to dither against.
        assert!(!is_called_for(24, BitDepth::Float32));
    }

    /// Noise power in a frequency band, by direct DFT over the bins it covers.
    ///
    /// A real measurement rather than a proxy: the whole claim of noise shaping
    /// is spectral, so a test that only counted amplitudes would check nothing
    /// about it.
    fn band_power(samples: &[f64], rate: u32, low_hz: f64, high_hz: f64) -> f64 {
        use std::f64::consts::PI;
        let n = samples.len();
        let bin_hz = rate as f64 / n as f64;
        let first = (low_hz / bin_hz).ceil() as usize;
        let last = ((high_hz / bin_hz).floor() as usize).min(n / 2);

        let mut power = 0.0;
        for k in first..=last {
            let (mut re, mut im) = (0.0, 0.0);
            for (i, &x) in samples.iter().enumerate() {
                let phase = 2.0 * PI * k as f64 * i as f64 / n as f64;
                re += x * phase.cos();
                im += x * phase.sin();
            }
            power += (re * re + im * im) / (n * n) as f64;
        }
        power
    }

    /// The noise each mode adds to silence, at 16 bit.
    fn noise(mode: Dither, frames: usize) -> (Vec<f64>, f64) {
        let mut b = AudioBuffer::silence(1, frames, 44_100);
        let applied = apply(&mut b, BitDepth::Int16, mode, DEFAULT_SEED).unwrap();
        (b.into_channels().pop().unwrap(), applied.lsb)
    }

    #[test]
    fn shaping_moves_the_noise_out_of_the_band_that_is_heard() {
        // The claim, measured. 4096 frames at 44.1 kHz gives 10.8 Hz bins, which
        // is fine enough to separate the two bands cleanly.
        let frames = 4096;
        let (flat, _) = noise(Dither::Tpdf, frames);
        let (shaped, _) = noise(Dither::Shaped, frames);

        let low_flat = band_power(&flat, 44_100, 20.0, 5_000.0);
        let low_shaped = band_power(&shaped, 44_100, 20.0, 5_000.0);
        let high_flat = band_power(&flat, 44_100, 15_000.0, 22_000.0);
        let high_shaped = band_power(&shaped, 44_100, 15_000.0, 22_000.0);

        let db = |a: f64, b: f64| 10.0 * (a / b).log10();

        // Below 5 kHz the shaped noise is markedly quieter — that is the point.
        assert!(
            db(low_shaped, low_flat) < -8.0,
            "low band only moved {:.1} dB",
            db(low_shaped, low_flat)
        );
        // Above 15 kHz it is markedly louder — that is the price.
        assert!(
            db(high_shaped, high_flat) > 6.0,
            "high band only moved {:.1} dB",
            db(high_shaped, high_flat)
        );
    }

    #[test]
    fn shaping_costs_total_noise_power_and_the_cost_is_the_predicted_one() {
        // `(1 - z⁻¹)²` has coefficients 1, -2, 1, so the noise power gain is
        // 1 + 4 + 1 = 6, which is 7.78 dB. Not a tuned number — it falls out of
        // the filter, and if the implementation drifted from the filter this is
        // the test that would say so.
        //
        // The comparison has to be like for like. `Dither::Tpdf` leaves the
        // rounding to the writer, so its buffer holds the dither alone, while a
        // shaped buffer holds dither *and* rounding error. Comparing those
        // directly measures 9.54 dB — the extra 1.76 dB being the L²/12 of the
        // rounding the flat side had not done yet, not anything about shaping.
        // So the flat side is quantised here first, as the writer would.
        let frames = 200_000;
        let (mut flat, lsb) = noise(Dither::Tpdf, frames);
        let scale = 1.0 / lsb;
        for s in flat.iter_mut() {
            *s = (*s * scale).round() / scale;
        }
        let (shaped, _) = noise(Dither::Shaped, frames);

        let power = |v: &[f64]| v.iter().map(|s| s * s).sum::<f64>() / v.len() as f64;
        let gain_db = 10.0 * (power(&shaped) / power(&flat)).log10();
        assert!(
            (gain_db - 7.78).abs() < 0.5,
            "total noise rose by {gain_db:.2} dB, not the predicted 7.78"
        );
    }

    #[test]
    fn shaping_leaves_the_samples_on_the_target_grid() {
        // The seam that keeps this out of `wav::write`: because the shaper has to
        // quantise in order to have an error to feed back, the writer's own
        // rounding must then be a no-op.
        let mut b = AudioBuffer::new(
            vec![(0..5000).map(|i| 0.3 * (i as f64 * 0.01).sin()).collect()],
            44_100,
        );
        let applied = apply(&mut b, BitDepth::Int16, Dither::Shaped, DEFAULT_SEED).unwrap();
        assert!(applied.quantised);

        let scale = (1i64 << 15) as f64;
        for (i, &s) in b.channel(0).iter().enumerate() {
            let raw = s * scale;
            assert!(
                (raw - raw.round()).abs() < 1e-9,
                "sample {i} is {s}, which is {raw} steps — not on the grid"
            );
        }
    }

    #[test]
    fn flat_dither_does_not_quantise() {
        // TPDF stays open loop, so the writer still does the rounding and the
        // two modes cannot be confused by a caller that checks.
        let mut b = AudioBuffer::silence(1, 100, 44_100);
        let applied = apply(&mut b, BitDepth::Int16, Dither::Tpdf, DEFAULT_SEED).unwrap();
        assert!(!applied.quantised);
        assert!(!Dither::Tpdf.quantises() && Dither::Shaped.quantises());
    }

    #[test]
    fn shaping_stays_bounded_on_a_signal_that_clips() {
        // Full scale plus dither overflows the quantiser, the clipping error
        // enters the feedback, and an unclamped shaper rings on it. The output
        // must stay inside the grid and stay finite.
        let mut b = AudioBuffer::new(vec![vec![1.0; 20_000]], 44_100);
        apply(&mut b, BitDepth::Int16, Dither::Shaped, DEFAULT_SEED);

        let ceiling = ((1i64 << 15) - 1) as f64 / (1i64 << 15) as f64;
        for (i, &s) in b.channel(0).iter().enumerate() {
            assert!(s.is_finite(), "sample {i} is {s}");
            assert!(s <= ceiling && s >= -1.0, "sample {i} left the grid at {s}");
        }
        // And it settles at the ceiling rather than oscillating away from it.
        let tail = &b.channel(0)[19_000..];
        assert!(
            tail.iter().all(|&s| s > ceiling - 4.0 / (1i64 << 15) as f64),
            "the shaper wandered off a clipped signal"
        );
    }

    #[test]
    fn shaped_dither_is_reproducible_too() {
        let mut a = buf(1000);
        let mut b = buf(1000);
        apply(&mut a, BitDepth::Int16, Dither::Shaped, DEFAULT_SEED);
        apply(&mut b, BitDepth::Int16, Dither::Shaped, DEFAULT_SEED);
        assert_eq!(a, b);
    }

    #[test]
    fn the_shaper_does_not_carry_state_between_channels() {
        // A channel's output must depend on that channel alone. Two buffers
        // whose second channels are identical and whose first channels are not:
        // the second channels have to come out byte-identical, because the seed
        // and the number of draws before them are the same in both. If the
        // shaper history ran on from channel one, they would not.
        let same: Vec<f64> = (0..3000).map(|i| 0.1 * (i as f64 * 0.02).sin()).collect();
        let quiet = vec![0.0; 3000];
        let loud: Vec<f64> = (0..3000).map(|i| 0.95 * (i as f64 * 0.3).sin()).collect();

        let mut a = AudioBuffer::new(vec![quiet, same.clone()], 44_100);
        let mut b = AudioBuffer::new(vec![loud, same], 44_100);
        apply(&mut a, BitDepth::Int16, Dither::Shaped, DEFAULT_SEED);
        apply(&mut b, BitDepth::Int16, Dither::Shaped, DEFAULT_SEED);

        assert_eq!(a.channel(1), b.channel(1), "the shaper leaked across channels");
        // And the first channels really were different, so the test had
        // something to leak.
        assert_ne!(a.channel(0), b.channel(0));
    }

    #[test]
    fn the_generator_does_not_get_stuck() {
        // Xorshift cannot leave a zero state, so a zero seed has to be replaced.
        let mut rng = Xorshift64Star::new(0);
        let first = rng.next_u64();
        assert_ne!(first, 0);
        assert_ne!(first, rng.next_u64());

        // Every draw stays in [0, 1).
        let mut rng = Xorshift64Star::new(DEFAULT_SEED);
        for _ in 0..10_000 {
            let x = rng.next_f64();
            assert!((0.0..1.0).contains(&x), "{x} out of range");
        }
    }
}
