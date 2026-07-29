use std::fmt;
use std::str::FromStr;

use crate::error::{Error, Result};
use crate::rational::Rational;

/// A time signature `N/D` — N beats of a 1/D note per bar.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct TimeSignature {
    pub num: u32,
    pub den: u32,
}

impl TimeSignature {
    pub const FOUR_FOUR: TimeSignature = TimeSignature { num: 4, den: 4 };

    pub fn new(num: u32, den: u32) -> Result<Self> {
        if num == 0 || den == 0 {
            return Err(Error::TimeSignature(format!("{num}/{den}")));
        }
        Ok(TimeSignature { num, den })
    }

    /// Bar length as a fraction of a whole note: `N/D`.
    pub fn whole_notes_per_bar(self) -> Rational {
        Rational::new(self.num as i128, self.den as i128)
    }
}

impl Default for TimeSignature {
    fn default() -> Self {
        Self::FOUR_FOUR
    }
}

impl FromStr for TimeSignature {
    type Err = Error;
    fn from_str(s: &str) -> Result<Self> {
        let err = || Error::TimeSignature(s.to_string());
        let (n, d) = s.trim().split_once('/').ok_or_else(err)?;
        let num = n.trim().parse::<u32>().map_err(|_| err())?;
        let den = d.trim().parse::<u32>().map_err(|_| err())?;
        TimeSignature::new(num, den)
    }
}

impl fmt::Display for TimeSignature {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.num, self.den)
    }
}

/// The note value one BPM beat refers to, as a fraction of a whole note.
///
/// DAW convention is the quarter note (1/4), but 6/8 is commonly counted in
/// dotted quarters (3/8) — two beats per bar rather than six. Carrying this as
/// an explicit parameter is what makes the bar math correct for any meter
/// instead of only for the ones that happen to count in quarters.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct BpmUnit(Rational);

impl BpmUnit {
    /// The default: BPM counts quarter notes.
    pub fn quarter() -> Self {
        BpmUnit(Rational::new(1, 4))
    }

    /// Dotted quarter — the usual count for 6/8, 9/8, 12/8.
    pub fn dotted_quarter() -> Self {
        BpmUnit(Rational::new(3, 8))
    }

    pub fn new(a: u32, b: u32) -> Result<Self> {
        if a == 0 || b == 0 {
            return Err(Error::BpmUnit(format!("{a}/{b}")));
        }
        Ok(BpmUnit(Rational::new(a as i128, b as i128)))
    }

    /// The unit as a fraction of a whole note.
    pub fn whole_notes(self) -> Rational {
        self.0
    }
}

impl Default for BpmUnit {
    fn default() -> Self {
        Self::quarter()
    }
}

impl FromStr for BpmUnit {
    type Err = Error;
    fn from_str(s: &str) -> Result<Self> {
        let err = || Error::BpmUnit(s.to_string());
        let (a, b) = s.trim().split_once('/').ok_or_else(err)?;
        let a = a.trim().parse::<u32>().map_err(|_| err())?;
        let b = b.trim().parse::<u32>().map_err(|_| err())?;
        BpmUnit::new(a, b)
    }
}

impl fmt::Display for BpmUnit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.0.num(), self.0.den())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_signatures() {
        assert_eq!("7/8".parse::<TimeSignature>().unwrap(), TimeSignature { num: 7, den: 8 });
        assert_eq!(" 3 / 4 ".parse::<TimeSignature>().unwrap(), TimeSignature { num: 3, den: 4 });
        assert!("4".parse::<TimeSignature>().is_err());
        assert!("0/4".parse::<TimeSignature>().is_err());
        assert!("4/0".parse::<TimeSignature>().is_err());
        assert!("-4/4".parse::<TimeSignature>().is_err());
        assert!("x/4".parse::<TimeSignature>().is_err());
    }

    #[test]
    fn parses_bpm_units() {
        assert_eq!("1/4".parse::<BpmUnit>().unwrap(), BpmUnit::quarter());
        assert_eq!("3/8".parse::<BpmUnit>().unwrap(), BpmUnit::dotted_quarter());
        // Normalised, so 2/8 and 1/4 are the same unit.
        assert_eq!("2/8".parse::<BpmUnit>().unwrap(), BpmUnit::quarter());
        assert!("1/0".parse::<BpmUnit>().is_err());
    }

    #[test]
    fn bar_length_in_whole_notes() {
        assert_eq!(TimeSignature::FOUR_FOUR.whole_notes_per_bar(), Rational::ONE);
        assert_eq!(
            TimeSignature::new(7, 8).unwrap().whole_notes_per_bar(),
            Rational::new(7, 8)
        );
    }

    #[test]
    fn round_trips_through_display() {
        for s in ["4/4", "7/8", "3/4", "12/8"] {
            assert_eq!(s.parse::<TimeSignature>().unwrap().to_string(), s);
        }
        assert_eq!(BpmUnit::quarter().to_string(), "1/4");
        assert_eq!(BpmUnit::dotted_quarter().to_string(), "3/8");
    }
}
