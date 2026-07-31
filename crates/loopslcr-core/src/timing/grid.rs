//! The bar grid — the single authority on where a cut lands.
//!
//! Everything here is exact. A cut point is one `i128` product, one division
//! and one round-half-up; it is never accumulated bar by bar, so there is no
//! drift to accumulate in the first place.

use std::fmt;

use crate::error::Result;
use crate::rational::Rational;

use super::signature::TimeSignature;
use super::speed::Ratio;
use super::tempo::Tempo;

/// Which of the two constraints wins when the bar length is not a whole number
/// of samples — they cannot both hold at once.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
pub enum Align {
    /// Cut-in on the grid, length = `round(loop_bars · samples_per_bar)`.
    ///
    /// The default: for a loop, a length that is exactly right matters more
    /// than an end marker that sits on the grid, because the length is what
    /// determines whether it stays in sync after the hundredth repeat.
    #[default]
    Loop,
    /// Both markers on the grid; the length may differ by a sample.
    Grid,
}

impl fmt::Display for Align {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Align::Loop => "loop",
            Align::Grid => "grid",
        })
    }
}

/// A half-open sample region `[start, end)`.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Region {
    pub start: u64,
    pub end: u64,
}

impl Region {
    pub fn len(self) -> u64 {
        self.end - self.start
    }

    pub fn is_empty(self) -> bool {
        self.end == self.start
    }
}

/// How far a rounded sample position sits from the exact one.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Residual {
    /// Rounded minus exact, in samples. Always within ±1/2.
    pub samples: Rational,
    sample_rate: u32,
    exact_samples: Rational,
}

impl Residual {
    /// Signed error in microseconds.
    ///
    /// Reporting only — the error itself is held exactly in `samples`.
    #[allow(clippy::float_arithmetic)]
    pub fn micros(&self) -> f64 {
        self.samples.to_f64() / self.sample_rate as f64 * 1e6
    }

    /// Signed error relative to the region's own length, in parts per million.
    ///
    /// Reporting only, as with [`Residual::micros`].
    #[allow(clippy::float_arithmetic)]
    pub fn ppm(&self) -> f64 {
        if self.exact_samples.is_zero() {
            0.0
        } else {
            (self.samples / self.exact_samples).to_f64() * 1e6
        }
    }

    /// True when the position lands on an exact sample — no rounding at all.
    pub fn is_exact(&self) -> bool {
        self.samples.is_zero()
    }
}

impl fmt::Display for Residual {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_exact() {
            f.write_str("exact")
        } else {
            write!(f, "{:+.3} µs ({:+.3} ppm)", self.micros(), self.ppm())
        }
    }
}

/// Tempo, meter and sample rate — everything needed to place a bar line.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Grid {
    pub tempo: Tempo,
    pub sig: TimeSignature,
    pub sample_rate: u32,
}

impl Grid {
    pub fn new(tempo: Tempo, sig: TimeSignature, sample_rate: u32) -> Self {
        assert!(sample_rate > 0, "sample rate must be positive");
        Grid {
            tempo,
            sig,
            sample_rate,
        }
    }

    /// Bar length in seconds:
    /// `(60/BPM) · (N/D) / U` = `60·N·b·q / (p·D·a)`.
    pub fn seconds_per_bar(&self) -> Rational {
        Rational::from_int(60) / self.tempo.value() * self.sig.whole_notes_per_bar()
            / self.tempo.unit().whole_notes()
    }

    /// How long `whole_notes` of music lasts, in seconds.
    ///
    /// The shape every length on this grid comes from: a bar is
    /// [`TimeSignature::whole_notes_per_bar`] of it, a note value is its own
    /// fraction of a whole note. Exposed so [`note`](super::note) computes note
    /// lengths through the same expression the cut points use rather than a
    /// parallel one that merely agrees.
    pub fn seconds_per_whole_notes(&self, whole_notes: Rational) -> Rational {
        Rational::from_int(60) / self.tempo.value() * whole_notes
            / self.tempo.unit().whole_notes()
    }

    /// Bar length in samples — exact, generally not an integer.
    pub fn samples_per_bar(&self) -> Rational {
        self.seconds_per_bar() * Rational::from(self.sample_rate)
    }

