//! `java.math`: `BigDecimal` and `BigInteger` as Java's, exact (money in economy plugins), with Java's
//! scales, rounding modes, `toString` and exceptions; `MathContext` and `RoundingMode`.
use crate::{R, V, exc, native, s};
use bigdecimal::{BigDecimal, RoundingMode};
use num_bigint::{BigInt, Sign};
use num_traits::{One, Signed, ToPrimitive, Zero};
use std::str::FromStr;

/// `BigDecimal`: an unscaled integer and its scale, as Java keeps them (`1.50` has scale 2).
pub struct JBigDec(pub BigDecimal);
/// `BigInteger`.
pub struct JBigInt(pub BigInt);
/// `MathContext`: digits, and how to round to them (0: unlimited).
pub struct JMathContext(u64, RoundingMode);

fn dec(v: BigDecimal) -> V {
    native(JBigDec(v))
}

fn int(v: BigInt) -> V {
    native(JBigInt(v))
}

fn arithmetic(message: &str) -> V {
    exc("ArithmeticException", message)
}

fn rounding(v: &V) -> RoundingMode {
    let name = match v {
        V::Int(n) => ["UP", "DOWN", "CEILING", "FLOOR", "HALF_UP", "HALF_DOWN", "HALF_EVEN", "UNNECESSARY"].get(*n as usize).copied().unwrap_or("HALF_UP").to_owned(),
        other => other.text(),
    };
    match name.rsplit('.').next().unwrap_or(&name) {
        "UP" => RoundingMode::Up,
        "DOWN" => RoundingMode::Down,
        "CEILING" => RoundingMode::Ceiling,
        "FLOOR" => RoundingMode::Floor,
        "HALF_DOWN" => RoundingMode::HalfDown,
        "HALF_EVEN" => RoundingMode::HalfEven,
        // UNNECESSARY: checked by the caller.
        _ => RoundingMode::HalfUp,
    }
}

fn unnecessary(v: &V) -> bool {
    matches!(v, V::Int(7)) || v.text().ends_with("UNNECESSARY")
}

/// A value as a `BigDecimal`: another one, a `BigInteger`, a number or its text.
pub fn to_dec(v: &V) -> Result<BigDecimal, V> {
    if let Some(d) = v.native::<JBigDec>() {
        return Ok(d.0.clone());
    }
    if let Some(i) = v.native::<JBigInt>() {
        return Ok(BigDecimal::from(i.0.clone()));
    }
    match v {
        V::Int(n) => Ok(BigDecimal::from(*n)),
        V::Long(n) => Ok(BigDecimal::from(*n)),
        V::Double(d) => from_double_text(*d),
        V::Null => Err(exc("NullPointerException", "null")),
        other => BigDecimal::from_str(other.text().trim()).map_err(|_| exc("NumberFormatException", format!("Character array is missing \"exponent\" mark 'e' or 'E'."))),
    }
}

/// `BigDecimal.valueOf(double)`: by the double's shortest text, as Java's `Double.toString` (0.1 is 0.1).
fn from_double_text(d: f64) -> Result<BigDecimal, V> {
    if !d.is_finite() {
        return Err(exc("NumberFormatException", "Infinite or NaN"));
    }
    let text = V::Double(d).text();
    BigDecimal::from_str(&text).map_err(|_| exc("NumberFormatException", text))
}

