//! `java.math.BigDecimal`: an integer and a scale, with Java's text form. `money` is saved as this
//! text, so a balance must read back exactly as Java wrote it.
use num_bigint::{BigInt, Sign};
use num_integer::Integer;
use num_traits::{One, Signed, ToPrimitive, Zero};
use std::cmp::Ordering;
use std::str::FromStr;

/// `MathContext.DECIMAL128`'s digits.
const PRECISION: usize = 34;

#[derive(Clone, Debug)]
pub struct BigDecimal {
    unscaled: BigInt,
    scale: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rounding {
    Floor,
    Down,
    HalfUp,
    HalfEven,
}

fn pow10(power: u32) -> BigInt {
    num_traits::pow(BigInt::from(10), power as usize)
}

impl Default for BigDecimal {
    fn default() -> Self {
        Self::zero()
    }
}

impl BigDecimal {
    pub fn zero() -> Self {
        Self { unscaled: BigInt::zero(), scale: 0 }
    }
    pub fn one() -> Self {
        Self { unscaled: BigInt::one(), scale: 0 }
    }
    pub fn from_i64(value: i64) -> Self {
        Self { unscaled: BigInt::from(value), scale: 0 }
    }
    /// `BigDecimal.valueOf(double)`: through `Double.toString`.
    pub fn from_f64(value: f64) -> Self {
        use horizon_bukkit::JavaString;
        Self::parse(&value.java_string()).unwrap_or_else(Self::zero)
    }
    /// `new BigDecimal(String)`.
    pub fn parse(text: &str) -> Option<Self> {
        let (mantissa, exponent) = match text.find(['e', 'E']) {
            Some(at) => (&text[..at], text[at + 1..].strip_prefix('+').unwrap_or(&text[at + 1..]).parse::<i32>().ok()?),
            None => (text, 0),
        };
        let (sign, digits) = match mantissa.as_bytes().first()? {
            b'-' => (true, &mantissa[1..]),
            b'+' => (false, &mantissa[1..]),
            _ => (false, mantissa),
        };
        let (whole, fraction) = digits.split_once('.').unwrap_or((digits, ""));
        if whole.is_empty() && fraction.is_empty() || !whole.bytes().chain(fraction.bytes()).all(|b| b.is_ascii_digit()) {
            return None;
        }
        let joined = format!("{whole}{fraction}");
        let mut unscaled = BigInt::from_str(&joined).ok()?;
        if sign {
            unscaled = -unscaled;
        }
        Some(Self { unscaled, scale: i32::try_from(fraction.len()).ok()?.checked_sub(exponent)? })
    }
    /// `new BigDecimal(String, MathContext.DECIMAL128)`.
    pub fn parse_128(text: &str) -> Option<Self> {
        Self::parse(text).map(|value| value.round_128())
    }
    fn precision(&self) -> usize {
        if self.unscaled.is_zero() { 1 } else { self.unscaled.magnitude().to_string().len() }
    }
    fn round_128(self) -> Self {
        let precision = self.precision();
        if precision <= PRECISION {
            return self;
        }
        let drop = (precision - PRECISION) as i32;
        self.set_scale(self.scale - drop, Rounding::HalfEven)
    }
    pub fn signum(&self) -> i32 {
        match self.unscaled.sign() {
            Sign::Minus => -1,
            Sign::NoSign => 0,
            Sign::Plus => 1,
        }
    }
    pub fn scale(&self) -> i32 {
        self.scale
    }
    pub fn negate(&self) -> Self {
        Self { unscaled: -self.unscaled.clone(), scale: self.scale }
    }
    pub fn abs(&self) -> Self {
        Self { unscaled: self.unscaled.abs(), scale: self.scale }
    }
    fn at_scale(&self, scale: i32) -> BigInt {
        &self.unscaled * pow10((scale - self.scale) as u32)
    }
    pub fn add(&self, other: &Self) -> Self {
        let scale = self.scale.max(other.scale);
        Self { unscaled: self.at_scale(scale) + other.at_scale(scale), scale }
    }
    pub fn subtract(&self, other: &Self) -> Self {
        self.add(&other.negate())
    }
    pub fn multiply(&self, other: &Self) -> Self {
        Self { unscaled: &self.unscaled * &other.unscaled, scale: self.scale + other.scale }
    }
    /// `remainder`.
    pub fn remainder(&self, other: &Self) -> Self {
        let scale = self.scale.max(other.scale);
        Self { unscaled: self.at_scale(scale) % other.at_scale(scale), scale }
    }
    /// `scaleByPowerOfTen`.
    pub fn scale_by_power_of_ten(&self, power: i32) -> Self {
        Self { unscaled: self.unscaled.clone(), scale: self.scale - power }
    }
    /// `divide(other, MathContext.DECIMAL128)`: `None` for a zero divisor, Java's `ArithmeticException`.
    pub fn divide_128(&self, other: &Self) -> Option<Self> {
        if other.unscaled.is_zero() {
            return None;
        }
        let preferred = self.scale - other.scale;
        if self.unscaled.is_zero() {
            return Some(Self { unscaled: BigInt::zero(), scale: preferred });
        }
        let scale = preferred + PRECISION as i32 + 3 + other.precision() as i32;
        let shift = scale - self.scale + other.scale;
        let (quotient, remainder): (BigInt, BigInt) = if shift >= 0 {
            let scaled: BigInt = &self.unscaled * pow10(shift as u32);
            Integer::div_rem(&scaled, &other.unscaled)
        } else {
            let scaled: BigInt = &other.unscaled * pow10((-shift) as u32);
            Integer::div_rem(&self.unscaled, &scaled)
        };
        // A sticky digit, so a rounded tie can't be mistaken for an exact one.
        let sticky: BigInt = if remainder.is_zero() { BigInt::zero() } else { BigInt::from(quotient.sign_of()) };
        let unscaled: BigInt = quotient * BigInt::from(10) + sticky;
        let mut result = Self { unscaled, scale: scale + 1 }.round_128();
        // Java strips trailing zeros down to the preferred scale.
        while result.scale > preferred && (&result.unscaled % BigInt::from(10)).is_zero() {
            result.unscaled /= BigInt::from(10);
            result.scale -= 1;
        }
        Some(result)
    }
    /// `setScale(scale, rounding)`.
    pub fn set_scale(&self, scale: i32, rounding: Rounding) -> Self {
        if scale >= self.scale {
            return Self { unscaled: self.at_scale(scale), scale };
        }
        let divisor = pow10((self.scale - scale) as u32);
        let (quotient, remainder): (BigInt, BigInt) = Integer::div_rem(&self.unscaled, &divisor);
        let negative = self.unscaled.is_negative();
        let twice: BigInt = remainder.abs() * BigInt::from(2);
        let away = !remainder.is_zero()
            && match rounding {
                Rounding::Floor => negative,
                Rounding::Down => false,
                Rounding::HalfUp => twice >= divisor,
                Rounding::HalfEven => match twice.cmp(&divisor) {
                    Ordering::Greater => true,
                    Ordering::Equal => quotient.is_odd(),
                    Ordering::Less => false,
                },
            };
        let step = if negative { BigInt::from(-1) } else { BigInt::one() };
        Self { unscaled: if away { quotient + step } else { quotient }, scale }
    }
    pub fn to_f64(&self) -> f64 {
        self.to_plain_string().parse().unwrap_or(0.0)
    }
    pub fn to_i64(&self) -> i64 {
        self.set_scale(0, Rounding::Down).unscaled.to_i64().unwrap_or(0)
    }
    /// `equals`: 1.0 and 1.00 differ, as they do in Java.
    pub fn equals_exact(&self, other: &Self) -> bool {
        self.scale == other.scale && self.unscaled == other.unscaled
    }
    /// `toPlainString`.
    pub fn to_plain_string(&self) -> String {
        let digits = self.unscaled.magnitude().to_string();
        let sign = if self.unscaled.is_negative() { "-" } else { "" };
        if self.scale <= 0 {
            let zeros = if self.unscaled.is_zero() { 0 } else { (-self.scale) as usize };
            return format!("{sign}{digits}{}", "0".repeat(zeros));
        }
        let scale = self.scale as usize;
        if digits.len() > scale {
            let (whole, fraction) = digits.split_at(digits.len() - scale);
            format!("{sign}{whole}.{fraction}")
        } else {
            format!("{sign}0.{}{digits}", "0".repeat(scale - digits.len()))
        }
    }
}

/// `compareTo`: by value, whatever the scale. `equals_exact` is Java's `equals`.
impl PartialEq for BigDecimal {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl Eq for BigDecimal {}

impl PartialOrd for BigDecimal {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for BigDecimal {
    fn cmp(&self, other: &Self) -> Ordering {
        let scale = self.scale.max(other.scale);
        self.at_scale(scale).cmp(&other.at_scale(scale))
    }
}

/// `toString`.
impl std::fmt::Display for BigDecimal {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let digits = self.unscaled.magnitude().to_string();
        let adjusted = i64::from(-self.scale) + digits.len() as i64 - 1;
        if self.scale == 0 || (self.scale > 0 && adjusted >= -6) {
            return formatter.write_str(&self.to_plain_string());
        }
        let sign = if self.unscaled.is_negative() { "-" } else { "" };
        let (first, rest) = digits.split_at(1);
        let dot = if rest.is_empty() { "" } else { "." };
        let exponent = if adjusted >= 0 { format!("+{adjusted}") } else { adjusted.to_string() };
        write!(formatter, "{sign}{first}{dot}{rest}E{exponent}")
    }
}

trait SignOf {
    fn sign_of(&self) -> i32;
}

impl SignOf for BigInt {
    fn sign_of(&self) -> i32 {
        if self.is_negative() { -1 } else { 1 }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(text: &str) -> BigDecimal {
        BigDecimal::parse(text).unwrap()
    }

    #[test]
    fn text_is_javas() {
        assert_eq!(d("100.50").to_string(), "100.50");
        assert_eq!(d("0.000001").to_string(), "0.000001");
        assert_eq!(d("0.0000001").to_string(), "1E-7");
        assert_eq!(d("1E+3").to_string(), "1E+3");
        assert_eq!(d("1E+3").to_plain_string(), "1000");
        assert_eq!(d("-0.5").to_plain_string(), "-0.5");
        assert_eq!(BigDecimal::from_f64(100.0).to_string(), "100.0");
    }

    #[test]
    fn sums_keep_scale() {
        assert_eq!(d("1.10").add(&d("2.2")).to_string(), "3.30");
        assert_eq!(d("1.5").multiply(&d("2")).to_string(), "3.0");
    }

    #[test]
    fn compare_ignores_scale_and_equals_does_not() {
        assert_eq!(d("1.0"), d("1.00"));
        assert!(!d("1.0").equals_exact(&d("1.00")));
    }

    #[test]
    fn division_is_decimal128() {
        assert_eq!(d("1").divide_128(&d("3")).unwrap().to_string(), "0.3333333333333333333333333333333333");
        assert_eq!(d("10").divide_128(&d("4")).unwrap().to_string(), "2.5");
        assert_eq!(d("6").divide_128(&d("2")).unwrap().to_string(), "3");
        assert!(d("1").divide_128(&d("0")).is_none());
    }

    #[test]
    fn rounding_modes() {
        assert_eq!(d("-1.001").set_scale(2, Rounding::Floor).to_string(), "-1.01");
        assert_eq!(d("1.009").set_scale(2, Rounding::Floor).to_string(), "1.00");
        assert_eq!(d("1.005").set_scale(2, Rounding::HalfUp).to_string(), "1.01");
        assert_eq!(d("1.005").set_scale(2, Rounding::HalfEven).to_string(), "1.00");
    }
}