    /// Beat length in samples, where a beat is one BPM unit.
    pub fn samples_per_beat(&self) -> Rational {
        Rational::from_int(60) / self.tempo.value() * Rational::from(self.sample_rate)
    }

    /// The exact, unrounded sample position of bar line `bar`.
    pub fn exact_sample(&self, bar: u64) -> Rational {
        self.samples_per_bar() * Rational::from(bar)
    }

    /// The sample position of bar line `bar`.
    ///
    /// `round(bar · SR · 60 · N · b · q / (p · D · a))` — a single expression,
    /// never a running sum, so bar 10 000 is as accurate as bar 1.
    pub fn cut_sample(&self, bar: u64) -> u64 {
        let s = self.exact_sample(bar).round_half_up();
        debug_assert!(s >= 0, "bar line before the start of the file");
        s as u64
    }

    /// How far `cut_sample(bar)` sits from the exact bar line.
    pub fn residual(&self, bar: u64) -> Residual {
        let exact = self.exact_sample(bar);
        Residual {
            samples: Rational::from_int(exact.round_half_up()) - exact,
            sample_rate: self.sample_rate,
            exact_samples: exact,
        }
    }

    /// The region to extract: skip `skip_bars` of warmup, keep `loop_bars`.
    pub fn region(&self, skip_bars: u64, loop_bars: u64, align: Align) -> Region {
        let start = self.cut_sample(skip_bars);
        let end = match align {
            // Length is what has to be exact, so it is rounded on its own
            // rather than inherited from the difference of two bar lines.
            Align::Loop => start + self.exact_sample(loop_bars).round_half_up() as u64,
            Align::Grid => self.cut_sample(skip_bars + loop_bars),
        };
        Region { start, end }
    }

    /// Residual of the extracted *length* under `align` — what actually decides
    /// whether the loop drifts against a sequencer over many repeats.
    pub fn length_residual(&self, skip_bars: u64, loop_bars: u64, align: Align) -> Residual {
        let region = self.region(skip_bars, loop_bars, align);
        let exact = self.exact_sample(loop_bars);
        Residual {
            samples: Rational::from_int(region.len() as i128) - exact,
            sample_rate: self.sample_rate,
            exact_samples: exact,
        }
    }

    /// Sample count of `bars` bars after a varispeed of `ratio`.
    ///
    /// **Not** `cut_length / ratio`. The cut length is already rounded, so
    /// dividing it rounds a second time and the result can miss the true value
    /// by a sample. Dividing the exact bar mathematics first keeps it to the one
    /// rounding the docs allow:
    ///
    /// ```text
    /// wrong:  round( round(bars · samplesPerBar) / ratio )
    /// right:  round( bars · samplesPerBar / ratio )
    /// ```
    ///
    /// This is the length the resampler must be asked for. Its effective ratio
    /// is then defined by the two integer lengths rather than by the nominal
    /// speed — an error far below a part per million in pitch, in exchange for
    /// an output that is exactly as long as the new tempo requires.
    pub fn resampled_length(&self, bars: u64, ratio: Ratio) -> u64 {
        let exact = self.exact_sample(bars);
        let scaled = match ratio.as_rational() {
            Some(r) => (exact / r).round_half_up(),
            // No rational to divide by, so the one rounding happens in f64.
            #[allow(clippy::float_arithmetic)]
            None => (exact.to_f64() / ratio.to_f64()).round() as i128,
        };
        debug_assert!(scaled >= 0, "negative length after varispeed");
        scaled.max(0) as u64
    }

    /// The grid a loop lands on after a varispeed of `ratio`.
    ///
    /// The sample rate does not change — this is resampling, not a rate
    /// conversion. What changes is the tempo, and with it every bar line.
    pub fn scaled(&self, ratio: Ratio) -> Result<Grid> {
        Ok(Grid {
            tempo: ratio.resulting_tempo(self.tempo)?,
            ..*self
        })
    }

    /// True when `bars` bars are an exact whole number of samples at this
    /// tempo — the loop then needs no rounding at all.
    pub fn is_sample_exact(&self, bars: u64) -> bool {
        self.exact_sample(bars).is_integer()
    }

