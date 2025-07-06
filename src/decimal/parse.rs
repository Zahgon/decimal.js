//! The Decimal constructor: `new Decimal(v)` for numbers, strings (decimal, binary, octal and
//! hexadecimal notation), BigInts and Decimals.

use super::arith::{divide, int_pow, DivSpec};
use super::digits::{convert_base, get_base10_exponent, word_value};
use super::{Context, Decimal, BASE, LOG_BASE};
use crate::js::number;
use crate::js::{BigInt, JsError, Value};

/// `new Decimal(v)`.
pub(crate) fn construct(cx: &mut Context, v: &Value) -> Result<Decimal, JsError> {
    match v {
        Value::Decimal(d) => Ok(Decimal::copy_of(cx, d)),
        Value::Number(n) => Ok(from_number(cx, *n)),
        Value::String(s) => parse_str(cx, s),
        Value::BigInt(b) => Ok(from_bigint(cx, b)),
        other => Err(JsError::invalid_argument(&other.concat_string(cx))),
    }
}

impl Decimal {
    /// `new Ctor(v)` for a Decimal `v`: a copy, which becomes Infinity or zero if the exponent is
    /// outside `[minE, maxE]` while results are external.
    pub(crate) fn copy_of(cx: &Context, v: &Decimal) -> Decimal {
        if cx.external {
            match &v.d {
                None => Decimal {
                    s: v.s,
                    e: 0,
                    d: None,
                },
                Some(_) if v.e > cx.max_e => Decimal {
                    s: v.s,
                    e: 0,
                    d: None,
                },
                Some(_) if v.e < cx.min_e => Decimal {
                    s: v.s,
                    e: 0,
                    d: Some(vec![0]),
                },
                Some(_) => v.clone(),
            }
        } else {
            v.clone()
        }
    }
}

/// `new Decimal(n)` for a JavaScript number.
pub(crate) fn from_number(cx: &Context, v: f64) -> Decimal {
    if v == 0.0 {
        return Decimal::zero(if v.is_sign_negative() { -1 } else { 1 });
    }

    let (s, v) = if v < 0.0 { (-1, -v) } else { (1, v) };

    // Fast path for small integers.
    if v == f64::from(number::to_int32(v)) && v < 1e7 {
        let w = v as i64;
        let e = super::digits::digit_count(w) - 1;
        if cx.external {
            if e > cx.max_e {
                return Decimal::infinity(s);
            }
            if e < cx.min_e {
                return Decimal::zero(s);
            }
        }
        return Decimal {
            s,
            e,
            d: Some(vec![w]),
        };
    }

    if v.is_nan() {
        return Decimal::nan();
    }
    if v.is_infinite() {
        return Decimal::infinity(s);
    }

    parse_decimal(cx, s, &number::to_string(v))
}

/// `new Decimal(n)` for a BigInt.
pub(crate) fn from_bigint(cx: &Context, v: &BigInt) -> Decimal {
    let s = if v.is_negative() { -1 } else { 1 };
    parse_decimal(cx, s, v.magnitude())
}

/// `new Decimal(str)`.
pub(crate) fn parse_str(cx: &mut Context, v: &str) -> Result<Decimal, JsError> {
    let (s, rest) = match v.as_bytes().first() {
        Some(b'-') => (-1, &v[1..]),
        Some(b'+') => (1, &v[1..]),
        _ => (1, v),
    };
    if is_decimal(rest) {
        Ok(parse_decimal(cx, s, rest))
    } else {
        parse_other(cx, s, rest)
    }
}

/// `new Ctor(text)` for text the caller has just built from digits and an exponent or from a JavaScript
/// number: an optionally signed decimal numeral, `Infinity` or `NaN`. decimal.js parses these without
/// throwing, so unlike [`parse_str`] this conversion has no error case.
pub(crate) fn parse_numeric(cx: &Context, text: &str) -> Decimal {
    let (s, rest) = match text.as_bytes().first() {
        Some(b'-') => (-1, &text[1..]),
        Some(b'+') => (1, &text[1..]),
        _ => (1, text),
    };
    if is_decimal(rest) {
        parse_decimal(cx, s, rest)
    } else if rest == "Infinity" {
        Decimal::infinity(s)
    } else {
        Decimal::nan()
    }
}

/// The largest exponent magnitude kept exactly; beyond it a value can only overflow or underflow.
const EXPONENT_CLAMP: f64 = 1e17;