/// `new BigDecimal(double)`: the double's exact binary value (0.1 is 0.1000000000000000055511151231257827...).
fn from_double_exact(d: f64) -> Result<BigDecimal, V> {
    if !d.is_finite() {
        return Err(exc("NumberFormatException", "Infinite or NaN"));
    }
    if d == 0.0 {
        return Ok(BigDecimal::zero());
    }
    let bits = d.to_bits();
    let negative = bits >> 63 == 1;
    let exponent = ((bits >> 52) & 0x7ff) as i64;
    let mantissa = if exponent == 0 { (bits & 0xf_ffff_ffff_ffff) << 1 } else { (bits & 0xf_ffff_ffff_ffff) | 0x10_0000_0000_0000 };
    let e = exponent - 1075;
    let mut value = BigDecimal::from(mantissa);
    if e > 0 {
        value = value * BigDecimal::from(BigInt::from(2).pow(e as u32));
    } else if e < 0 {
        // mantissa / 2^-e = mantissa * 5^-e / 10^-e: exact.
        let five = BigInt::from(5).pow((-e) as u32);
        value = BigDecimal::new(BigInt::from(mantissa) * five, -e);
    }
    let value = value.normalized();
    Ok(if negative { -value } else { value })
}

fn scale_of(d: &BigDecimal) -> i64 {
    d.as_bigint_and_exponent().1
}

/// Java's `BigDecimal.toString`: plain unless the scale is negative or the number is very small.
pub fn java_text(d: &BigDecimal, plain: bool) -> String {
    let (unscaled, scale) = d.as_bigint_and_exponent();
    let negative = unscaled.sign() == Sign::Minus;
    let digits = unscaled.abs().to_string();
    let adjusted = digits.len() as i64 - 1 - scale;
    let body = if plain || (scale >= 0 && adjusted >= -6) {
        if scale <= 0 {
            let zeros = if unscaled.is_zero() { String::new() } else { "0".repeat((-scale) as usize) };
            format!("{digits}{zeros}")
        } else {
            let scale = scale as usize;
            let padded = if digits.len() <= scale { format!("{}{digits}", "0".repeat(scale - digits.len() + 1)) } else { digits };
            let (whole, fraction) = padded.split_at(padded.len() - scale);
            format!("{whole}.{fraction}")
        }
    } else {
        let (first, rest) = digits.split_at(1);
        let mantissa = if rest.is_empty() { first.to_owned() } else { format!("{first}.{rest}") };
        format!("{mantissa}E{}{adjusted}", if adjusted >= 0 { "+" } else { "" })
    };
    if negative { format!("-{body}") } else { body }
}

/// `a.divide(b)`: the exact quotient, at the scale Java prefers, or the exception Java throws.
fn divide_exact(a: &BigDecimal, b: &BigDecimal) -> Result<BigDecimal, V> {
    if b.is_zero() {
        return Err(arithmetic(if a.is_zero() { "Division undefined" } else { "Division by zero" }));
    }
    let preferred = scale_of(a) - scale_of(b);
    let quotient = (a / b).normalized();
    if &(&quotient * b) != a {
        return Err(arithmetic("Non-terminating decimal expansion; no exact representable decimal result."));
    }
    let scale = scale_of(&quotient).max(preferred);
    Ok(quotient.with_scale(scale))
}

fn divide_scaled(a: &BigDecimal, b: &BigDecimal, scale: i64, mode: &V) -> Result<BigDecimal, V> {
    if b.is_zero() {
        return Err(arithmetic("Division by zero"));
    }
    // Enough digits before rounding: those of the result, and some to round by.
    let wanted = (a.digits() as i64 + b.digits() as i64 + scale.max(0) + 20).max(40) as u64;
    let quotient = a.with_prec(wanted.max(a.digits() + 4)) / b;
    round_to_scale(&quotient, scale, mode)
}

fn round_to_scale(d: &BigDecimal, scale: i64, mode: &V) -> Result<BigDecimal, V> {
    let rounded = d.with_scale_round(scale, rounding(mode));
    if unnecessary(mode) && &rounded != d {
        return Err(arithmetic("Rounding necessary"));
    }
    Ok(rounded)
}

fn with_context(d: BigDecimal, context: &JMathContext) -> BigDecimal {
    if context.0 == 0 || d.digits() <= context.0 {
        return d;
    }
    let drop = d.digits() as i64 - context.0 as i64;
    d.with_scale_round(scale_of(&d) - drop, context.1)
}

