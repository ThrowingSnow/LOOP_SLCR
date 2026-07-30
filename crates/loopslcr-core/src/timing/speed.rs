//! Varispeed ratios and the taper laws that produce them.
//!
//! One number reaches the audio: the playback speed ratio. `2.0` is twice as
//! fast and an octave up; `0.5` is half speed and an octave down. Pitch and
//! tempo always move together — this is a tape transport, not a pitch shifter,
//! and that is what lets a perfect loop stay a perfect loop.
//!
//! **Two of the three drive modes are exact.** Fitting 103 BPM to 90 is the
//! ratio 90/103, a rational number; ±5 % is 21/20 or 19/20; a whole octave is
//! 2/1. Only a semitone interval is irrational — 2^(1/12) is not a fraction —
//! and even there the octaves come back exact. Keeping that distinction visible
//! is worth the enum: when the ratio is rational, the resampled loop length
//! stays a single rounding away from exact, the same guarantee the cut has.

// Taper laws are transcendental by nature: 2^(st/12) and its inverse. This is
// the parameter domain, not the sample domain — the ratio that comes out is
// exact whenever it can be.
#![allow(clippy::float_arithmetic)]

use std::fmt;

use crate::error::{Error, Result};
use crate::rational::Rational;

use super::tempo::Tempo;

/// A playback speed ratio, exact when it can be.
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum Ratio {
    /// A rational ratio: BPM fitting, percentages, whole octaves. The resulting
    /// tempo and loop length are exact.
    Exact(Rational),
    /// An irrational ratio: any semitone interval that is not a whole octave.
    Approx(f64),
}

impl Ratio {
    pub const UNITY: Ratio = Ratio::Exact(Rational::ONE);

    /// # Errors
    /// If `ratio` is not strictly positive — a tape cannot run backwards here,
    /// and zero speed is not a speed.
    pub fn exact(ratio: Rational) -> Result<Self> {
        if ratio.is_zero() || ratio.is_negative() {
            return Err(Error::SpeedRatio(ratio.to_f64()));
        }
        Ok(Ratio::Exact(ratio))
    }

    /// # Errors
    /// If `ratio` is not strictly positive, or is not finite.
    pub fn approx(ratio: f64) -> Result<Self> {
        if !ratio.is_finite() || ratio <= 0.0 {
            return Err(Error::SpeedRatio(ratio));
        }
        Ok(Ratio::Approx(ratio))
    }

    /// The semitone law — sampler behaviour, symmetric in both directions.
    ///
    /// Whole octaves come back exact: 12 semitones is 2/1, not 1.9999999999.
    /// That matters, because an exact ratio keeps the output length exact too.
    pub fn from_semitones(semitones: f64) -> Result<Self> {
        if !semitones.is_finite() {
            return Err(Error::SpeedRatio(semitones));
        }
        // A whole number of octaves is a power of two, which `i128` holds up to
        // ±126 — far beyond anything audible, but the guard keeps it honest.
        let octaves = semitones / 12.0;
        if octaves.fract() == 0.0 && octaves.abs() <= 100.0 {
            let n = octaves as i32;
            let power = 1i128 << n.unsigned_abs();
            return Ratio::exact(if n >= 0 {
                Rational::from_int(power)
            } else {
                Rational::new(1, power)
            });
        }
        Ratio::approx(2f64.powf(octaves))
    }

    /// The semitone law in cents. 100 cents is one semitone.
    pub fn from_cents(cents: f64) -> Result<Self> {
        Ratio::from_semitones(cents / 100.0)
    }

    /// The speed law — real tape, a percentage of nominal speed.
    ///
    /// Deliberately asymmetric: +50 % is +7.02 semitones while −50 % is −12.
    /// That asymmetry *is* the tape feel, and it is why this law exists next to
    /// the semitone one rather than instead of it.
    pub fn from_percent(percent: Rational) -> Result<Self> {
        Ratio::exact(Rational::ONE + percent / Rational::from_int(100))
    }

    /// The BPM-driven mode: the ratio that turns `from` into `to`.
    ///
    /// Exact, always. Fitting a loop to a track is the common case and it costs
    /// no precision at all.
    pub fn from_tempi(from: Tempo, to: Tempo) -> Result<Self> {
        Ratio::exact(to.value() / from.value())
    }

    pub fn to_f64(self) -> f64 {
        match self {
            Ratio::Exact(r) => r.to_f64(),
            Ratio::Approx(x) => x,
        }
    }

