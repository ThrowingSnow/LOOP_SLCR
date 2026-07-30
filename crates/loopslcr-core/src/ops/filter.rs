//! One-pole and biquad filters, and the trick that makes them loop-safe.
//!
//! # Why a filter is a problem for a loop at all
//!
//! A filter has state. Started from silence it needs time to settle, so the
//! first few hundred samples of the output are not what the filter would have
//! produced had the signal been running forever — and in a loop the signal *has*
//! been running forever, because the material before frame 0 is the material
//! before the end. Filtering from a cleared state puts a transient at exactly
//! the seam this tool exists to clean.
//!
//! [`run_periodic`] is the fix, and it is the same warmup trick the rest of the
//! program uses on audio: run the loop twice through the filter and keep the
//! second pass, carrying the state across. The state entering the kept pass then
//! differs from the true periodic steady state by the filter's decay over a
//! whole loop period — `|pole|^frames`. For a 40 Hz pole at 44.1 kHz that factor
//! is about `0.994^800000`, which underflows to zero long before it reaches an
//! audible level. Exact in floating point, not merely close.
//!
//! This only holds for a filter whose coefficients stay put. A filter whose
//! cutoff is itself being modulated has no steady state to settle into, which is
//! why the tape wobble runs before these and not through them.

// Filtering is the sample domain.
#![allow(clippy::float_arithmetic)]

use std::f64::consts::PI;

/// A single-sample filter with internal state.
pub trait Filter {
    fn process(&mut self, x: f64) -> f64;
    fn reset(&mut self);
}

/// Filters `samples` in place as one period of an endlessly repeating loop.
///
/// Two passes: the first only to charge the state, the second kept. See the
/// module documentation for why this is exact rather than approximate.
pub fn run_periodic<F: Filter + ?Sized>(filter: &mut F, samples: &mut [f64]) {
    filter.reset();
    for &s in samples.iter() {
        filter.process(s);
    }
    for s in samples.iter_mut() {
        *s = filter.process(*s);
    }
}

/// A 6 dB/octave lowpass.
///
/// One pole rather than a steeper design because that is what tape HF loss
/// actually looks like: a gentle slope starting well below the corner, not a
/// shelf with a knee. A brick wall would sound like a codec, not like tape.
#[derive(Copy, Clone, Debug)]
pub struct OnePole {
    coefficient: f64,
    state: f64,
}

impl OnePole {
    /// Lowpass at `cutoff_hz`, the −3 dB point.
    ///
    /// The cutoff is clamped below Nyquist: the tape character scales its cutoff
    /// with playback speed, and pitching a 12 kHz corner up far enough would
    /// otherwise ask for a pole outside the unit circle — which does not sound
    /// like a bright tape, it sounds like an explosion.
    pub fn lowpass(cutoff_hz: f64, sample_rate: u32) -> Self {
        let nyquist = sample_rate as f64 / 2.0;
        let cutoff = cutoff_hz.clamp(1.0, nyquist * 0.99);
        OnePole {
            coefficient: 1.0 - (-2.0 * PI * cutoff / sample_rate as f64).exp(),
            state: 0.0,
        }
    }
}

impl Filter for OnePole {
    fn process(&mut self, x: f64) -> f64 {
        self.state += self.coefficient * (x - self.state);
        self.state
    }

    fn reset(&mut self) {
        self.state = 0.0;
    }
}

/// A second-order section in direct form 1.
#[derive(Copy, Clone, Debug)]
pub struct Biquad {
    b0: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,
    x1: f64,
    x2: f64,
    y1: f64,
    y2: f64,
}

impl Biquad {
    /// Peaking EQ, per the Audio EQ Cookbook.
    ///
    /// `q` around 0.7 gives the broad, gentle lift a head bump actually is; a
    /// high `q` would ring, and a resonance sitting on the kick drum of every
    /// repeat is not character, it is a fault.
    pub fn peaking(freq_hz: f64, gain_db: f64, q: f64, sample_rate: u32) -> Self {
        let nyquist = sample_rate as f64 / 2.0;
        let freq = freq_hz.clamp(1.0, nyquist * 0.99);
        let amplitude = 10.0f64.powf(gain_db / 40.0);
        let w0 = 2.0 * PI * freq / sample_rate as f64;
        let alpha = w0.sin() / (2.0 * q.max(1e-3));
        let cos_w0 = w0.cos();

        let a0 = 1.0 + alpha / amplitude;
        Biquad {
            b0: (1.0 + alpha * amplitude) / a0,
            b1: (-2.0 * cos_w0) / a0,
            b2: (1.0 - alpha * amplitude) / a0,
            a1: (-2.0 * cos_w0) / a0,
            a2: (1.0 - alpha / amplitude) / a0,
            x1: 0.0,
            x2: 0.0,
            y1: 0.0,
            y2: 0.0,
        }
    }