pub fn construct(class: &str, a: &[V]) -> Option<R> {
    let simple = class.rsplit('.').next().unwrap_or(class);
    Some(match (simple, a.len()) {
        ("BigDecimal", 1 | 2) => {
            let value = match &a[0] {
                V::Double(d) => from_double_exact(*d),
                // `new BigDecimal(char[])`, `new BigDecimal(BigInteger, scale)`.
                V::Arr(_) => to_dec(&s(&a[0].items().unwrap_or_default().iter().map(V::text).collect::<String>())),
                other if other.native::<JBigInt>().is_some() && a.len() == 2 && matches!(a[1], V::Int(_)) => {
                    Ok(BigDecimal::new(other.native::<JBigInt>().map(|i| i.0.clone()).unwrap_or_default(), a[1].long()))
                }
                other => to_dec(other),
            };
            match (value, a.get(1).and_then(|c| c.native::<JMathContext>())) {
                (Ok(value), Some(context)) => Ok(dec(with_context(value, context))),
                (value, _) => value.map(dec),
            }
        }
        ("BigInteger", 1 | 2) => {
            let radix = a.get(1).map_or(10, V::int) as u32;
            match &a[0] {
                V::Str(text) => BigInt::parse_bytes(text.trim().as_bytes(), radix).map(int).ok_or_else(|| exc("NumberFormatException", format!("For input string: \"{text}\""))),
                other => Ok(int(BigInt::from(other.long()))),
            }
        }
        ("MathContext", 1 | 2) => Ok(native(JMathContext(a[0].long().max(0) as u64, a.get(1).map_or(RoundingMode::HalfUp, rounding)))),
        _ => return None,
    })
}

pub fn field(class: &str, name: &str) -> Option<V> {
    let simple = class.rsplit('.').next().unwrap_or(class);
    Some(match (simple, name) {
        ("BigDecimal", "ZERO") => dec(BigDecimal::zero()),
        ("BigDecimal", "ONE") => dec(BigDecimal::one()),
        ("BigDecimal", "TWO") => dec(BigDecimal::from(2)),
        ("BigDecimal", "TEN") => dec(BigDecimal::from(10)),
        ("BigDecimal", rounding) if rounding.starts_with("ROUND_") => V::Int(["ROUND_UP", "ROUND_DOWN", "ROUND_CEILING", "ROUND_FLOOR", "ROUND_HALF_UP", "ROUND_HALF_DOWN", "ROUND_HALF_EVEN", "ROUND_UNNECESSARY"].iter().position(|n| *n == rounding)? as i32),
        ("BigInteger", "ZERO") => int(BigInt::zero()),
        ("BigInteger", "ONE") => int(BigInt::one()),
        ("BigInteger", "TWO") => int(BigInt::from(2)),
        ("BigInteger", "TEN") => int(BigInt::from(10)),
        ("MathContext", "DECIMAL32") => native(JMathContext(7, RoundingMode::HalfEven)),
        ("MathContext", "DECIMAL64") => native(JMathContext(16, RoundingMode::HalfEven)),
        ("MathContext", "DECIMAL128") => native(JMathContext(34, RoundingMode::HalfEven)),
        ("MathContext", "UNLIMITED") => native(JMathContext(0, RoundingMode::HalfUp)),
        _ => return None,
    })
}

pub fn static_call(class: &str, name: &str, a: &[V]) -> Option<R> {
    let simple = class.rsplit('.').next().unwrap_or(class);
    Some(match (simple, name, a.len()) {
        ("BigDecimal", "valueOf", 1) => match &a[0] {
            V::Double(d) => from_double_text(*d).map(dec),
            other => Ok(dec(BigDecimal::from(other.long()))),
        },
        ("BigDecimal", "valueOf", 2) => Ok(dec(BigDecimal::new(BigInt::from(a[0].long()), a[1].long()))),
        ("BigInteger", "valueOf", 1) => Ok(int(BigInt::from(a[0].long()))),
        ("BigInteger", "probablePrime", _) => return None,
        ("RoundingMode", "valueOf", 1) => Ok(s(&a[0].text())),
        _ => return None,
    })
}

