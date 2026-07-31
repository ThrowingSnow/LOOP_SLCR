//! Note values, and how long each one is.
//!
//! The arithmetic behind the calculator screen: what a sixteenth is in
//! milliseconds, in samples, in hertz, and as a share of the bar. Musicians
//! reach for this to set a delay time, and a delay set from a rounded number is
//! a delay that drifts out of the grid over a long loop.
//!
//! # Exact, like everything else in this module tree
//!
//! A note length is a rational number of samples, computed the same way a bar
//! length is:
//!
//! ```text
//! seconds(w) = (60 / BPM) · w / U
//! ```
//!
//! where `w` is the note as a fraction of a whole note and `U` is the BPM unit.
//! Dots multiply `w` by 3/2 and triplets by 2/3, both exact. Nothing here rounds
//! until something has to be displayed, which is the same rule the cut points
//! follow — and the reason the numbers on this screen agree with the ones the
//! cutter uses rather than merely resembling them.
//!
//! # Why the BPM unit matters here
//!
//! In 6/8 counted in dotted quarters, "120 BPM" means 120 dotted quarters a
//! minute, so a sixteenth is not what it would be at 120 quarter notes. A
//! calculator that assumed quarters would be wrong for exactly the material
//! this tool is aimed at.

use crate::rational::Rational;
use crate::timing::Grid;

/// Straight, dotted, or triplet.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
pub enum Flavour {
    #[default]
    Straight,
    /// Half again as long: `3/2`.
    Dotted,
    /// Two thirds as long: three in the space of two.
    Triplet,
}

impl Flavour {
    /// The factor applied to the plain note length.
    pub fn factor(self) -> Rational {
        match self {
            Flavour::Straight => Rational::from_int(1),
            Flavour::Dotted => Rational::new(3, 2),
            Flavour::Triplet => Rational::new(2, 3),
        }
    }

    pub fn suffix(self) -> &'static str {
        match self {
            Flavour::Straight => "",
            Flavour::Dotted => ".",
            Flavour::Triplet => "T",
        }
    }
}

/// One note value: a denominator and a flavour. `1/8.` is a dotted eighth.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct NoteValue {
    /// The denominator: 4 for a quarter, 16 for a sixteenth.
    pub denominator: u32,
    pub flavour: Flavour,
}

impl NoteValue {
    pub fn new(denominator: u32, flavour: Flavour) -> Self {
        NoteValue {
            denominator,
            flavour,
        }
    }

    /// The value as a fraction of a whole note.
    pub fn whole_notes(self) -> Rational {
        Rational::new(1, self.denominator.max(1) as i128) * self.flavour.factor()
    }

    /// `1/4`, `1/8.`, `1/16T`.
    pub fn label(self) -> String {
        format!("1/{}{}", self.denominator, self.flavour.suffix())
    }
}

/// A note value measured against a particular grid.
#[derive(Copy, Clone, Debug)]
pub struct NoteLength {
    pub value: NoteValue,
    /// Exact, generally not an integer.
    pub samples: Rational,
    pub seconds: Rational,
    /// The share of one bar this note occupies, as a fraction.
    pub per_bar: Rational,
}

impl NoteLength {
    /// The rate at which this note repeats, in hertz.
    ///
    /// Display only, like [`Residual::micros`](crate::timing::Residual::micros):
    /// a frequency is read off a dial, not used to place a sample.
    #[allow(clippy::float_arithmetic)]
    pub fn hertz(&self) -> f64 {
        let seconds = self.seconds.to_f64();
        if seconds > 0.0 {
            1.0 / seconds
        } else {
            f64::INFINITY
        }
    }

    /// Milliseconds. Display only, for the same reason.
    #[allow(clippy::float_arithmetic)]
    pub fn millis(&self) -> f64 {
        self.seconds.to_f64() * 1000.0
    }

    /// Whether this note lands on a whole number of samples.
    ///
    /// The one fact on the screen that is not decoration: a delay time that is
    /// sample-exact stays locked to the grid however long the loop runs, and one
    /// that is not accumulates error at the rate shown.
    pub fn is_sample_exact(&self) -> bool {
        self.samples.den() == 1
    }
}

/// The length of one note value on `grid`.
pub fn length(grid: &Grid, value: NoteValue) -> NoteLength {
    let seconds = grid.seconds_per_whole_notes(value.whole_notes());
    NoteLength {
        value,
        samples: seconds * Rational::from(grid.sample_rate),
        seconds,
        per_bar: seconds / grid.seconds_per_bar(),
    }
}

/// The denominators the table covers: whole note down to a thirty-second.
pub const DENOMINATORS: &[u32] = &[1, 2, 4, 8, 16, 32];