    /// Integer BPM values within `window` of the current tempo at which `bars`
    /// bars come out sample-exact. Ascending, so the caller can pick the
    /// nearest below and above.
    ///
    /// At 44.1 kHz over 8 bars of 4/4 the sample count is `84 672 000 / BPM`,
    /// so the exact tempos are the divisors of `2⁹ · 3³ · 5³ · 7²` — in the
    /// 90–130 range: 90, 96, 98, 100, 105, 108, 112, 120, 125, 126, 128.
    pub fn sample_exact_bpms(&self, bars: u64, window: u32) -> Vec<u32> {
        let centre = self.tempo.value().round_half_up();
        let lo = (centre - window as i128).max(1);
        let hi = centre + window as i128;
        (lo..=hi)
            .filter_map(|bpm| {
                let candidate = Grid {
                    tempo: Tempo::new(Rational::from_int(bpm), self.tempo.unit()).ok()?,
                    ..*self
                };
                candidate
                    .is_sample_exact(bars)
                    .then(|| u32::try_from(bpm).ok())
                    .flatten()
            })
            .collect()
    }

    /// Tempo of the same material after a varispeed ratio is applied.
    pub fn scaled_tempo(&self, ratio: Rational) -> Option<Tempo> {
        self.tempo.scaled(ratio).ok()
    }
}

#[cfg(test)]
mod tests {
    // Tests compare against hand-computed decimal values, so they cross into
    // the float domain deliberately.
    #![allow(clippy::float_arithmetic)]

    use super::*;
    use crate::timing::BpmUnit;

    fn grid(bpm: u32, sig: &str, sr: u32) -> Grid {
        Grid::new(
            Tempo::bpm(bpm).unwrap(),
            sig.parse().unwrap(),
            sr,
        )
    }

    /// The reference case from the design docs, worked by hand:
    /// `8·44100·60·4·4 / (103·4·1) = 338688000 / 412 = 822058.25…` → 822058.
    #[test]
    fn reference_case_103bpm() {
        let g = grid(103, "4/4", 44_100);

        assert_eq!(g.samples_per_bar(), Rational::new(44_100 * 60 * 4, 103));
        assert_eq!(g.exact_sample(8), Rational::new(338_688_000, 412));
        assert_eq!(g.cut_sample(8), 822_058);

        // Bar length 2.3301 s, 16 bars ≈ 37.28 s — matches the observed file.
        assert!((g.seconds_per_bar().to_f64() - 2.330_097).abs() < 1e-6);
        assert!((g.seconds_per_bar().to_f64() * 16.0 - 37.281_55).abs() < 1e-4);

        // The desired cut: 18.641 s → 37.282 s.
        let region = g.region(8, 8, Align::Loop);
        assert_eq!(region.start, 822_058);
        assert_eq!(region.len(), 822_058);
        assert!((region.start as f64 / 44_100.0 - 18.641).abs() < 1e-3);
        assert!((region.end as f64 / 44_100.0 - 37.282).abs() < 1e-3);

        // The docs quote ≈0.25 samples ≈ 5.7 µs ≈ 0.3 ppm; exactly it is
        // 26/103 of a sample, rounded down, so the error is negative.
        let res = g.residual(8);
        assert_eq!(res.samples, Rational::new(-26, 103));
        assert!((res.micros() + 5.724).abs() < 1e-3, "{}", res.micros());
        assert!((res.ppm() + 0.307).abs() < 1e-3, "{}", res.ppm());
    }

    #[test]
    fn cut_points_never_accumulate() {
        // A tempo whose bar length is stubbornly non-integer.
        let g = grid(103, "7/8", 44_100);
        for bar in [0u64, 1, 7, 64, 1_000, 100_000, 1_000_000] {
            let exact = g.exact_sample(bar);
            let got = g.cut_sample(bar);
            let err = (Rational::from_int(got as i128) - exact).abs();
            // Bounded by half a sample at any distance — a running sum would
            // be off by hundreds of samples by bar 1 000 000.
            assert!(err <= Rational::new(1, 2), "bar {bar}: error {err}");
        }
    }