fn result(v: Result<BigDecimal, V>) -> R {
    v.map(dec)
}

pub fn method(this: &V, name: &str, a: &[V]) -> Option<R> {
    if let Some(d) = this.native::<JBigDec>() {
        return Some(dec_method(&d.0, name, a));
    }
    if let Some(i) = this.native::<JBigInt>() {
        return Some(int_method(&i.0, name, a));
    }
    if let Some(c) = this.native::<JMathContext>() {
        return Some(Ok(match name {
            "getPrecision" => V::Int(c.0 as i32),
            "getRoundingMode" => s(&format!("{:?}", c.1).to_uppercase()),
            "toString" => s(&format!("precision={} roundingMode={:?}", c.0, c.1)),
            _ => return None,
        }));
    }
    None
}

fn dec_method(d: &BigDecimal, name: &str, a: &[V]) -> R {
    let other = |at: usize| to_dec(a.get(at).unwrap_or(&V::Null));
    let context = a.iter().find_map(|v| v.native::<JMathContext>());
    let in_context = |v: BigDecimal| match context {
        Some(c) => with_context(v, c),
        None => v,
    };
    match (name, a.len()) {
        ("add", _) => result(other(0).map(|o| in_context(d + o))),
        ("subtract", _) => result(other(0).map(|o| in_context(d - o))),
        ("multiply", _) => result(other(0).map(|o| in_context(d * o))),
        ("divide", 1) => result(other(0).and_then(|o| divide_exact(d, &o))),
        ("divide", 2) if context.is_some() => result(other(0).and_then(|o| {
            if o.is_zero() {
                return Err(arithmetic("Division by zero"));
            }
            let c = context.expect("checked above");
            let precision = if c.0 == 0 { 100 } else { c.0 + 5 };
            Ok(with_context(d.with_prec(precision.max(d.digits() + 4)) / &o, c))
        })),
        ("divide", 2) => result(other(0).and_then(|o| divide_scaled(d, &o, scale_of(d), &a[1]))),
        ("divide", 3) => result(other(0).and_then(|o| divide_scaled(d, &o, a[1].long(), &a[2]))),
        ("divideToIntegralValue", _) => result(other(0).and_then(|o| if o.is_zero() { Err(arithmetic("Division by zero")) } else { Ok(divide_scaled(d, &o, 0, &s("DOWN"))?.with_scale((scale_of(d) - scale_of(&o)).max(0))) })),
        ("remainder", _) => result(other(0).and_then(|o| {
            if o.is_zero() {
                return Err(arithmetic("Division by zero"));
            }
            let whole = divide_scaled(d, &o, 0, &s("DOWN"))?;
            Ok(d - whole * o)
        })),
        ("setScale", 1) => result(round_to_scale(d, a[0].long(), &s("UNNECESSARY"))),
        ("setScale", 2) => result(round_to_scale(d, a[0].long(), &a[1])),
        ("round", 1) => Ok(dec(in_context(d.clone()))),
        ("compareTo", 1) => Ok(V::Int(match other(0)?.cmp(d) {
            std::cmp::Ordering::Less => 1,
            std::cmp::Ordering::Equal => 0,
            std::cmp::Ordering::Greater => -1,
        })),
        // Java's equals counts the scale: 1.0 is not 1.00.
        ("equals", 1) => Ok(V::Bool(a[0].native::<JBigDec>().is_some_and(|o| o.0 == *d && scale_of(&o.0) == scale_of(d)))),
        ("hashCode", 0) => Ok(V::Int(java_text(d, false).bytes().fold(0i32, |h, b| h.wrapping_mul(31).wrapping_add(i32::from(b))))),
        ("signum", 0) => Ok(V::Int(if d.is_zero() { 0 } else if d.is_negative() { -1 } else { 1 })),
        ("negate", _) => Ok(dec(in_context(-d.clone()))),
        ("abs", _) => Ok(dec(in_context(d.abs()))),
        ("plus", _) => Ok(dec(in_context(d.clone()))),
        ("max", 1) => { let o = other(0)?; Ok(dec(if &o > d { o } else { d.clone() })) }
        ("min", 1) => { let o = other(0)?; Ok(dec(if &o < d { o } else { d.clone() })) }
        ("pow", _) => {
            let n = a[0].long();
            if !(0..=999_999_999).contains(&n) {
                return Err(arithmetic("Invalid operation"));
            }
            let mut out = BigDecimal::one();
            for _ in 0..n {
                out = out * d;
            }
            Ok(dec(in_context(out)))
        }
        ("sqrt", _) => d.sqrt().map(|r| dec(in_context(r))).ok_or_else(|| arithmetic("Attempted square root of negative BigDecimal")),
        ("scale", 0) => Ok(V::Int(scale_of(d) as i32)),
        ("precision", 0) => Ok(V::Int(if d.is_zero() { 1 } else { d.digits() as i32 })),
        ("unscaledValue", 0) => Ok(int(d.as_bigint_and_exponent().0)),
        ("stripTrailingZeros", 0) => Ok(dec(if d.is_zero() { BigDecimal::zero() } else { d.normalized() })),
        ("movePointLeft" | "scaleByPowerOfTen" | "movePointRight", 1) => {
            let n = if name == "movePointLeft" { a[0].long() } else { -a[0].long() };
            let (unscaled, scale) = d.as_bigint_and_exponent();
            let moved = BigDecimal::new(unscaled, scale + n);
            Ok(dec(if name != "scaleByPowerOfTen" && scale_of(&moved) < 0 { moved.with_scale(0) } else { moved }))
        }
        ("doubleValue" | "floatValue", 0) => Ok(V::Double(java_text(d, false).parse::<f64>().unwrap_or(0.0))),
        ("intValue" | "shortValue" | "byteValue", 0) => Ok(V::Int(d.with_scale_round(0, RoundingMode::Down).as_bigint_and_exponent().0.to_i64().unwrap_or(0) as i32)),
        ("longValue", 0) => Ok(V::Long(d.with_scale_round(0, RoundingMode::Down).as_bigint_and_exponent().0.to_i64().unwrap_or(0))),
        ("intValueExact" | "longValueExact", 0) => {
            let whole = d.with_scale_round(0, RoundingMode::Down);
            if &whole != d {
                return Err(arithmetic("Rounding necessary"));
            }
            let n = whole.as_bigint_and_exponent().0.to_i64().ok_or_else(|| arithmetic("Overflow"))?;
            Ok(if name == "intValueExact" { V::Int(i32::try_from(n).map_err(|_| arithmetic("Overflow"))?) } else { V::Long(n) })
        }
        ("toBigInteger" | "toBigIntegerExact", 0) => Ok(int(d.with_scale_round(0, RoundingMode::Down).as_bigint_and_exponent().0)),
        ("toString" | "toEngineeringString", 0) => Ok(s(&java_text(d, false))),
        ("toPlainString", 0) => Ok(s(&java_text(d, true))),
        ("ulp", 0) => Ok(dec(BigDecimal::new(BigInt::one(), scale_of(d)))),
        _ => Err(exc("NoSuchMethodError", format!("BigDecimal.{name} is not converted"))),
    }
}