    /// The rational value, when there is one.
    pub fn as_rational(self) -> Option<Rational> {
        match self {
            Ratio::Exact(r) => Some(r),
            Ratio::Approx(_) => None,
        }
    }

    pub fn is_exact(self) -> bool {
        matches!(self, Ratio::Exact(_))
    }

    pub fn is_unity(self) -> bool {
        match self {
            Ratio::Exact(r) => r == Rational::ONE,
            Ratio::Approx(x) => x == 1.0,
        }
    }

    /// The interval in semitones. Reporting: the display shows all three units
    /// at once, whichever law produced the ratio.
    pub fn semitones(self) -> f64 {
        12.0 * self.to_f64().log2()
    }

    pub fn cents(self) -> f64 {
        self.semitones() * 100.0
    }

    /// Deviation from nominal speed, in percent.
    pub fn percent(self) -> f64 {
        (self.to_f64() - 1.0) * 100.0
    }

    /// The tempo a loop ends up at. Exact when the ratio is.
    ///
    /// An approximate ratio has to be forced into a rational to be a [`Tempo`]
    /// at all; it is rounded to a thousandth of a BPM, which is finer than any
    /// tag format records and keeps the value readable.
    pub fn resulting_tempo(self, from: Tempo) -> Result<Tempo> {
        match self {
            Ratio::Exact(r) => from.scaled(r),
            Ratio::Approx(x) => {
                let scaled = from.value().to_f64() * x;
                if !scaled.is_finite() {
                    return Err(Error::SpeedRatio(x));
                }
                Tempo::new(
                    Rational::new((scaled * 1000.0).round() as i128, 1000),
                    from.unit(),
                )
            }
        }
    }

    /// Output length for an input of `frames`, rounded once.
    ///
    /// Only for material with no bar grid behind it. When the tempo and bar
    /// count are known, [`super::Grid::resampled_length`] is the right call: it
    /// rounds from the exact bar mathematics instead of from an already-rounded
    /// cut length, which can differ by a sample.
    pub fn output_frames(self, frames: usize) -> usize {
        match self {
            Ratio::Exact(r) => (Rational::from_int(frames as i128) / r).round_half_up() as usize,
            Ratio::Approx(x) => (frames as f64 / x).round() as usize,
        }
    }
}