    #[test]
    fn golden_cut_points() {
        // Hand-checked with the formula: round(bars · SR · 60 · N·b·q / (p·D·a)).
        let cases: &[(u32, &str, u32, u64, u64)] = &[
            // bpm,  sig,  sample rate, bars, expected sample
            (103, "4/4", 44_100, 8, 822_058), // 84672000/103 = 822058.2524
            (103, "4/4", 48_000, 8, 894_757), // 92160000/103 = 894757.2816
            (120, "4/4", 44_100, 8, 705_600), // exact: 2 s per bar
            (120, "4/4", 48_000, 8, 768_000), // exact
            (128, "4/4", 44_100, 8, 661_500), // exact: 1.875 s per bar
            (100, "3/4", 44_100, 8, 635_040), // exact: 1.8 s per bar
            (103, "3/4", 44_100, 8, 616_544), // 63504000/103 = 616543.6893
            (103, "7/8", 44_100, 8, 719_301), // 74088000/103 = 719300.9709
            (140, "7/8", 44_100, 8, 529_200), // exact
            // 6/8 counted in quarters is three quarters per bar — the same bar
            // length as 3/4. The dotted-quarter count is a separate test.
            (103, "6/8", 44_100, 8, 616_544),
        ];
        for &(bpm, sig, sr, bars, expected) in cases {
            let g = grid(bpm, sig, sr);
            assert_eq!(
                g.cut_sample(bars),
                expected,
                "{bpm} BPM {sig} @ {sr} Hz, {bars} bars — exact value {}",
                g.exact_sample(bars)
            );
        }
    }

    #[test]
    fn bpm_unit_changes_the_bar_length() {
        // At the same BPM number, 6/8 counted in dotted quarters is two beats
        // per bar where the quarter count gives three — so the bar is 2/3 as
        // long. Getting this backwards is exactly the bug the explicit unit
        // exists to prevent.
        let quarters = grid(103, "6/8", 44_100);
        let dotted = Grid {
            tempo: quarters.tempo.with_unit(BpmUnit::dotted_quarter()),
            ..quarters
        };
        assert_eq!(quarters.seconds_per_bar(), Rational::new(180, 103));
        assert_eq!(dotted.seconds_per_bar(), Rational::new(120, 103));
        assert_eq!(
            dotted.seconds_per_bar(),
            quarters.seconds_per_bar() * Rational::new(2, 3)
        );

        // Counted in quarters, one bar of 6/8 is one bar of 3/4.
        assert_eq!(
            quarters.samples_per_bar(),
            grid(103, "3/4", 44_100).samples_per_bar()
        );

        // Two beats per bar under the dotted-quarter unit.
        assert_eq!(
            dotted.samples_per_bar(),
            dotted.samples_per_beat() * Rational::from_int(2)
        );
    }

    #[test]
    fn fractional_bpm_stays_exact() {
        let g = Grid::new(
            "103.5".parse().unwrap(),
            TimeSignature::FOUR_FOUR,
            44_100,
        );
        // 8 · 44100 · 60 · 4 · 4 / (207/2 · 4) = 338688000 / 414 · 2 ... worked
        // through as a rational, not a float.
        assert_eq!(g.exact_sample(8), Rational::new(338_688_000 * 2, 207 * 4));
        assert_eq!(g.cut_sample(8), 818_087);
    }

    #[test]
    fn align_modes_differ_by_at_most_one_sample() {
        let g = grid(103, "7/8", 44_100);
        let l = g.region(8, 8, Align::Loop);
        let r = g.region(8, 8, Align::Grid);
        assert_eq!(l.start, r.start);
        assert!(l.len().abs_diff(r.len()) <= 1, "{} vs {}", l.len(), r.len());

        // Loop priority reproduces the length exactly wherever it starts;
        // grid priority does not have to.
        for skip in [0u64, 3, 8, 17, 4001] {
            let a = g.region(skip, 8, Align::Loop);
            assert_eq!(a.len(), g.exact_sample(8).round_half_up() as u64);
        }
    }

    #[test]
    fn length_residual_is_zero_for_exact_tempos() {
        let g = grid(120, "4/4", 48_000);
        assert!(g.is_sample_exact(8));
        assert!(g.length_residual(8, 8, Align::Loop).is_exact());
        assert!(g.length_residual(8, 8, Align::Grid).is_exact());
        assert_eq!(g.region(8, 8, Align::Loop), g.region(8, 8, Align::Grid));
    }

