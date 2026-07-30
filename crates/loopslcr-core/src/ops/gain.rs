//! Peak measurement and the one gain change the tool will make.
//!
//! Normalising is opt-in and always reported. A foldback can push a loop past
//! full scale, and the right response depends on the material: scale it down,
//! re-render quieter, or write float and decide later. The tool measures and
//! says; it does not quietly adjust.

// Gain is the sample domain.
#![allow(clippy::float_arithmetic)]

use crate::buffer::AudioBuffer;

/// What a peak measurement found.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Peak {
    /// Largest absolute sample value across all channels.
    pub value: f64,
    /// Frame it occurs in. Zero for silence.
    pub frame: usize,
    pub channel: usize,
}

impl Peak {
    pub fn measure(buffer: &AudioBuffer) -> Self {
        let mut peak = Peak {
            value: 0.0,
            frame: 0,
            channel: 0,
        };
        for (channel, samples) in buffer.channels().iter().enumerate() {
            for (frame, &s) in samples.iter().enumerate() {
                if s.abs() > peak.value {
                    peak = Peak {
                        value: s.abs(),
                        frame,
                        channel,
                    };
                }
            }
        }
        peak
    }

    /// True when quantising to integer would clip.
    pub fn clips(&self) -> bool {
        self.value > 1.0
    }

    /// Level in dBFS. Silence reports negative infinity, which is what it is.
    pub fn dbfs(&self) -> f64 {
        if self.value <= 0.0 {
            f64::NEG_INFINITY
        } else {
            20.0 * self.value.log10()
        }
    }

    /// Headroom to full scale in dB — negative when the signal is over.
    pub fn headroom_db(&self) -> f64 {
        -self.dbfs()
    }
}

/// Scales `buffer` by `factor`, returning the new peak.
pub fn apply(buffer: &mut AudioBuffer, factor: f64) -> Peak {
    if factor != 1.0 {
        for channel in buffer.channels_mut() {
            for s in channel.iter_mut() {
                *s *= factor;
            }
        }
    }
    Peak::measure(buffer)
}

/// Scales `buffer` so its peak sits at `target`, and returns the factor used.
///
/// Silence is left alone rather than amplified to nothing: dividing by a zero
/// peak has no meaningful answer, and the alternative — a huge factor on the
/// noise floor — is worse than doing nothing.
pub fn normalize(buffer: &mut AudioBuffer, target: f64) -> Option<f64> {
    let peak = Peak::measure(buffer).value;
    if peak <= 0.0 || target <= 0.0 {
        return None;
    }
    let factor = target / peak;
    apply(buffer, factor);
    Some(factor)
}

/// The default normalising target: full scale.
///
/// Not a hair under. Quantisation clamps `1.0` to the largest representable
/// value, which is one step below — a difference of one LSB, inaudible, and
/// preferable to inventing a headroom figure nobody asked for.
pub const FULL_SCALE: f64 = 1.0;

#[cfg(test)]
mod tests {
    use super::*;

    fn buf(samples: Vec<Vec<f64>>) -> AudioBuffer {
        AudioBuffer::new(samples, 44_100)
    }

    #[test]
    fn finds_the_peak_and_where_it_is() {
        let b = buf(vec![vec![0.1, -0.9, 0.2], vec![0.3, 0.4, 0.5]]);
        let p = Peak::measure(&b);
        assert_eq!(p.value, 0.9);
        assert_eq!(p.frame, 1);
        assert_eq!(p.channel, 0);
        assert!(!p.clips());
    }

    #[test]
    fn a_peak_past_unity_is_reported_as_clipping() {
        let p = Peak::measure(&buf(vec![vec![0.5, 1.3]]));
        assert!(p.clips());
        assert!((p.dbfs() - 2.2789).abs() < 0.001, "{}", p.dbfs());
        assert!(p.headroom_db() < 0.0);
    }

    #[test]
    fn full_scale_is_zero_dbfs_and_silence_is_minus_infinity() {
        assert_eq!(Peak::measure(&buf(vec![vec![-1.0, 0.5]])).dbfs(), 0.0);
        let silent = Peak::measure(&AudioBuffer::silence(2, 10, 44_100));
        assert_eq!(silent.value, 0.0);
        assert_eq!(silent.dbfs(), f64::NEG_INFINITY);
        assert!(!silent.clips());
    }

    #[test]
    fn normalising_puts_the_peak_exactly_on_target() {
        let mut b = buf(vec![vec![0.25, -0.5], vec![0.1, 0.2]]);
        let factor = normalize(&mut b, FULL_SCALE).unwrap();
        assert_eq!(factor, 2.0);
        assert_eq!(Peak::measure(&b).value, 1.0);
        // Every channel is scaled by the same factor — the image is preserved.
        assert_eq!(b.channel(0), &[0.5, -1.0]);
        assert_eq!(b.channel(1), &[0.2, 0.4]);
    }

    #[test]
    fn normalising_brings_an_overshoot_back_down() {
        let mut b = buf(vec![vec![0.8, 1.6]]);
        assert_eq!(normalize(&mut b, FULL_SCALE), Some(0.625));
        assert!(!Peak::measure(&b).clips());
        assert_eq!(b.channel(0), &[0.5, 1.0]);
    }

    #[test]
    fn silence_is_left_alone_rather_than_amplified() {
        let mut b = AudioBuffer::silence(1, 8, 44_100);
        assert_eq!(normalize(&mut b, FULL_SCALE), None);
        assert!(b.channel(0).iter().all(|&s| s == 0.0));
        // A nonsensical target is refused too, not applied.
        let mut real = buf(vec![vec![0.5]]);
        assert_eq!(normalize(&mut real, 0.0), None);
        assert_eq!(real.channel(0), &[0.5]);
    }

    #[test]
    fn unity_gain_touches_nothing() {
        let source = buf(vec![vec![0.1, -0.2, 0.3]]);
        let mut b = source.clone();
        apply(&mut b, 1.0);
        assert_eq!(b, source);
    }
}