impl fmt::Display for Ratio {
    /// All three units at once — the taper affects feel, never information.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_unity() {
            return f.write_str("unity");
        }
        write!(
            f,
            "{:+.3} st ({:+.1} cents), {:+.3} % speed, ratio {:.9}",
            self.semitones(),
            self.cents(),
            self.percent(),
            self.to_f64()
        )?;
        if let Ratio::Exact(r) = self {
            if !r.is_integer() {
                write!(f, " = {r}")?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::timing::BpmUnit;

    fn t(bpm: u32) -> Tempo {
        Tempo::bpm(bpm).unwrap()
    }

    #[test]
    fn fitting_one_tempo_to_another_is_exact() {
        // The documented example: 103 → 90 BPM.
        let r = Ratio::from_tempi(t(103), t(90)).unwrap();
        assert_eq!(r.as_rational(), Some(Rational::new(90, 103)));
        assert!(r.is_exact());
        // −2.3358 semitones — the docs' "−2.34" rounded to two places.
        assert!((r.semitones() - -2.335_769).abs() < 1e-6, "{}", r.semitones());
        // And the tempo comes back exactly, not 89.99999.
        assert_eq!(r.resulting_tempo(t(103)).unwrap().value(), Rational::from_int(90));
    }

    #[test]
    fn whole_octaves_are_exact_despite_the_semitone_law() {
        for (st, expected) in [
            (12.0, Rational::from_int(2)),
            (24.0, Rational::from_int(4)),
            (-12.0, Rational::new(1, 2)),
            (-24.0, Rational::new(1, 4)),
            (0.0, Rational::ONE),
        ] {
            let r = Ratio::from_semitones(st).unwrap();
            assert_eq!(r.as_rational(), Some(expected), "{st} st");
        }
    }

    #[test]
    fn a_semitone_that_is_not_an_octave_is_approximate() {
        let r = Ratio::from_semitones(1.0).unwrap();
        assert!(!r.is_exact());
        assert_eq!(r.as_rational(), None);
        // 2^(1/12)
        assert!((r.to_f64() - 1.059_463_094_359_295).abs() < 1e-15);
        // Round-trips back through the reporting side.
        assert!((r.semitones() - 1.0).abs() < 1e-12);
        assert!((r.cents() - 100.0).abs() < 1e-9);
    }

    #[test]
    fn the_speed_law_is_asymmetric_and_that_is_the_point() {
        // The docs' own numbers: +50 % is +7.02 st, −50 % is −12 st.
        let up = Ratio::from_percent(Rational::from_int(50)).unwrap();
        let down = Ratio::from_percent(Rational::from_int(-50)).unwrap();
        assert_eq!(up.as_rational(), Some(Rational::new(3, 2)));
        assert_eq!(down.as_rational(), Some(Rational::new(1, 2)));
        assert!((up.semitones() - 7.0195).abs() < 0.001, "{}", up.semitones());
        assert_eq!(down.semitones(), -12.0);
        // Equal percentages are not equal intervals.
        assert!(up.semitones() + down.semitones() < 0.0);
    }

    #[test]
    fn the_three_units_agree_with_each_other() {
        for st in [-12.0, -7.0, -2.5, -0.01, 0.0, 0.01, 3.0, 7.02, 12.0] {
            let r = Ratio::from_semitones(st).unwrap();
            assert!((r.semitones() - st).abs() < 1e-9, "{st} st");
            assert!((r.cents() - st * 100.0).abs() < 1e-6, "{st} st in cents");
            // The percent reading has to describe the same ratio.
            assert!(
                (r.percent() / 100.0 + 1.0 - r.to_f64()).abs() < 1e-12,
                "{st} st in percent"
            );
        }
    }

    #[test]
    fn cents_and_semitones_are_the_same_law() {
        assert_eq!(
            Ratio::from_cents(-234.0).unwrap().to_f64(),
            Ratio::from_semitones(-2.34).unwrap().to_f64()
        );
        // 1200 cents is an octave, so it must come back exact.
        assert_eq!(
            Ratio::from_cents(1200.0).unwrap().as_rational(),
            Some(Rational::from_int(2))
        );
    }

    #[test]
    fn output_length_is_the_input_divided_by_the_ratio() {
        // Half speed doubles the length.
        let half = Ratio::from_semitones(-12.0).unwrap();
        assert_eq!(half.output_frames(822_058), 1_644_116);
        // Double speed halves it, rounding once.
        let double = Ratio::from_semitones(12.0).unwrap();
        assert_eq!(double.output_frames(822_058), 411_029);
        assert_eq!(double.output_frames(822_059), 411_030); // .5 rounds up
        // Unity changes nothing at all.
        assert_eq!(Ratio::UNITY.output_frames(12_345), 12_345);
    }

    #[test]
    fn a_ratio_must_be_positive_and_finite() {
        assert!(Ratio::exact(Rational::ZERO).is_err());
        assert!(Ratio::exact(Rational::from_int(-2)).is_err());
        assert!(Ratio::approx(0.0).is_err());
        assert!(Ratio::approx(-1.0).is_err());
        assert!(Ratio::approx(f64::NAN).is_err());
        assert!(Ratio::approx(f64::INFINITY).is_err());
        assert!(Ratio::from_semitones(f64::NAN).is_err());
        // −100 % is a full stop, not a speed.
        assert!(Ratio::from_percent(Rational::from_int(-100)).is_err());
    }

    #[test]
    fn the_bpm_unit_survives_a_speed_change() {
        // Scaling the tempo must not silently reinterpret which note it counts.
        let dotted = t(103).with_unit(BpmUnit::dotted_quarter());
        let out = Ratio::from_percent(Rational::from_int(50))
            .unwrap()
            .resulting_tempo(dotted)
            .unwrap();
        assert_eq!(out.unit(), BpmUnit::dotted_quarter());
        assert_eq!(out.value(), Rational::new(309, 2)); // 103 × 1.5
    }

    #[test]
    fn an_approximate_ratio_still_yields_a_usable_tempo() {
        // 103 BPM down two semitones.
        let r = Ratio::from_semitones(-2.0).unwrap();
        let out = r.resulting_tempo(t(103)).unwrap();
        // 103 × 2^(-2/12) = 91.76256…, kept to a thousandth of a BPM.
        assert_eq!(out.value(), Rational::new(91_763, 1000));
        assert!((out.value().to_f64() - 103.0 * r.to_f64()).abs() < 0.001);
    }

    #[test]
    fn display_shows_every_unit() {
        let s = Ratio::from_tempi(t(103), t(90)).unwrap().to_string();
        assert!(s.contains("-2.336 st"), "{s}");
        assert!(s.contains("cents"), "{s}");
        assert!(s.contains("% speed"), "{s}");
        assert!(s.contains("90/103"), "{s}");
        assert_eq!(Ratio::UNITY.to_string(), "unity");
    }
}