/// `parseDecimal(x, str)` for a string that matches `isDecimal` (or the digits of an integer).
pub(crate) fn parse_decimal(cx: &Context, s: i8, text: &str) -> Decimal {
    let mut str = text.to_string();
    let dot = str.find('.');
    if let Some(at) = dot {
        str.remove(at);
    }

    let mut e: f64;
    if let Some(i) = str.find(['e', 'E']).filter(|&i| i > 0) {
        e = dot.unwrap_or(i) as f64;
        let exponent = number::string_to_number(&str[i + 1..]);
        e += exponent.clamp(-EXPONENT_CLAMP, EXPONENT_CLAMP);
        str.truncate(i);
    } else {
        e = dot.unwrap_or(str.len()) as f64;
    }

    let bytes = str.as_bytes();
    let lead = bytes.iter().take_while(|&&b| b == b'0').count();
    let mut end = bytes.len();
    while end > lead && bytes[end - 1] == b'0' {
        end -= 1;
    }
    let digits = &str[lead..end];

    if digits.is_empty() {
        return Decimal::zero(s);
    }

    let len = digits.len() as i64;
    let e = (e - lead as f64 - 1.0) as i64;
    let mut d = Vec::with_capacity(digits.len() / LOG_BASE as usize + 2);

    // i is where to slice the digits to get the first word.
    let mut i = (e + 1) % LOG_BASE;
    if e < 0 {
        i += LOG_BASE;
    }

    let mut rest: String;
    if i < len {
        let mut at = i as usize;
        if at > 0 {
            d.push(word_value(&digits[..at]));
        }
        let full = len - LOG_BASE;
        while (at as i64) < full {
            d.push(word_value(&digits[at..at + LOG_BASE as usize]));
            at += LOG_BASE as usize;
        }
        rest = digits[at..].to_string();
        i = LOG_BASE - rest.len() as i64;
    } else {
        rest = digits.to_string();
        i -= len;
    }
    for _ in 0..i.max(0) {
        rest.push('0');
    }
    d.push(word_value(&rest));

    if cx.external {
        if e > cx.max_e {
            return Decimal::infinity(s);
        }
        if e < cx.min_e {
            return Decimal::zero(s);
        }
    }

    Decimal { s, e, d: Some(d) }
}

/// `parseOther(x, str)`: `Infinity`, `NaN`, decimal notation with `_` separators, and binary, octal
/// and hexadecimal notation with an optional binary exponent.
fn parse_other(cx: &mut Context, s: i8, text: &str) -> Result<Decimal, JsError> {
    let mut str = text.to_string();
    if str.contains('_') {
        str = remove_digit_separators(&str);
        if is_decimal(&str) {
            return Ok(parse_decimal(cx, s, &str));
        }
    } else if str == "Infinity" {
        return Ok(Decimal::infinity(s));
    } else if str == "NaN" {
        return Ok(Decimal::nan());
    }

    let base: i64;
    if is_based(&str, 'x', 16) {
        base = 16;
        str = str.to_ascii_lowercase();
    } else if is_based(&str, 'b', 2) {
        base = 2;
    } else if is_based(&str, 'o', 8) {
        base = 8;
    } else {
        return Err(JsError::invalid_argument(&str));
    }

    // Is there a binary exponent part?
    let mut p = 0.0;
    let mut has_p = false;
    match str.find(['p', 'P']).filter(|&i| i > 0) {
        Some(i) => {
            p = number::string_to_number(&str[i + 1..]);
            has_p = true;
            str = str[2..i].to_string();
        }
        None => str = str[2..].to_string(),
    }

    // Convert the digits as an integer, then divide by base^(fraction digits).
    let dot = str.find('.');
    let mut divisor = None;
    let mut len = 0;
    if let Some(at) = dot {
        str.remove(at);
        len = str.len() as i64;
        let i = len - at as i64;
        let b = from_number(cx, base as f64);
        divisor = Some(int_pow(cx, &b, i as f64, (i * 2) as f64)?);
    }

    let mut xd = convert_base(&str, base, BASE);
    let xe = xd.len() as i64 - 1;

    // Remove trailing zeros.
    while xd.last() == Some(&0) {
        xd.pop();
    }
    if xd.is_empty() {
        return Ok(Decimal::zero(s));
    }

    let mut x = Decimal {
        s,
        e: get_base10_exponent(&xd, xe),
        d: Some(xd),
    };
    cx.external = false;

    // Dividing at 4 times as many digits as the text has is always exact enough. decimal.js passes
    // no rounding mode here, so the quotient is rounded like ROUND_HALF_DOWN.
    if let Some(divisor) = divisor {
        x = divide(cx, &x, &divisor, DivSpec::sd(len * 4, 5));
    }

    // Multiply by the binary exponent part if present.
    if has_p && p != 0.0 && !p.is_nan() {
        let factor = if p.abs() < 54.0 {
            from_number(cx, number::pow(2.0, p))
        } else {
            let two = from_number(cx, 2.0);
            let exponent = from_number(cx, p);
            two.pow(cx, &exponent)?
        };
        x = x.times(cx, &factor);
    }
    cx.external = true;

    Ok(x)
}

/// `str.replace(/(\d)_(?=\d)/g, '$1')`.
fn remove_digit_separators(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        out.push(c);
        if c.is_ascii_digit()
            && chars.get(i + 1) == Some(&'_')
            && chars.get(i + 2).is_some_and(char::is_ascii_digit)
        {
            i += 2;
        } else {
            i += 1;
        }
    }
    out
}

