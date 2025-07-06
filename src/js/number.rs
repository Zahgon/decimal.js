//! JavaScript number semantics: `Number::toString`, `Number.prototype.toExponential()`,
//! `StringToNumber`, `ToInt32` and the `Math` functions decimal.js relies on.
//!
//! decimal.js does much of its bookkeeping with JavaScript numbers (IEEE 754 doubles), and converts
//! numbers to strings and back on several paths. Reproducing its results requires the exact
//! conversions of the ECMAScript specification, which this module implements on top of `f64`.

/// The decimal digits and exponent of the shortest representation of a positive finite double:
/// `x == 0.d1 d2 ... dk × 10^n`.
struct Shortest {
    digits: Vec<u8>,
    n: i32,
}

/// The significant digits and decimal exponent of a `{:e}` rendering. Rust always writes that exponent
/// as a plain integer, so the zero fallback is never taken.
fn split_exponential(text: &str) -> (Vec<u8>, i32) {
    let (mantissa, exponent) = text.split_once('e').unwrap_or((text, "0"));
    let digits = mantissa.bytes().filter(u8::is_ascii_digit).collect();
    (digits, exponent.parse().unwrap_or(0))
}

fn shortest(x: f64) -> Shortest {
    // `{:e}` prints a shortest digit string that round-trips and is closest to x. ECMAScript
    // further asks for the even one of two equally close candidates.
    let (mut digits, exponent) = split_exponential(&format!("{:e}", x));
    if let Some(even) = even_tie(x, &digits, exponent) {
        digits = even;
    }
    Shortest {
        digits,
        n: exponent + 1,
    }
}

/// If x lies exactly halfway between the k-digit candidate `digits` and its neighbour, and the
/// neighbour is even and also round-trips, the neighbour.
fn even_tie(x: f64, digits: &[u8], exponent: i32) -> Option<Vec<u8>> {
    let k = digits.len();
    if (digits[k - 1] - b'0').is_multiple_of(2) {
        return None;
    }
    // A tie needs the digit after the k-th to be 5 and all later digits zero. Check a few dozen
    // digits first, then the exact expansion (a double has at most 767 significant digits).
    let halfway = |precision: usize| -> Option<Vec<u8>> {
        let (exact, e) = split_exponential(&format!("{:.*e}", precision, x));
        if e != exponent || exact[k] != b'5' || exact[k + 1..].iter().any(|&d| d != b'0') {
            return None;
        }
        Some(exact[..k].to_vec())
    };
    halfway(k + 24)?;
    let lower = halfway(800)?;
    let neighbour = if lower == digits {
        increment(&lower)?
    } else if increment(&lower).as_deref() == Some(digits) {
        lower
    } else {
        return None;
    };
    let text = format!(
        "{}e{}",
        neighbour.iter().map(|&d| d as char).collect::<String>(),
        exponent - k as i32 + 1
    );
    (text.parse::<f64>() == Ok(x) && neighbour.len() == k).then_some(neighbour)
}

/// The digit string plus one in its last place, `None` on a carry out of the first digit.
fn increment(digits: &[u8]) -> Option<Vec<u8>> {
    let mut out = digits.to_vec();
    for d in out.iter_mut().rev() {
        if *d == b'9' {
            *d = b'0';
        } else {
            *d += 1;
            return Some(out);
        }
    }
    None
}

fn push_digits(out: &mut String, digits: &[u8]) {
    out.extend(digits.iter().map(|&d| d as char));
}

/// `Number::toString(x)` with radix 10, as `String(x)` and string concatenation produce it.
pub fn to_string(x: f64) -> String {
    if x.is_nan() {
        return "NaN".to_string();
    }
    if x == 0.0 {
        return "0".to_string();
    }
    if x.is_infinite() {
        return if x > 0.0 { "Infinity" } else { "-Infinity" }.to_string();
    }
    let mut out = String::new();
    if x < 0.0 {
        out.push('-');
    }
    let Shortest { digits, n } = shortest(x.abs());
    let k = digits.len() as i32;
    if k <= n && n <= 21 {
        push_digits(&mut out, &digits);
        out.extend(std::iter::repeat_n('0', (n - k) as usize));
    } else if 0 < n && n <= 21 {
        push_digits(&mut out, &digits[..n as usize]);
        out.push('.');
        push_digits(&mut out, &digits[n as usize..]);
    } else if -6 < n && n <= 0 {
        out.push_str("0.");
        out.extend(std::iter::repeat_n('0', (-n) as usize));
        push_digits(&mut out, &digits);
    } else {
        push_exponential(&mut out, &digits, n - 1);
    }
    out
}

fn push_exponential(out: &mut String, digits: &[u8], e: i32) {
    out.push(digits[0] as char);
    if digits.len() > 1 {
        out.push('.');
        push_digits(out, &digits[1..]);
    }
    out.push('e');
    out.push(if e < 0 { '-' } else { '+' });
    out.push_str(&e.unsigned_abs().to_string());
}

