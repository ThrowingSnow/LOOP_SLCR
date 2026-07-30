use std::fmt;
use std::str::FromStr;

use crate::error::{Error, Result};
use crate::rational::Rational;

use super::signature::BpmUnit;

/// A tempo: an exact BPM value plus the note value it counts.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Tempo {
    bpm: Rational,
    unit: BpmUnit,
}

impl Tempo {
    /// # Errors
    /// If `bpm` is not strictly positive.
    pub fn new(bpm: Rational, unit: BpmUnit) -> Result<Self> {
        if bpm.is_zero() || bpm.is_negative() {
            return Err(Error::Tempo(bpm.to_string()));
        }
        Ok(Tempo { bpm, unit })
    }

    /// Integer BPM against the default quarter-note unit — the common case.
    /// The tempo an `acid` chunk declares.
    ///
    /// Via the decimal spelling rather than `f32 as f64`: a chunk saying 103.5
    /// should become the exact fraction 207/2, not the binary approximation of
    /// it, because that fraction is what makes the loop length exact.
    pub fn from_f32(tempo: f32) -> Option<Self> {
        if !tempo.is_finite() || tempo <= 0.0 {
            return None;
        }
        format!("{tempo}").parse().ok()
    }

    pub fn bpm(bpm: u32) -> Result<Self> {
        Tempo::new(Rational::from_int(bpm as i128), BpmUnit::quarter())
    }

    pub fn value(self) -> Rational {
        self.bpm
    }

    pub fn unit(self) -> BpmUnit {
        self.unit
    }

    pub fn with_unit(self, unit: BpmUnit) -> Self {
        Tempo { unit, ..self }
    }

    /// Scales the tempo by a varispeed ratio, exactly.
    pub fn scaled(self, ratio: Rational) -> Result<Self> {
        Tempo::new(self.bpm * ratio, self.unit)
    }
}

impl FromStr for Tempo {
    type Err = Error;

    /// Accepts `103`, `103.5`, or an explicit fraction `207/2`.
    fn from_str(s: &str) -> Result<Self> {
        let s = s.trim();
        let err = || Error::Tempo(s.to_string());

        let value = if let Some((n, d)) = s.split_once('/') {
            let n = n.trim().parse::<i128>().map_err(|_| err())?;
            let d = d.trim().parse::<i128>().map_err(|_| err())?;
            if d == 0 {
                return Err(err());
            }
            Rational::new(n, d)
        } else if let Some((int, frac)) = s.split_once('.') {
            // Decimal input is exact too: 103.5 is 1035/10, never 103.49999…
            let int: i128 = if int.is_empty() {
                0
            } else {
                int.parse().map_err(|_| err())?
            };
            if frac.is_empty() || !frac.bytes().all(|b| b.is_ascii_digit()) {
                return Err(err());
            }
            let scale = 10i128.checked_pow(frac.len() as u32).ok_or_else(err)?;
            let frac: i128 = frac.parse().map_err(|_| err())?;
            let num = int.checked_mul(scale).and_then(|i| i.checked_add(frac)).ok_or_else(err)?;
            Rational::new(num, scale)
        } else {
            Rational::from_int(s.parse::<i128>().map_err(|_| err())?)
        };

        Tempo::new(value, BpmUnit::quarter())
    }
}

impl fmt::Display for Tempo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.bpm.is_integer() {
            write!(f, "{} BPM", self.bpm.num())?;
        } else {
            write!(f, "{} ({}) BPM", self.bpm.to_f64(), self.bpm)?;
        }
        if self.unit != BpmUnit::quarter() {
            write!(f, " per {}", self.unit)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_integer_and_decimal_bpm() {
        assert_eq!("103".parse::<Tempo>().unwrap().value(), Rational::from_int(103));
        assert_eq!("103.5".parse::<Tempo>().unwrap().value(), Rational::new(207, 2));
        assert_eq!("103.500".parse::<Tempo>().unwrap().value(), Rational::new(207, 2));
        assert_eq!("207/2".parse::<Tempo>().unwrap().value(), Rational::new(207, 2));
        // A tempo no float represents exactly, held exactly here.
        assert_eq!("0.1".parse::<Tempo>().unwrap().value(), Rational::new(1, 10));
    }

    #[test]
    fn rejects_nonsense() {
        for s in ["0", "-103", "abc", "103.", "103.x", "1/0", ""] {
            assert!(s.parse::<Tempo>().is_err(), "{s:?} should not parse");
        }
    }

    #[test]
    fn scaling_is_exact() {
        let t = Tempo::bpm(103).unwrap();
        let half = t.scaled(Rational::new(1, 2)).unwrap();
        assert_eq!(half.value(), Rational::new(103, 2));
        assert_eq!(half.scaled(Rational::from_int(2)).unwrap(), t);
    }

    #[test]
    fn display() {
        assert_eq!(Tempo::bpm(103).unwrap().to_string(), "103 BPM");
        assert_eq!(
            Tempo::bpm(103).unwrap().with_unit(BpmUnit::dotted_quarter()).to_string(),
            "103 BPM per 3/8"
        );
        assert_eq!("103.5".parse::<Tempo>().unwrap().to_string(), "103.5 (207/2) BPM");
    }
}