    /// Magnitude response at `freq_hz`, for reporting and for tests.
    pub fn magnitude_at(&self, freq_hz: f64, sample_rate: u32) -> f64 {
        let w = 2.0 * PI * freq_hz / sample_rate as f64;
        // H(z) at z = e^{jw}, evaluated as two complex polynomials.
        let (c1, s1) = ((-w).cos(), (-w).sin());
        let (c2, s2) = ((-2.0 * w).cos(), (-2.0 * w).sin());
        let num = (
            self.b0 + self.b1 * c1 + self.b2 * c2,
            self.b1 * s1 + self.b2 * s2,
        );
        let den = (1.0 + self.a1 * c1 + self.a2 * c2, self.a1 * s1 + self.a2 * s2);
        (num.0 * num.0 + num.1 * num.1).sqrt() / (den.0 * den.0 + den.1 * den.1).sqrt()
    }
}

impl Filter for Biquad {
    fn process(&mut self, x: f64) -> f64 {
        let y = self.b0 * x + self.b1 * self.x1 + self.b2 * self.x2
            - self.a1 * self.y1
            - self.a2 * self.y2;
        self.x2 = self.x1;
        self.x1 = x;
        self.y2 = self.y1;
        self.y1 = y;
        y
    }

    fn reset(&mut self) {
        self.x1 = 0.0;
        self.x2 = 0.0;
        self.y1 = 0.0;
        self.y2 = 0.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Amplitude of a tone at `cycles` per buffer, by direct correlation.
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

    fn tone(frames: usize, cycles: usize, amplitude: f64) -> Vec<f64> {
        (0..frames)
            .map(|i| amplitude * (2.0 * PI * cycles as f64 * i as f64 / frames as f64).sin())
            .collect()
    }

    #[test]
    fn a_lowpass_is_three_db_down_at_its_corner() {
        // 44100 / 1000 frames = 44.1 Hz per cycle. 100 cycles is 4410 Hz.
        let mut f = OnePole::lowpass(4410.0, 44_100);
        let mut samples = tone(1000, 100, 0.5);
        run_periodic(&mut f, &mut samples);
        let out = magnitude_at(&samples, 100.0);
        // −3 dB is a factor 0.7071. A one-pole is close but not exact at the
        // corner because the analogue prototype is warped by the exponential.
        assert!(
            (out / 0.5 - std::f64::consts::FRAC_1_SQRT_2).abs() < 0.02,
            "gain at the corner: {}",
            out / 0.5
        );
    }

    /// The 6 dB/octave law, measured rather than assumed.
    ///
    /// A one-pole is gentle — an octave above the corner is only −7 dB — and that
    /// gentleness is the point: it is what tape HF loss looks like. Testing
    /// against the analogue prototype `1/sqrt(1 + r²)` states that intent, where
    /// a single "the top is gone" threshold would pass for any filter at all.
    ///
    /// Only up to a quarter of the sample rate. Beyond that the digital pole
    /// flattens into a shelf instead of continuing the slope — at Nyquist the
    /// gain is `a/(2-a)`, not zero — so the prototype stops describing it.
    #[test]
    fn a_lowpass_follows_the_six_db_per_octave_law() {
        // 4410 frames at 44.1 kHz is 0.1 s, so one cycle per buffer is 10 Hz and
        // the 1 kHz corner is 100 cycles. Stepping in whole cycles keeps every
        // test tone exactly periodic in the buffer, so the measurement is clean.
        let corner = 1000.0;
        for cycles in [25usize, 50, 100, 200, 400, 800] {
            let ratio = cycles as f64 * 10.0 / corner;
            let mut samples = tone(4410, cycles, 0.5);
            run_periodic(&mut OnePole::lowpass(corner, 44_100), &mut samples);

            let measured = magnitude_at(&samples, cycles as f64) / 0.5;
            let ideal = 1.0 / (1.0 + ratio * ratio).sqrt();
            let hz = cycles * 10;
            assert!(
                (measured - ideal).abs() < 0.02,
                "{hz} Hz: measured {measured}, prototype {ideal}"
            );
        }
    }

    #[test]
    fn dc_passes_a_lowpass_at_unity() {
        let mut samples = vec![0.5; 5000];
        run_periodic(&mut OnePole::lowpass(1000.0, 44_100), &mut samples);
        for (i, &s) in samples.iter().enumerate() {
            assert!((s - 0.5).abs() < 1e-12, "sample {i} is {s}");
        }
    }

    /// The reason for [`run_periodic`]. A single pass leaves the head of the
    /// buffer climbing out of silence, which in a loop is a click every repeat.
    #[test]
    fn the_second_pass_removes_the_startup_transient() {
        let samples = tone(4410, 10, 0.5);

        let mut once = samples.clone();
        let mut f = OnePole::lowpass(200.0, 44_100);
        f.reset();
        for s in once.iter_mut() {
            *s = f.process(*s);
        }

        let mut twice = samples.clone();
        run_periodic(&mut OnePole::lowpass(200.0, 44_100), &mut twice);

        // The seam is the join between the last sample and the first. A settled
        // filter steps between them by about as much as anywhere else in the
        // waveform; an unsettled one jumps.
        let step = |v: &[f64]| (v[0] - v[v.len() - 1]).abs();
        let typical = |v: &[f64]| {
            v.windows(2)
                .map(|w| (w[1] - w[0]).abs())
                .fold(0.0f64, f64::max)
        };

        assert!(
            step(&twice) <= typical(&twice) * 1.001,
            "warmed seam jumps by {} against a typical {}",
            step(&twice),
            typical(&twice)
        );
        assert!(
            step(&once) > typical(&once) * 5.0,
            "cold seam is unexpectedly smooth: {} against {}",
            step(&once),
            typical(&once)
        );
    }

    #[test]
    fn a_peaking_filter_lifts_its_centre_and_leaves_the_rest() {
        let f = Biquad::peaking(60.0, 6.0, 0.7, 44_100);
        let gain_db = |hz: f64| 20.0 * f.magnitude_at(hz, 44_100).log10();

        assert!((gain_db(60.0) - 6.0).abs() < 0.01, "centre: {}", gain_db(60.0));
        // Broad at Q 0.7, but back to nothing by the top of the band.
        assert!(gain_db(1000.0) < 0.6, "1 kHz: {}", gain_db(1000.0));
        assert!(gain_db(10_000.0).abs() < 0.05, "10 kHz: {}", gain_db(10_000.0));
    }

    #[test]
    fn a_peaking_filters_measured_gain_matches_its_computed_response() {
        // The response function is used for reporting, so it has to agree with
        // what the filter actually does to a signal.
        let mut samples = tone(4410, 6, 0.25); // 60 Hz
        let f = Biquad::peaking(60.0, 6.0, 0.7, 44_100);
        run_periodic(&mut f.clone(), &mut samples);

        let measured = magnitude_at(&samples, 6.0) / 0.25;
        let predicted = f.magnitude_at(60.0, 44_100);
        assert!(
            (measured - predicted).abs() < 1e-6,
            "measured {measured}, predicted {predicted}"
        );
    }

    #[test]
    fn zero_gain_peaking_is_a_pass_through() {
        let f = Biquad::peaking(60.0, 0.0, 0.7, 44_100);
        for hz in [20.0, 60.0, 500.0, 5000.0, 20_000.0] {
            assert!((f.magnitude_at(hz, 44_100) - 1.0).abs() < 1e-12, "at {hz} Hz");
        }
    }

    #[test]
    fn a_cutoff_past_nyquist_stays_stable() {
        // What a large upward varispeed does to a scaled cutoff. It must clamp,
        // not produce a pole outside the unit circle.
        let mut samples = tone(2000, 50, 0.5);
        run_periodic(&mut OnePole::lowpass(400_000.0, 44_100), &mut samples);
        assert!(samples.iter().all(|s| s.is_finite() && s.abs() <= 1.0));
        // And at that point it is very nearly a pass-through.
        assert!(magnitude_at(&samples, 50.0) > 0.45);
    }
}