fn int_method(i: &BigInt, name: &str, a: &[V]) -> R {
    let other = |at: usize| -> Result<BigInt, V> {
        let v = a.get(at).unwrap_or(&V::Null);
        match v.native::<JBigInt>() {
            Some(o) => Ok(o.0.clone()),
            None if v.is_null() => Err(exc("NullPointerException", "null")),
            None => Ok(BigInt::from(v.long())),
        }
    };
    let zero_check = |o: &BigInt| if o.is_zero() { Err(arithmetic("BigInteger divide by zero")) } else { Ok(()) };
    Ok(match (name, a.len()) {
        ("add", 1) => int(i + other(0)?),
        ("subtract", 1) => int(i - other(0)?),
        ("multiply", 1) => int(i * other(0)?),
        ("divide", 1) => { let o = other(0)?; zero_check(&o)?; int(i / o) }
        ("remainder", 1) => { let o = other(0)?; zero_check(&o)?; int(i % o) }
        ("mod", 1) => {
            let o = other(0)?;
            if !o.is_positive() {
                return Err(arithmetic("BigInteger: modulus not positive"));
            }
            int(((i % &o) + &o) % &o)
        }
        ("divideAndRemainder", 1) => { let o = other(0)?; zero_check(&o)?; crate::arr(vec![int(i / &o), int(i % &o)]) }
        ("pow", 1) => int(i.pow(a[0].int().max(0) as u32)),
        ("modPow", 2) => int(i.modpow(&other(0)?, &other(1)?)),
        ("gcd", 1) => { use num_integer::Integer; int(i.gcd(&other(0)?)) }
        ("negate", 0) => int(-i),
        ("abs", 0) => int(i.abs()),
        ("signum", 0) => V::Int(if i.is_zero() { 0 } else if i.is_negative() { -1 } else { 1 }),
        ("max", 1) => { let o = other(0)?; int(if &o > i { o } else { i.clone() }) }
        ("min", 1) => { let o = other(0)?; int(if &o < i { o } else { i.clone() }) }
        ("compareTo", 1) => V::Int(match i.cmp(&other(0)?) {
            std::cmp::Ordering::Less => -1,
            std::cmp::Ordering::Equal => 0,
            std::cmp::Ordering::Greater => 1,
        }),
        ("equals", 1) => V::Bool(a[0].native::<JBigInt>().is_some_and(|o| o.0 == *i)),
        ("hashCode", 0) => V::Int(i.to_string().bytes().fold(0i32, |h, b| h.wrapping_mul(31).wrapping_add(i32::from(b)))),
        ("toString", 0) => s(&i.to_string()),
        ("toString", 1) => s(&i.to_str_radix(a[0].int().clamp(2, 36) as u32)),
        ("intValue" | "shortValue" | "byteValue", 0) => V::Int((i & BigInt::from(u32::MAX)).to_u32().unwrap_or(0) as i32),
        ("longValue", 0) => V::Long((i & BigInt::from(u64::MAX)).to_u64().unwrap_or(0) as i64),
        ("intValueExact", 0) => V::Int(i.to_i32().ok_or_else(|| arithmetic("BigInteger out of int range"))?),
        ("longValueExact", 0) => V::Long(i.to_i64().ok_or_else(|| arithmetic("BigInteger out of long range"))?),
        ("doubleValue" | "floatValue", 0) => V::Double(i.to_f64().unwrap_or(f64::INFINITY)),
        ("bitLength", 0) => V::Int(if i.is_negative() { (-i - 1u32).bits() } else { i.bits() } as i32),
        ("bitCount", 0) => V::Int(i.magnitude().count_ones() as i32),
        ("testBit", 1) => V::Bool(i.bit(a[0].long().max(0) as u64)),
        ("shiftLeft", 1) => int(i << a[0].int().max(0) as usize),
        ("shiftRight", 1) => int(i >> a[0].int().max(0) as usize),
        ("and", 1) => int(i & other(0)?),
        ("or", 1) => int(i | other(0)?),
        ("xor", 1) => int(i ^ other(0)?),
        ("sqrt", 0) => int(i.sqrt()),
        ("toByteArray", 0) => crate::javaio::to_byte_array(&i.to_signed_bytes_be()),
        ("isProbablePrime", 1) => V::Bool(probably_prime(i)),
        _ => return Err(exc("NoSuchMethodError", format!("BigInteger.{name} is not converted"))),
    })
}