/// The length of the run of characters at the start of `b` that satisfy `f`.
fn run(b: &[u8], f: impl Fn(u8) -> bool) -> usize {
    b.iter().take_while(|&&c| f(c)).count()
}

/// Whether `b` is a significand with digits accepted by `f`: `D+(\.D*)?|\.D+`; returns its length.
fn significand(b: &[u8], f: impl Fn(u8) -> bool + Copy) -> Option<usize> {
    let int = run(b, f);
    let mut i = int;
    if i < b.len() && b[i] == b'.' {
        let frac = run(&b[i + 1..], f);
        if int == 0 && frac == 0 {
            return None;
        }
        i += 1 + frac;
    } else if int == 0 {
        return None;
    }
    Some(i)
}

/// Whether `b` is an exponent part `[+-]?\d+` (after the marker letter).
fn is_exponent(b: &[u8]) -> bool {
    let b = match b.first() {
        Some(b'+' | b'-') => &b[1..],
        _ => b,
    };
    !b.is_empty() && b.iter().all(u8::is_ascii_digit)
}

/// `isDecimal`: `/^(\d+(\.\d*)?|\.\d+)(e[+-]?\d+)?$/i`.
pub(crate) fn is_decimal(s: &str) -> bool {
    let b = s.as_bytes();
    let Some(n) = significand(b, |c| c.is_ascii_digit()) else {
        return false;
    };
    n == b.len() || matches!(b[n], b'e' | b'E') && is_exponent(&b[n + 1..])
}

/// `isHex`, `isBinary` and `isOctal`: `/^0x(H+(\.H*)?|\.H+)(p[+-]?\d+)?$/i` for the digits of `radix`.
fn is_based(s: &str, marker: char, radix: u32) -> bool {
    let b = s.as_bytes();
    if b.len() < 2 || b[0] != b'0' || !(b[1] as char).eq_ignore_ascii_case(&marker) {
        return false;
    }
    let b = &b[2..];
    let Some(n) = significand(b, |c| (c as char).is_digit(radix)) else {
        return false;
    };
    n == b.len() || matches!(b[n], b'p' | b'P') && is_exponent(&b[n + 1..])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(cx: &mut Context, s: &str) -> String {
        match parse_str(cx, s) {
            Ok(d) => d.to_js_string(cx),
            Err(e) => e.to_string(),
        }
    }

    #[test]
    fn decimal_notation() {
        let mut cx = Context::default();
        assert_eq!(parse(&mut cx, "123.4500"), "123.45");
        assert_eq!(parse(&mut cx, "-0.000012"), "-0.000012");
        assert_eq!(parse(&mut cx, "1e21"), "1e+21");
        assert_eq!(parse(&mut cx, ".5e-3"), "0.0005");
        assert_eq!(parse(&mut cx, "5."), "5");
        assert_eq!(
            parse(&mut cx, "00012345678901234567890"),
            "12345678901234567890"
        );
        assert_eq!(parse(&mut cx, "1_000_000"), "1000000");
        assert_eq!(parse(&mut cx, "-Infinity"), "-Infinity");
        assert_eq!(parse(&mut cx, "+NaN"), "NaN");
    }

    #[test]
    fn invalid_strings() {
        let mut cx = Context::default();
        for (input, shown) in [
            ("-abc", "abc"),
            ("", ""),
            ("1e", "1e"),
            ("1__0", "1__0"),
            ("0x1_F", "0x1_F"),
            ("1_000x", "1000x"),
            ("infinity", "infinity"),
            ("0b2", "0b2"),
        ] {
            assert_eq!(
                parse(&mut cx, input),
                format!("Error: [DecimalError] Invalid argument: {shown}")
            );
        }
    }

    #[test]
    fn other_bases() {
        let mut cx = Context::default();
        assert_eq!(parse(&mut cx, "0xff"), "255");
        assert_eq!(parse(&mut cx, "-0b101.1"), "-5.5");
        assert_eq!(parse(&mut cx, "0o17"), "15");
        assert_eq!(parse(&mut cx, "0x1p4"), "16");
        assert_eq!(parse(&mut cx, "0x.8"), "0.5");
        assert_eq!(parse(&mut cx, "0x0"), "0");
        assert!(cx.external);
    }

    #[test]
    fn numbers() {
        let cx = Context::default();
        assert_eq!(from_number(&cx, 0.1).to_js_string(&cx), "0.1");
        assert_eq!(from_number(&cx, 1e21).to_js_string(&cx), "1e+21");
        assert_eq!(from_number(&cx, -0.0).value_of(&cx), "-0");
        assert_eq!(from_number(&cx, 9999999.0).to_js_string(&cx), "9999999");
        assert_eq!(from_number(&cx, 5e-324).to_js_string(&cx), "5e-324");
    }
}
