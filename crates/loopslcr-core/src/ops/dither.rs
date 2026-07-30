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
}

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

/// Adds TPDF noise of ±1 LSB of `depth`.
///
/// Returns the LSB size used, or `None` when nothing was done. Applying this to
/// a float output, or with [`Dither::None`], is a no-op rather than an error:
/// the caller decides policy, this decides nothing.
pub fn apply(buffer: &mut AudioBuffer, depth: BitDepth, mode: Dither, seed: u64) -> Option<f64> {
    if mode == Dither::None || depth.is_float() {
        return None;
    }
    // One LSB of the target grid, matching how the writer scales: full-scale
    // negative is exactly -1.0, so the step is 2^-(bits-1).
    let lsb = 1.0 / (1i64 << (depth.bits() - 1)) as f64;

    let mut rng = Xorshift64Star::new(seed);
    for channel in buffer.channels_mut() {
        for s in channel.iter_mut() {
            // The difference of two independent uniforms is triangular, peaking
            // at zero and vanishing at ±1 LSB. One uniform would be flat-topped
            // and leave the error still partly correlated with the signal.
            *s += (rng.next_f64() - rng.next_f64()) * lsb;
        }
    }
    Some(lsb)
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
            let lsb = apply(&mut b, depth, Dither::Tpdf, DEFAULT_SEED).unwrap();
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
        let lsb = apply(&mut b, BitDepth::Int16, Dither::Tpdf, DEFAULT_SEED).unwrap();
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