/// The whole table, straight then dotted then triplet.
///
/// Ordered by flavour rather than interleaved, because the three are read as
/// three columns of a chart and not as eighteen unrelated rows.
pub fn table(grid: &Grid) -> Vec<NoteLength> {
    let mut out = Vec::with_capacity(DENOMINATORS.len() * 3);
    for flavour in [Flavour::Straight, Flavour::Dotted, Flavour::Triplet] {
        for &denominator in DENOMINATORS {
            out.push(length(grid, NoteValue::new(denominator, flavour)));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::timing::{BpmUnit, Tempo, TimeSignature};

    fn grid(bpm: u32, rate: u32) -> Grid {
        Grid::new(
            Tempo::bpm(bpm).expect("tempo"),
            TimeSignature::default(),
            rate,
        )
    }

    #[test]
    fn a_quarter_at_120_is_half_a_second() {
        let g = grid(120, 48_000);
        let quarter = length(&g, NoteValue::new(4, Flavour::Straight));
        assert_eq!(quarter.seconds, Rational::new(1, 2));
        assert_eq!(quarter.samples, Rational::from_int(24_000));
        assert!(quarter.is_sample_exact());
        assert!((quarter.hertz() - 2.0).abs() < 1e-12);
        assert!((quarter.millis() - 500.0).abs() < 1e-9);
    }

    #[test]
    fn a_dotted_eighth_is_three_quarters_of_a_quarter() {
        let g = grid(120, 48_000);
        let dotted = length(&g, NoteValue::new(8, Flavour::Dotted));
        // 1/8 · 3/2 = 3/16 of a whole note = 0.75 of a quarter = 0.375 s.
        assert_eq!(dotted.seconds, Rational::new(3, 8));
        assert_eq!(dotted.samples, Rational::from_int(18_000));
    }

    #[test]
    fn a_quarter_triplet_is_a_third_of_a_half_note() {
        let g = grid(120, 48_000);
        let triplet = length(&g, NoteValue::new(4, Flavour::Triplet));
        assert_eq!(triplet.seconds, Rational::new(1, 3));
        // Three of them fill exactly one half note, with nothing left over.
        assert_eq!(triplet.seconds * Rational::from_int(3), Rational::from_int(1));
    }

    #[test]
    fn the_share_of_a_bar_is_a_fraction_not_a_rounding() {
        let g = grid(103, 44_100);
        let sixteenth = length(&g, NoteValue::new(16, Flavour::Straight));
        // Four four with quarter-note BPM: a bar is four quarters, so a
        // sixteenth is exactly one sixteenth of it, whatever the tempo.
        assert_eq!(sixteenth.per_bar, Rational::new(1, 16));
    }

    #[test]
    fn a_tempo_that_does_not_divide_is_not_sample_exact() {
        let g = grid(103, 44_100);
        let quarter = length(&g, NoteValue::new(4, Flavour::Straight));
        // 60 · 44100 / 103 is not an integer, and saying so is the point of the
        // flag: a delay set to this drifts.
        assert!(!quarter.is_sample_exact());
        assert_eq!(quarter.samples, Rational::new(60 * 44_100, 103));
    }

    #[test]
    fn the_bpm_unit_changes_every_row() {
        // 120 dotted quarters a minute is not 120 quarters a minute, and a
        // calculator that ignored the unit would be wrong for exactly the
        // material this tool is aimed at.
        let quarters = Grid::new(
            Tempo::bpm(120).expect("tempo").with_unit(BpmUnit::quarter()),
            TimeSignature::new(6, 8).expect("signature"),
            48_000,
        );
        let dotted = Grid::new(
            Tempo::bpm(120)
                .expect("tempo")
                .with_unit(BpmUnit::dotted_quarter()),
            TimeSignature::new(6, 8).expect("signature"),
            48_000,
        );
        let a = length(&quarters, NoteValue::new(4, Flavour::Straight)).seconds;
        let b = length(&dotted, NoteValue::new(4, Flavour::Straight)).seconds;
        // A dotted-quarter count makes every note two thirds as long.
        assert_eq!(b, a * Rational::new(2, 3));
    }

    #[test]
    fn the_table_covers_every_value_once() {
        let g = grid(120, 48_000);
        let rows = table(&g);
        assert_eq!(rows.len(), DENOMINATORS.len() * 3);

        let mut labels: Vec<String> = rows.iter().map(|r| r.value.label()).collect();
        let count = labels.len();
        labels.sort();
        labels.dedup();
        assert_eq!(labels.len(), count, "a value appears twice");
    }

    #[test]
    fn halving_the_denominator_doubles_the_length() {
        let g = grid(97, 44_100);
        for flavour in [Flavour::Straight, Flavour::Dotted, Flavour::Triplet] {
            let eighth = length(&g, NoteValue::new(8, flavour)).samples;
            let quarter = length(&g, NoteValue::new(4, flavour)).samples;
            assert_eq!(quarter, eighth * Rational::from_int(2), "{flavour:?}");
        }
    }
}
