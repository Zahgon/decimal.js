//! The JavaScript values that reach decimal.js through the `decimal` command, and the conversions
//! decimal.js applies to them (`ToString`, `ToNumber`, `ToBoolean`, `ToInt32`).

use super::error::JsError;
use super::number;
use crate::decimal::{Context, Decimal};

/// A JavaScript BigInt: a sign and the decimal digits of the magnitude, without leading zeros.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BigInt {
    negative: bool,
    digits: String,
}

impl BigInt {
    /// `BigInt(text)` for an optionally signed string of decimal digits; `None` for any other text.
    pub fn parse(text: &str) -> Option<BigInt> {
        let (negative, unsigned) = match text.as_bytes().first() {
            Some(b'-') => (true, &text[1..]),
            Some(b'+') => (false, &text[1..]),
            _ => (false, text),
        };
        if unsigned.is_empty() || !unsigned.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        let trimmed = unsigned.trim_start_matches('0');
        let digits = if trimmed.is_empty() { "0" } else { trimmed };
        Some(BigInt {
            negative: negative && digits != "0",
            digits: digits.to_string(),
        })
    }

    /// Whether the value is less than zero.
    pub fn is_negative(&self) -> bool {
        self.negative
    }

    /// The decimal digits of the absolute value.
    pub fn magnitude(&self) -> &str {
        &self.digits
    }

    /// The comparison of a BigInt with an integer, as the relational operators perform it: by
    /// mathematical value.
    pub fn compare_integer(&self, n: i64) -> std::cmp::Ordering {
        match self.digits.parse::<i128>() {
            Ok(m) => (if self.negative { -m } else { m }).cmp(&i128::from(n)),
            Err(_) if self.negative => std::cmp::Ordering::Less,
            Err(_) => std::cmp::Ordering::Greater,
        }
    }
}

impl std::fmt::Display for BigInt {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.negative {
            f.write_str("-")?;
        }
        f.write_str(&self.digits)
    }
}

/// A JavaScript value.
#[derive(Clone, Debug)]
pub enum Value {
    Undefined,
    Null,
    Bool(bool),
    Number(f64),
    String(String),
    BigInt(BigInt),
    /// An array.
    Array(Vec<Value>),
    /// A plain object; the entries are in the order `Object.keys` gives.
    Object(Vec<(String, Value)>),
    /// A Decimal instance.
    Decimal(Decimal),
}

impl Value {
    /// Whether the value is `undefined` (`v === void 0`).
    pub fn is_undefined(&self) -> bool {
        matches!(self, Value::Undefined)
    }

    /// Whether the value is `null` or `undefined` (`v == null`).
    pub fn is_nullish(&self) -> bool {
        matches!(self, Value::Undefined | Value::Null)
    }

    /// `ToBoolean`.
    pub fn truthy(&self) -> bool {
        match self {
            Value::Undefined | Value::Null => false,
            Value::Bool(b) => *b,
            Value::Number(n) => !(*n == 0.0 || n.is_nan()),
            Value::String(s) => !s.is_empty(),
            Value::BigInt(b) => b.magnitude() != "0",
            Value::Array(_) | Value::Object(_) | Value::Decimal(_) => true,
        }
    }

    /// The string that string concatenation (`'text' + v`) makes of the value: objects are converted
    /// with the default hint, so a Decimal contributes its `valueOf()`.
    pub fn concat_string(&self, cx: &Context) -> String {
        match self {
            Value::Decimal(d) => d.value_of(cx),
            _ => self.to_js_string(cx),
        }
    }

    /// `ToString`, as `String(v)` and `Array.prototype.join` perform it: objects are converted with
    /// the string hint, so a Decimal contributes its `toString()`.
    pub fn to_js_string(&self, cx: &Context) -> String {
        match self {
            Value::Undefined => "undefined".to_string(),
            Value::Null => "null".to_string(),
            Value::Bool(b) => b.to_string(),
            Value::Number(n) => number::to_string(*n),
            Value::String(s) => s.clone(),
            Value::BigInt(b) => b.to_string(),
            Value::Array(items) => join(items, cx),
            Value::Object(_) => "[object Object]".to_string(),
            Value::Decimal(d) => d.to_js_string(cx),
        }
    }

    /// `ToNumber`.
    pub fn to_number(&self, cx: &Context) -> Result<f64, JsError> {
        Ok(match self {
            Value::Undefined => f64::NAN,
            Value::Null => 0.0,
            Value::Bool(b) => f64::from(u8::from(*b)),
            Value::Number(n) => *n,
            Value::String(s) => number::string_to_number(s),
            Value::BigInt(_) => return Err(JsError::bigint_to_number()),
            Value::Array(items) => number::string_to_number(&join(items, cx)),
            Value::Object(_) => f64::NAN,
            Value::Decimal(d) => number::string_to_number(&d.value_of(cx)),
        })
    }