    #[test]
    fn sample_exact_bpms_matches_the_documented_set() {
        let g = grid(103, "4/4", 44_100);
        assert!(!g.is_sample_exact(8));
        // 84 672 000 / BPM must be a whole number: divisors of 2⁹·3³·5³·7².
        let list = g.sample_exact_bpms(8, 30); // 73..=133
        assert_eq!(
            list,
            vec![75, 80, 84, 90, 96, 98, 100, 105, 108, 112, 120, 125, 126, 128]
        );
        // The 90–130 range quoted in the design docs.
        assert_eq!(
            list.iter().copied().filter(|&b| (90..=130).contains(&b)).collect::<Vec<_>>(),
            vec![90, 96, 98, 100, 105, 108, 112, 120, 125, 126, 128]
        );
        // The advice the CLI will print for 103 BPM.
        assert_eq!(list.iter().rev().find(|&&b| b < 103), Some(&100));
        assert_eq!(list.iter().find(|&&b| b > 103), Some(&105));
    }

    /// The reason `resampled_length` exists rather than a division at the call
    /// site. Verified independently against exact fractions.
    #[test]
    fn dividing_the_rounded_length_can_be_a_sample_wrong() {
        let g = grid(103, "4/4", 44_100);
        let cut = g.region(8, 8, Align::Loop).len(); // 822058, already rounded

        // Half speed. Dividing the exact bar mathematics gives 1644117;
        // dividing the rounded cut length gives 1644116 — one sample short, and
        // a sample short is a loop that drifts.
        let half = Ratio::from_semitones(-12.0).unwrap();
        assert_eq!(g.resampled_length(8, half), 1_644_117);
        assert_eq!(half.output_frames(cut as usize), 1_644_116);

        // Where they agree they must keep agreeing — this is not a licence to
        // differ everywhere.
        for ratio in [
            Ratio::from_semitones(12.0).unwrap(),
            Ratio::from_percent(Rational::new(50, 1)).unwrap(),
            Ratio::from_tempi(Tempo::bpm(103).unwrap(), Tempo::bpm(90).unwrap()).unwrap(),
        ] {
            assert_eq!(
                g.resampled_length(8, ratio),
                ratio.output_frames(cut as usize) as u64,
                "{ratio}"
            );
        }
    }

    #[test]
    fn fitting_to_an_exact_tempo_removes_the_rounding_entirely() {
        // 8 bars of 4/4 at 90 BPM and 44.1 kHz is 940800 samples exactly, so
        // resampling 103 → 90 lands on a whole number with no residual at all.
        let g = grid(103, "4/4", 44_100);
        let to_90 = Ratio::from_tempi(Tempo::bpm(103).unwrap(), Tempo::bpm(90).unwrap()).unwrap();
        assert_eq!(g.resampled_length(8, to_90), 940_800);

        let scaled = g.scaled(to_90).unwrap();
        assert_eq!(scaled.tempo.value(), Rational::from_int(90));
        assert!(scaled.is_sample_exact(8));
        assert_eq!(scaled.exact_sample(8).round_half_up(), 940_800);
        // The sample rate is untouched: this is resampling, not rate conversion.
        assert_eq!(scaled.sample_rate, 44_100);
        assert_eq!(scaled.sig, g.sig);
    }

    #[test]
    fn unity_speed_changes_no_length() {
        let g = grid(103, "7/8", 48_000);
        for bars in [1u64, 4, 8, 16] {
            assert_eq!(
                g.resampled_length(bars, Ratio::UNITY),
                g.exact_sample(bars).round_half_up() as u64
            );
        }
        assert_eq!(g.scaled(Ratio::UNITY).unwrap(), g);
    }

    #[test]
    fn sample_exact_bpms_are_actually_exact() {
        for sr in [44_100u32, 48_000] {
            for sig in ["4/4", "3/4", "7/8"] {
                let g = grid(103, sig, sr);
                for bpm in g.sample_exact_bpms(8, 40) {
                    let exact = grid(bpm, sig, sr);
                    assert!(
                        exact.is_sample_exact(8),
                        "{bpm} BPM {sig} @ {sr} claimed exact but is {}",
                        exact.exact_sample(8)
                    );
                    assert!(exact.length_residual(8, 8, Align::Loop).is_exact());
                }
            }
        }
    }

    #[test]
    fn bar_zero_is_the_origin() {
        let g = grid(103, "4/4", 44_100);
        assert_eq!(g.cut_sample(0), 0);
        assert!(g.residual(0).is_exact());
        assert_eq!(g.residual(0).ppm(), 0.0);
        assert!(g.region(0, 0, Align::Loop).is_empty());
    }
}