/// `Number.prototype.toExponential()` called without a fraction digit count.
pub fn to_exponential(x: f64) -> String {
    if !x.is_finite() {
        return to_string(x);
    }
    let mut out = String::new();
    if x < 0.0 {
        out.push('-');
    }
    if x == 0.0 {
        out.push_str("0e+0");
        return out;
    }
    let Shortest { digits, n } = shortest(x.abs());
    push_exponential(&mut out, &digits, n - 1);
    out
}

/// `ToInt32`: the conversion behind the `|0` and `~~` operators.
pub fn to_int32(x: f64) -> i32 {
    if !x.is_finite() {
        return 0;
    }
    let t = x.trunc();
    let m = t.rem_euclid(4_294_967_296.0);
    if m >= 2_147_483_648.0 {
        (m - 4_294_967_296.0) as i32
    } else {
        m as i32
    }
}

/// Whether `c` is white space or a line terminator for `StringToNumber` (and `String.prototype.trim`).
pub fn is_js_space(c: char) -> bool {
    matches!(
        c,
        '\u{0009}'
            | '\u{000A}'
            | '\u{000B}'
            | '\u{000C}'
            | '\u{000D}'
            | '\u{0020}'
            | '\u{00A0}'
            | '\u{1680}'
            | '\u{2000}'
            ..='\u{200A}'
                | '\u{2028}'
                | '\u{2029}'
                | '\u{202F}'
                | '\u{205F}'
                | '\u{3000}'
                | '\u{FEFF}'
    )
}

/// `StringToNumber`: the conversion of `Number(text)` and the unary `+` operator.
pub fn string_to_number(text: &str) -> f64 {
    let s = text.trim_matches(is_js_space);
    if s.is_empty() {
        return 0.0;
    }
    let bytes = s.as_bytes();
    if bytes.len() > 2 && bytes[0] == b'0' {
        let radix = match bytes[1] {
            b'x' | b'X' => 16,
            b'o' | b'O' => 8,
            b'b' | b'B' => 2,
            _ => 0,
        };
        if radix != 0 {
            return parse_radix_integer(&s[2..], radix).unwrap_or(f64::NAN);
        }
    }
    let unsigned = s.strip_prefix(['+', '-']).unwrap_or(s);
    if unsigned == "Infinity" {
        return if s.starts_with('-') {
            f64::NEG_INFINITY
        } else {
            f64::INFINITY
        };
    }
    if !is_decimal_literal(unsigned) {
        return f64::NAN;
    }
    s.parse::<f64>().unwrap_or(f64::NAN)
}

/// `StrUnsignedDecimalLiteral` without `Infinity`: digits with an optional point and exponent.
fn is_decimal_literal(s: &str) -> bool {
    let b = s.as_bytes();
    let mut i = 0;
    let int_digits = count_digits(&b[i..]);
    i += int_digits;
    let mut frac_digits = 0;
    if i < b.len() && b[i] == b'.' {
        i += 1;
        frac_digits = count_digits(&b[i..]);
        i += frac_digits;
    }
    if int_digits == 0 && frac_digits == 0 {
        return false;
    }
    if i < b.len() && (b[i] == b'e' || b[i] == b'E') {
        i += 1;
        if i < b.len() && (b[i] == b'+' || b[i] == b'-') {
            i += 1;
        }
        let exp_digits = count_digits(&b[i..]);
        if exp_digits == 0 {
            return false;
        }
        i += exp_digits;
    }
    i == b.len()
}

fn count_digits(b: &[u8]) -> usize {
    b.iter().take_while(|c| c.is_ascii_digit()).count()
}

/// The value of a non-empty string of digits in a power-of-two radix, correctly rounded to a double
/// (round half to even), or `None` if a character is not a digit of the radix.
fn parse_radix_integer(digits: &str, radix: u32) -> Option<f64> {
    let bits_per_digit = radix.trailing_zeros();
    let mut values = Vec::with_capacity(digits.len());
    for c in digits.chars() {
        values.push(c.to_digit(radix)?);
    }
    // Collect the bits, most significant first, without leading zeros.
    let mut bits: Vec<bool> = Vec::with_capacity(values.len() * bits_per_digit as usize);
    for v in values {
        for k in (0..bits_per_digit).rev() {
            let bit = (v >> k) & 1 == 1;
            if bit || !bits.is_empty() {
                bits.push(bit);
            }
        }
    }
    Some(round_bits(&bits))
}