    /// The name `typeof` gives the value.
    pub fn type_of(&self) -> &'static str {
        match self {
            Value::Undefined => "undefined",
            Value::Null | Value::Array(_) | Value::Object(_) | Value::Decimal(_) => "object",
            Value::Bool(_) => "boolean",
            Value::Number(_) => "number",
            Value::String(_) => "string",
            Value::BigInt(_) => "bigint",
        }
    }
}

/// `Array.prototype.join` with the default separator.
fn join(items: &[Value], cx: &Context) -> String {
    let mut out = String::new();
    for (i, item) in items.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        if !item.is_nullish() {
            out.push_str(&item.to_js_string(cx));
        }
    }
    out
}

/// decimal.js `checkInt32(i, min, max)`: the value must be a number that is an integer within
/// `[min, max]` (`i === ~~i`), otherwise `Invalid argument: i` is thrown.
pub fn check_int32(cx: &Context, i: &Value, min: i64, max: i64) -> Result<i64, JsError> {
    match i {
        Value::Number(n) if *n == f64::from(number::to_int32(*n)) => {
            let n = number::to_int32(*n) as i64;
            if n < min || n > max {
                Err(JsError::invalid_argument(&i.concat_string(cx)))
            } else {
                Ok(n)
            }
        }
        Value::BigInt(b) => {
            // `~~1n === 1n`, so a BigInt within range passes the check and fails at the first
            // arithmetic that mixes it with numbers.
            let in_range = b.compare_integer(min) != std::cmp::Ordering::Less
                && b.compare_integer(max) != std::cmp::Ordering::Greater;
            if in_range {
                Err(JsError::mixed_bigint())
            } else {
                Err(JsError::invalid_argument(&i.concat_string(cx)))
            }
        }
        _ => Err(JsError::invalid_argument(&i.concat_string(cx))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bigint_parsing_normalises() {
        assert_eq!(BigInt::parse("+42").unwrap().to_string(), "42");
        assert_eq!(BigInt::parse("-0").unwrap().to_string(), "0");
        assert_eq!(BigInt::parse("-007").unwrap().to_string(), "-7");
        assert!(BigInt::parse("1.5").is_none());
        assert!(BigInt::parse("").is_none());
        assert!(BigInt::parse("-").is_none());
    }

    #[test]
    fn strings_of_values() {
        let cx = Context::default();
        let arr = Value::Array(vec![
            Value::Number(1.0),
            Value::Null,
            Value::Array(vec![Value::Number(2.0), Value::Number(3.0)]),
            Value::Undefined,
        ]);
        assert_eq!(arr.to_js_string(&cx), "1,,2,3,");
        assert_eq!(Value::Object(vec![]).concat_string(&cx), "[object Object]");
        assert_eq!(Value::Number(-0.0).concat_string(&cx), "0");
        assert_eq!(Value::Number(1e21).concat_string(&cx), "1e+21");
    }

    #[test]
    fn numbers_of_values() {
        let cx = Context::default();
        assert_eq!(
            Value::Array(vec![Value::Number(3.0)]).to_number(&cx),
            Ok(3.0)
        );
        assert!(Value::Object(vec![]).to_number(&cx).unwrap().is_nan());
        assert_eq!(Value::Null.to_number(&cx), Ok(0.0));
        assert_eq!(Value::Bool(true).to_number(&cx), Ok(1.0));
        assert!(Value::BigInt(BigInt::parse("1").unwrap())
            .to_number(&cx)
            .is_err());
    }

    #[test]
    fn check_int32_accepts_only_integral_numbers() {
        let cx = Context::default();
        assert_eq!(check_int32(&cx, &Value::Number(5.0), 0, 8), Ok(5));
        assert_eq!(check_int32(&cx, &Value::Number(-0.0), 0, 8), Ok(0));
        let err = check_int32(&cx, &Value::Number(2.5), 0, 8).unwrap_err();
        assert_eq!(err.message, "[DecimalError] Invalid argument: 2.5");
        let err = check_int32(&cx, &Value::String("2".into()), 0, 8).unwrap_err();
        assert_eq!(err.message, "[DecimalError] Invalid argument: 2");
        let err = check_int32(&cx, &Value::Number(4294967298.0), 0, 8).unwrap_err();
        assert_eq!(err.message, "[DecimalError] Invalid argument: 4294967298");
    }
}
