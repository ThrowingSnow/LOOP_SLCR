//! Exact rational arithmetic on `i128`.
//!
//! Every cut point in this crate derives from these. Floats appear only once
//! the sample domain is reached — never in the timing domain.
//!
//! Overflow panics rather than wrapping, in release builds too. The numbers
//! involved here (sample rates × bar counts) are minuscule against `i128`, so
//! the checks cost nothing measurable and a silent wrap would corrupt a cut
//! point without any way to notice.

use std::fmt;
use std::ops::{Add, Div, Mul, Neg, Sub};

/// An exact rational number. Always normalised: `den > 0` and `gcd(num, den) == 1`.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct Rational {
    num: i128,
    den: i128,
}

impl Rational {
    pub const ZERO: Rational = Rational { num: 0, den: 1 };
    pub const ONE: Rational = Rational { num: 1, den: 1 };

    /// # Panics
    /// If `den == 0`.
    pub fn new(num: i128, den: i128) -> Self {
        assert!(den != 0, "rational with zero denominator");
        let (num, den) = if den < 0 {
            (
                num.checked_neg().expect("rational overflow: negate numerator"),
                den.checked_neg().expect("rational overflow: negate denominator"),
            )
        } else {
            (num, den)
        };
        let g = gcd(num.unsigned_abs(), den.unsigned_abs()) as i128;
        Rational {
            num: num / g,
            den: den / g,
        }
    }

    pub const fn from_int(n: i128) -> Self {
        Rational { num: n, den: 1 }
    }

    pub const fn num(self) -> i128 {
        self.num
    }

    pub const fn den(self) -> i128 {
        self.den
    }

    pub const fn is_integer(self) -> bool {
        self.den == 1
    }

    pub const fn is_zero(self) -> bool {
        self.num == 0
    }

    pub const fn is_negative(self) -> bool {
        self.num < 0
    }

    /// Multiplicative inverse.
    ///
    /// # Panics
    /// If `self` is zero.
    pub fn recip(self) -> Self {
        assert!(self.num != 0, "reciprocal of zero");
        Rational::new(self.den, self.num)
    }

    pub fn abs(self) -> Self {
        Rational {
            num: self.num.checked_abs().expect("rational overflow: abs"),
            den: self.den,
        }
    }

    /// Rounds to the nearest integer, ties going toward positive infinity.
    ///
    /// Half-up rather than half-to-even so that a cut point is a pure function
    /// of the tempo, never of which side of the grid it happens to land on.
    pub fn round_half_up(self) -> i128 {
        let q = self.num.div_euclid(self.den);
        let r = self.num.rem_euclid(self.den); // 0 <= r < den
        // `r >= den - r` rather than `2 * r >= den`: both sides are
        // non-negative and bounded by `den`, so neither can overflow.
        if r >= self.den - r {
            q.checked_add(1).expect("rational overflow: round_half_up")
        } else {
            q
        }
    }

    /// Largest integer `<= self`.
    pub fn floor(self) -> i128 {
        self.num.div_euclid(self.den)
    }

    /// Smallest integer `>= self`.
    pub fn ceil(self) -> i128 {
        let q = self.num.div_euclid(self.den);
        if self.num.rem_euclid(self.den) == 0 {
            q
        } else {
            q.checked_add(1).expect("rational overflow: ceil")
        }
    }

    /// Lossy — for display and for handing a ratio to the sample domain only.
    ///
    /// This is the one door out of exact arithmetic. Nothing that feeds a cut
    /// point may pass through it.
    #[allow(clippy::float_arithmetic)]
    pub fn to_f64(self) -> f64 {
        self.num as f64 / self.den as f64
    }
}

fn gcd(mut a: u128, mut b: u128) -> u128 {
    while b != 0 {
        let t = a % b;
        a = b;
        b = t;
    }
    if a == 0 {
        1
    } else {
        a
    }
}

/// Cross-multiplying reduction before the product, so intermediate values stay
/// small: `(a/b) * (c/d)` reduces `a` against `d` and `c` against `b` first.
fn mul_reduced(a: Rational, b: Rational) -> Rational {
    let g1 = gcd(a.num.unsigned_abs(), b.den.unsigned_abs()) as i128;
    let g2 = gcd(b.num.unsigned_abs(), a.den.unsigned_abs()) as i128;
    let num = (a.num / g1)
        .checked_mul(b.num / g2)
        .expect("rational overflow: multiply numerators");
    let den = (a.den / g2)
        .checked_mul(b.den / g1)
        .expect("rational overflow: multiply denominators");
    Rational { num, den }
}

impl Add for Rational {
    type Output = Rational;
    fn add(self, rhs: Rational) -> Rational {
        let g = gcd(self.den.unsigned_abs(), rhs.den.unsigned_abs()) as i128;
        let lcm = (self.den / g)
            .checked_mul(rhs.den)
            .expect("rational overflow: add denominators");
        let num = (self.num)
            .checked_mul(lcm / self.den)
            .and_then(|l| {
                rhs.num
                    .checked_mul(lcm / rhs.den)
                    .and_then(|r| l.checked_add(r))
            })
            .expect("rational overflow: add numerators");
        Rational::new(num, lcm)
    }
}

impl Sub for Rational {
    type Output = Rational;
    fn sub(self, rhs: Rational) -> Rational {
        self + (-rhs)
    }
}