fn probably_prime(n: &BigInt) -> bool {
    let two = BigInt::from(2);
    if n < &two {
        return false;
    }
    for p in [2u32, 3, 5, 7, 11, 13, 17, 19, 23, 29, 31, 37] {
        let p = BigInt::from(p);
        if n == &p {
            return true;
        }
        if (n % &p).is_zero() {
            return false;
        }
    }
    let one = BigInt::one();
    let minus = n - &one;
    let mut d = minus.clone();
    let mut r = 0;
    while (&d % &two).is_zero() {
        d /= &two;
        r += 1;
    }
    'witness: for a in [2u32, 3, 5, 7, 11, 13, 17, 19, 23, 29, 31, 37] {
        let mut x = BigInt::from(a).modpow(&d, n);
        if x == one || x == minus {
            continue;
        }
        for _ in 1..r {
            x = x.modpow(&two, n);
            if x == minus {
                continue 'witness;
            }
        }
        return false;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(v: &V) -> String {
        // A value made by the code under test.
        java_text(&v.native::<JBigDec>().expect("a BigDecimal").0, false)
    }

    #[test]
    fn answers_as_java_does() {
        let a = construct("BigDecimal", &[s("1.50")]).unwrap().ok().unwrap();
        let b = static_call("BigDecimal", "valueOf", &[V::Double(2.25)]).unwrap().ok().unwrap();
        assert_eq!(text(&method(&a, "add", &[b]).unwrap().ok().unwrap()), "3.75");
        let ten = construct("BigDecimal", &[V::Int(10)]).unwrap().ok().unwrap();
        let three = construct("BigDecimal", &[V::Int(3)]).unwrap().ok().unwrap();
        assert!(method(&ten, "divide", &[three.clone()]).unwrap().ok().is_none());
        assert_eq!(text(&method(&ten, "divide", &[three, V::Int(2), s("HALF_UP")]).unwrap().ok().unwrap()), "3.33");
        let x = construct("BigDecimal", &[s("2.345")]).unwrap().ok().unwrap();
        assert_eq!(text(&method(&x, "setScale", &[V::Int(2), s("HALF_UP")]).unwrap().ok().unwrap()), "2.35");
        assert_eq!(text(&method(&x, "setScale", &[V::Int(2), s("HALF_EVEN")]).unwrap().ok().unwrap()), "2.34");
        assert_eq!(text(&construct("BigDecimal", &[V::Double(0.1)]).unwrap().ok().unwrap()), "0.1000000000000000055511151231257827021181583404541015625");
        assert_eq!(text(&static_call("BigDecimal", "valueOf", &[V::Double(1e-7)]).unwrap().ok().unwrap()), "1.0E-7");
        let four = construct("BigDecimal", &[s("4.00")]).unwrap().ok().unwrap();
        let two = construct("BigDecimal", &[s("2")]).unwrap().ok().unwrap();
        assert_eq!(text(&method(&four, "divide", &[two]).unwrap().ok().unwrap()), "2.00");
        assert_eq!(text(&static_call("BigDecimal", "valueOf", &[V::Long(12345), V::Int(2)]).unwrap().ok().unwrap()), "123.45");
        let big = construct("BigInteger", &[s("123456789012345678901234567890")]).unwrap().ok().unwrap();
        let sq = method(&big, "multiply", &[big.clone()]).unwrap().ok().unwrap();
        assert_eq!(method(&sq, "toString", &[]).unwrap().ok().unwrap().text(), "15241578753238836750495351562536198787501905199875019052100");
    }
}