/// A binary integer (most significant bit first, no leading zeros) rounded to the nearest double.
fn round_bits(bits: &[bool]) -> f64 {
    if bits.is_empty() {
        return 0.0;
    }
    if bits.len() <= 53 {
        return bits.iter().fold(0u64, |acc, &b| acc << 1 | b as u64) as f64;
    }
    let mut mantissa = bits[..53].iter().fold(0u64, |acc, &b| acc << 1 | b as u64);
    let half = bits[53];
    let sticky = bits[54..].iter().any(|&b| b);
    if half && (sticky || mantissa & 1 == 1) {
        mantissa += 1;
    }
    let shift = (bits.len() - 53) as i32;
    if shift > 1100 {
        return f64::INFINITY;
    }
    mantissa as f64 * 2f64.powi(shift)
}

/// `Math.pow(x, y)` with the special cases of ECMAScript (which differ from C for a base of ±1 and
/// an infinite or NaN exponent).
pub fn pow(x: f64, y: f64) -> f64 {
    if y.is_nan() {
        return f64::NAN;
    }
    if y == 0.0 {
        return 1.0;
    }
    if (x == 1.0 || x == -1.0) && y.is_infinite() {
        return f64::NAN;
    }
    if x.is_nan() {
        return f64::NAN;
    }
    super::fdlibm::pow(x, y)
}

/// `Math.log(x)`.
pub fn log(x: f64) -> f64 {
    super::fdlibm::log(x)
}

/// `Math.sqrt(x)`, correctly rounded like every IEEE 754 square root.
pub fn sqrt(x: f64) -> f64 {
    x.sqrt()
}

/// `Math.LN10`.
pub const LN10: f64 = std::f64::consts::LN_10;

/// `Math.floor` applied to an integer quotient: `Math.floor(a / b)` for exact integer operands.
pub fn floor_div(a: i64, b: i64) -> i64 {
    a.div_euclid(b) - if b < 0 && a.rem_euclid(b) != 0 { 1 } else { 0 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn number_to_string_follows_ecmascript() {
        assert_eq!(to_string(1e21), "1e+21");
        assert_eq!(to_string(1e20), "100000000000000000000");
        assert_eq!(to_string(1e-7), "1e-7");
        assert_eq!(to_string(0.000001), "0.000001");
        assert_eq!(to_string(-0.0), "0");
        assert_eq!(to_string(0.1 + 0.2), "0.30000000000000004");
        assert_eq!(to_string(5e-324), "5e-324");
        assert_eq!(to_string(1.7976931348623157e308), "1.7976931348623157e+308");
        assert_eq!(to_string(123.456), "123.456");
        assert_eq!(to_string(f64::NEG_INFINITY), "-Infinity");
    }

    #[test]
    #[allow(clippy::excessive_precision)]
    fn equally_close_candidates_resolve_to_even() {
        assert_eq!(to_string(-9859.0850830078125), "-9859.085083007812");
        assert_eq!(to_string(1960165500640869.25), "1960165500640869.2");
        assert_eq!(to_string(582618713378.90625), "582618713378.9062");
        assert_eq!(to_exponential(1608619.68994140625), "1.6086196899414062e+6");
        assert_eq!(to_string(0.1), "0.1");
        assert_eq!(to_string(5e-324), "5e-324");
    }

    #[test]
    fn to_exponential_uses_the_shortest_digits() {
        assert_eq!(to_exponential(1.0), "1e+0");
        assert_eq!(to_exponential(123.456), "1.23456e+2");
        assert_eq!(to_exponential(0.00015), "1.5e-4");
    }

    #[test]
    fn string_to_number_accepts_javascript_literals() {
        assert_eq!(string_to_number("  42  "), 42.0);
        assert_eq!(string_to_number(""), 0.0);
        assert_eq!(string_to_number("0x1F"), 31.0);
        assert_eq!(string_to_number("0b101"), 5.0);
        assert_eq!(string_to_number("0o17"), 15.0);
        assert!(string_to_number("-0x10").is_nan());
        assert!(string_to_number("1e").is_nan());
        assert!(string_to_number("infinity").is_nan());
        assert_eq!(string_to_number("-Infinity"), f64::NEG_INFINITY);
        assert_eq!(string_to_number(".5"), 0.5);
        assert_eq!(string_to_number("5."), 5.0);
        assert!(string_to_number("1_000").is_nan());
        assert_eq!(string_to_number("\u{a0}12\u{3000}"), 12.0);
        assert!(string_to_number("-0").is_sign_negative());
    }

    #[test]
    fn hex_strings_round_half_to_even() {
        assert_eq!(string_to_number("0x20000000000001"), 9007199254740992.0);
        assert_eq!(string_to_number("0x20000000000003"), 9007199254740996.0);
        assert_eq!(string_to_number("0x200000000000011"), 144115188075855904.0);
    }

    #[test]
    fn to_int32_wraps() {
        assert_eq!(to_int32(4294967298.0), 2);
        assert_eq!(to_int32(2147483648.0), -2147483648);
        assert_eq!(to_int32(-1.5), -1);
        assert_eq!(to_int32(f64::NAN), 0);
    }
}