impl Neg for Rational {
    type Output = Rational;
    fn neg(self) -> Rational {
        Rational {
            num: self.num.checked_neg().expect("rational overflow: negate"),
            den: self.den,
        }
    }
}

impl Mul for Rational {
    type Output = Rational;
    fn mul(self, rhs: Rational) -> Rational {
        mul_reduced(self, rhs)
    }
}

impl Div for Rational {
    type Output = Rational;
    fn div(self, rhs: Rational) -> Rational {
        mul_reduced(self, rhs.recip())
    }
}

impl From<i128> for Rational {
    fn from(n: i128) -> Self {
        Rational::from_int(n)
    }
}

impl From<u32> for Rational {
    fn from(n: u32) -> Self {
        Rational::from_int(n as i128)
    }
}

impl From<u64> for Rational {
    fn from(n: u64) -> Self {
        Rational::from_int(n as i128)
    }
}

impl PartialOrd for Rational {
    fn partial_cmp(&self, other: &Rational) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Rational {
    fn cmp(&self, other: &Rational) -> std::cmp::Ordering {
        // Both denominators are positive, so cross-multiplication preserves order.
        let l = self
            .num
            .checked_mul(other.den)
            .expect("rational overflow: compare");
        let r = other
            .num
            .checked_mul(self.den)
            .expect("rational overflow: compare");
        l.cmp(&r)
    }
}

impl fmt::Display for Rational {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.den == 1 {
            write!(f, "{}", self.num)
        } else {
            write!(f, "{}/{}", self.num, self.den)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(n: i128, d: i128) -> Rational {
        Rational::new(n, d)
    }

    #[test]
    fn normalises_on_construction() {
        assert_eq!(r(2, 4), r(1, 2));
        assert_eq!(r(-6, 8), r(-3, 4));
        assert_eq!(r(0, 5), Rational::ZERO);
        // Sign always migrates to the numerator.
        let x = r(1, -2);
        assert_eq!(x.num(), -1);
        assert_eq!(x.den(), 2);
    }

    #[test]
    fn arithmetic_is_exact() {
        assert_eq!(r(1, 3) + r(1, 6), r(1, 2));
        assert_eq!(r(1, 3) - r(1, 3), Rational::ZERO);
        assert_eq!(r(2, 3) * r(3, 4), r(1, 2));
        assert_eq!(r(2, 3) / r(4, 9), r(3, 2));
        // The classic float failure, exact here.
        let tenth = r(1, 10);
        let sum = (0..10).fold(Rational::ZERO, |a, _| a + tenth);
        assert_eq!(sum, Rational::ONE);
    }

    #[test]
    fn round_half_up_ties_go_upward() {
        assert_eq!(r(1, 2).round_half_up(), 1);
        assert_eq!(r(3, 2).round_half_up(), 2);
        assert_eq!(r(5, 2).round_half_up(), 3); // not half-to-even
        assert_eq!(r(-1, 2).round_half_up(), 0); // toward +inf
        assert_eq!(r(-3, 2).round_half_up(), -1);
        assert_eq!(r(1, 3).round_half_up(), 0);
        assert_eq!(r(2, 3).round_half_up(), 1);
        assert_eq!(r(-2, 3).round_half_up(), -1);
        assert_eq!(r(7, 1).round_half_up(), 7);
    }

    #[test]
    fn floor_and_ceil() {
        assert_eq!(r(7, 2).floor(), 3);
        assert_eq!(r(7, 2).ceil(), 4);
        assert_eq!(r(-7, 2).floor(), -4);
        assert_eq!(r(-7, 2).ceil(), -3);
        assert_eq!(r(4, 2).floor(), 2);
        assert_eq!(r(4, 2).ceil(), 2);
    }

    #[test]
    fn ordering() {
        assert!(r(1, 3) < r(1, 2));
        assert!(r(-1, 3) < r(1, 300));
        assert!(r(103, 1) > r(1029, 10));
        let mut v = vec![r(1, 2), r(1, 3), r(2, 3)];
        v.sort();
        assert_eq!(v, vec![r(1, 3), r(1, 2), r(2, 3)]);
    }

    #[test]
    fn associativity_over_a_sweep() {
        for a in 1..12i128 {
            for b in 1..12i128 {
                for c in 1..12i128 {
                    let (x, y, z) = (r(a, b), r(b, c), r(c, a));
                    assert_eq!((x + y) + z, x + (y + z));
                    assert_eq!((x * y) * z, x * (y * z));
                    assert_eq!(x * (y + z), x * y + x * z);
                    assert_eq!((x / y) * y, x);
                }
            }
        }
    }

    #[test]
    fn survives_large_magnitudes() {
        // The shape of a real cut-point computation, several orders of
        // magnitude beyond anything the tool will actually see.
        let big = r(i64::MAX as i128, 3);
        let back = (big * r(7, 11)) / r(7, 11);
        assert_eq!(back, big);
    }

    #[test]
    #[should_panic(expected = "zero denominator")]
    fn zero_denominator_panics() {
        let _ = r(1, 0);
    }

    #[test]
    #[should_panic(expected = "reciprocal of zero")]
    fn reciprocal_of_zero_panics() {
        let _ = Rational::ZERO.recip();
    }

    #[test]
    fn display() {
        assert_eq!(r(3, 4).to_string(), "3/4");
        assert_eq!(r(8, 4).to_string(), "2");
        assert_eq!(r(-1, 2).to_string(), "-1/2");
    }
}
