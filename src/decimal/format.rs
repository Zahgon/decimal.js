//! Conversions of Decimals to strings and numbers, and the rounding methods that take a number of
//! digits or decimal places.

use super::arith::{divide, DivSpec};
use super::digits::{convert_base, digits_to_string, get_precision, numeral, zeros};
use super::parse::{from_number, parse_decimal};
use super::{word_exponent, Context, Decimal, LOG_BASE, MAX_DIGITS};
use crate::js::number;
use crate::js::value::check_int32;
use crate::js::{JsError, Value};

impl Decimal {
    /// `finiteToString(x, isExp, sd)`: the digits in normal or exponential notation, padded with
    /// zeros to `sd` significant digits, without the sign.
    pub(crate) fn finite_to_string(&self, is_exp: bool, sd: Option<i64>) -> String {
        let Some(d) = &self.d else {
            return self.non_finite_to_string();
        };
        let e = self.e;
        let mut str = digits_to_string(d);
        let len = str.len() as i64;
        let sd = sd.filter(|&sd| sd != 0);

        if is_exp {
            match sd {
                Some(sd) if sd - len > 0 => {
                    str = format!("{}.{}{}", &str[..1], &str[1..], zeros(sd - len));
                }
                _ if len > 1 => str = format!("{}.{}", &str[..1], &str[1..]),
                _ => {}
            }
            str.push_str(if e < 0 { "e" } else { "e+" });
            str.push_str(&e.to_string());
        } else if e < 0 {
            str = format!("0.{}{}", zeros(-e - 1), str);
            if let Some(sd) = sd {
                str.push_str(&zeros(sd - len));
            }
        } else if e >= len {
            str.push_str(&zeros(e + 1 - len));
            if let Some(sd) = sd {
                if sd - e - 1 > 0 {
                    str.push('.');
                    str.push_str(&zeros(sd - e - 1));
                }
            }
        } else {
            let k = e + 1;
            if k < len {
                str.insert(k as usize, '.');
            }
            if let Some(sd) = sd {
                if sd - len > 0 {
                    if e + 1 == len {
                        str.push('.');
                    }
                    str.push_str(&zeros(sd - len));
                }
            }
        }
        str
    }

    /// `nonFiniteToString(x)`: `NaN` or `Infinity`, unsigned.
    pub(crate) fn non_finite_to_string(&self) -> String {
        if self.s == 0 { "NaN" } else { "Infinity" }.to_string()
    }

    /// Whether `toString` uses exponential notation for this exponent.
    fn uses_exponent(&self, cx: &Context) -> bool {
        self.d.is_some() && (self.e <= cx.to_exp_neg || self.e >= cx.to_exp_pos)
    }

    /// `x.toString()`.
    pub fn to_js_string(&self, cx: &Context) -> String {
        let str = self.finite_to_string(self.uses_exponent(cx), None);
        if self.is_neg() && !self.is_zero() {
            format!("-{str}")
        } else {
            str
        }
    }

    /// `x.valueOf()`: like `toString`, but negative zero keeps its minus sign.
    pub fn value_of(&self, cx: &Context) -> String {
        let str = self.finite_to_string(self.uses_exponent(cx), None);
        if self.is_neg() {
            format!("-{str}")
        } else {
            str
        }
    }

    /// `x.toNumber()`.
    pub fn to_number(&self, cx: &Context) -> f64 {
        number::string_to_number(&self.value_of(cx))
    }

    /// The sign the formatting methods put in front: a minus for negative non-zero values.
    fn signed(&self, str: String) -> String {
        if self.is_neg() && !self.is_zero() {
            format!("-{str}")
        } else {
            str
        }
    }

    /// `x.toExponential(dp, rm)`.
    pub(crate) fn to_exponential(
        &self,
        cx: &Context,
        dp: &Value,
        rm: &Value,
    ) -> Result<String, JsError> {
        if dp.is_undefined() {
            return Ok(self.signed(self.finite_to_string(true, None)));
        }
        let dp = check_int32(cx, dp, 0, MAX_DIGITS)?;
        let rm = rounding_mode(cx, rm)?;
        let x = Decimal::copy_of(cx, self).round_to(cx, dp + 1, rm);
        let str = x.finite_to_string(true, Some(dp + 1));
        Ok(x.signed(str))
    }

    /// `x.toFixed(dp, rm)`.
    pub(crate) fn to_fixed(&self, cx: &Context, dp: &Value, rm: &Value) -> Result<String, JsError> {
        let str = if dp.is_undefined() {
            self.finite_to_string(false, None)
        } else {
            let dp = check_int32(cx, dp, 0, MAX_DIGITS)?;
            let rm = rounding_mode(cx, rm)?;
            if self.is_finite() {
                let y = Decimal::copy_of(cx, self).round_to(cx, dp + self.e + 1, rm);
                let sd = if y.is_finite() { dp + y.e + 1 } else { 0 };
                y.finite_to_string(false, Some(sd))
            } else {
                self.non_finite_to_string()
            }
        };
        // The minus sign depends on the value before it was rounded.
        Ok(self.signed(str))
    }

    /// `x.toPrecision(sd, rm)`.
    pub(crate) fn to_precision(
        &self,
        cx: &Context,
        sd: &Value,
        rm: &Value,
    ) -> Result<String, JsError> {
        if sd.is_undefined() {
            return Ok(self.signed(self.finite_to_string(self.uses_exponent(cx), None)));
        }
        let sd = check_int32(cx, sd, 1, MAX_DIGITS)?;
        let rm = rounding_mode(cx, rm)?;
        let x = Decimal::copy_of(cx, self).round_to(cx, sd, rm);
        let is_exp = x.is_finite() && (sd <= x.e || x.e <= cx.to_exp_neg);
        let str = x.finite_to_string(is_exp, Some(sd));
        Ok(x.signed(str))
    }

    /// `x.toSD(sd, rm)`.
    pub(crate) fn to_sd(&self, cx: &Context, sd: &Value, rm: &Value) -> Result<Decimal, JsError> {
        let (sd, rm) = if sd.is_undefined() {
            (cx.precision, cx.rounding)
        } else {
            let sd = check_int32(cx, sd, 1, MAX_DIGITS)?;
            (sd, rounding_mode(cx, rm)?)
        };
        Ok(Decimal::copy_of(cx, self).round_to(cx, sd, rm))
    }

    /// `x.toDP(dp, rm)`.
    pub(crate) fn to_dp(&self, cx: &Context, dp: &Value, rm: &Value) -> Result<Decimal, JsError> {
        let x = Decimal::copy_of(cx, self);
        if dp.is_undefined() {
            return Ok(x);
        }
        let dp = check_int32(cx, dp, 0, MAX_DIGITS)?;
        let rm = rounding_mode(cx, rm)?;
        if !x.is_finite() {
            return Ok(x);
        }
        let sd = dp + x.e + 1;
        Ok(x.round_to(cx, sd, rm))
    }

    /// `finalise(new Ctor(x), x.e + 1, rm)`: `round`, `ceil`, `floor` and `trunc`.
    pub(crate) fn to_integer(&self, cx: &Context, rm: i64) -> Decimal {
        let x = Decimal::copy_of(cx, self);
        if !self.is_finite() {
            return x;
        }
        let sd = self.e + 1;
        x.round_to(cx, sd, rm)
    }

    /// `x.dp()`: the number of decimal places, NaN for NaN and ±Infinity.
    pub(crate) fn decimal_places(&self) -> f64 {
        let Some(d) = &self.d else {
            return f64::NAN;
        };
        let w = d.len() as i64 - 1;
        let mut n = (w - word_exponent(self.e)) * LOG_BASE;
        let mut last = d[w as usize];
        if last != 0 {
            while last % 10 == 0 {
                last /= 10;
                n -= 1;
            }
        }
        n.max(0) as f64
    }

    /// `x.sd(z)`: the number of significant digits, counting integer trailing zeros if `z` is
    /// truthy.
    pub(crate) fn significant_digits(&self, cx: &Context, z: &Value) -> Result<f64, JsError> {
        let valid = match z {
            Value::Undefined | Value::Bool(_) => true,
            Value::Number(n) => *n == 1.0 || *n == 0.0,
            _ => false,
        };
        if !valid {
            return Err(JsError::invalid_argument(&z.concat_string(cx)));
        }
        let Some(d) = &self.d else {
            return Ok(f64::NAN);
        };
        let mut k = get_precision(d);
        if z.truthy() && self.e + 1 > k {
            k = self.e + 1;
        }
        Ok(k as f64)
    }

    /// The number of significant digits of a finite Decimal (`x.sd()`).
    pub(crate) fn sd_count(&self) -> i64 {
        self.d.as_deref().map_or(0, get_precision)
    }

    /// `x.isInteger()`.
    pub(crate) fn is_integer(&self) -> bool {
        match &self.d {
            Some(d) => word_exponent(self.e) > d.len() as i64 - 2,
            None => false,
        }
    }

    /// `x.toNearest(y, rm)`.
    pub(crate) fn to_nearest(
        &self,
        cx: &mut Context,
        y: &Value,
        rm: &Value,
    ) -> Result<Decimal, JsError> {
        let mut x = Decimal::copy_of(cx, self);
        let (mut y, rm) = if y.is_nullish() {
            if !x.is_finite() {
                return Ok(x);
            }
            (from_number(cx, 1.0), cx.rounding)
        } else {
            let mut y = super::new_decimal(cx, y)?;
            let rm = rounding_mode(cx, rm)?;
            if !x.is_finite() {
                return Ok(if y.s != 0 { x } else { y });
            }
            if !y.is_finite() {
                if y.s != 0 {
                    y.s = x.s;
                }
                return Ok(y);
            }
            (y, rm)
        };

        if y.first_word() != Some(0) {
            cx.external = false;
            x = divide(cx, &x, &y, DivSpec::dp(0, rm)).times(cx, &y);
            cx.external = true;
            x = x.checked(cx);
        } else {
            y.s = x.s;
            x = y;
        }
        Ok(x)
    }

    /// `x.toFraction(maxD)`: the closest fraction `[numerator, denominator]` with a denominator up
    /// to `maxD`.
    pub(crate) fn to_fraction(&self, cx: &mut Context, max_d: &Value) -> Result<Value, JsError> {
        let x = self;
        let Some(xd) = &x.d else {
            return Ok(Value::Decimal(Decimal::copy_of(cx, x)));
        };

        let mut n1 = from_number(cx, 1.0);
        let mut d0 = n1.clone();
        let mut d1 = from_number(cx, 0.0);
        let mut n0 = d1.clone();

        let mut d = Decimal::copy_of(cx, &d1);
        let e = get_precision(xd) - x.e - 1;
        d.e = e;
        let k = e % LOG_BASE;
        let words = d.d.get_or_insert_with(Vec::new);
        words[0] = 10i64.pow((if k < 0 { LOG_BASE + k } else { k }) as u32);

        let max_d = if max_d.is_nullish() {
            // d is 10**e, the minimum max-denominator needed.
            if e > 0 {
                d.clone()
            } else {
                n1.clone()
            }
        } else {
            let n = super::new_decimal(cx, max_d)?;
            if !n.is_integer() || n.lt(cx, &n1) {
                return Err(JsError::invalid_argument(&n.value_of(cx)));
            }
            if n.gt(cx, &d) {
                if e > 0 {
                    d.clone()
                } else {
                    n1.clone()
                }
            } else {
                n
            }
        };

        cx.external = false;
        let mut n = parse_decimal(cx, 1, &digits_to_string(xd));
        let pr = cx.precision;
        let wpr = xd.len() as i64 * LOG_BASE * 2;
        cx.precision = wpr;

        loop {
            let q = divide(cx, &n, &d, DivSpec::dp(0, 1));
            let d2 = d0.plus(cx, &q.times(cx, &d1));
            if d2.cmp_value(cx, &max_d) == 1.0 {
                break;
            }
            d0 = d1;
            d1 = d2;
            let d2 = n1;
            n1 = n0.plus(cx, &q.times(cx, &d2));
            n0 = d2;
            let d2 = d;
            d = n.minus(cx, &q.times(cx, &d2));
            n = d2;
        }

        let d2 = divide(cx, &max_d.minus(cx, &d0), &d1, DivSpec::dp(0, 1));
        n0 = n0.plus(cx, &d2.times(cx, &n1));
        d0 = d0.plus(cx, &d2.times(cx, &d1));
        n0.s = x.s;
        n1.s = x.s;

        // Which fraction is closer to x, n0/d0 or n1/d1?
        let r1 = divide(cx, &n1, &d1, DivSpec::sd(wpr, 1))
            .minus(cx, x)
            .abs(cx);
        let r0 = divide(cx, &n0, &d0, DivSpec::sd(wpr, 1))
            .minus(cx, x)
            .abs(cx);
        let r = if r1.cmp_value(cx, &r0) < 1.0 {
            [n1, d1]
        } else {
            [n0, d0]
        };

        cx.precision = pr;
        cx.external = true;

        Ok(Value::Array(r.into_iter().map(Value::Decimal).collect()))
    }

    /// `toStringBinary(x, baseOut, sd, rm)`: `toBinary`, `toOctal` and `toHexadecimal`.
    pub(crate) fn to_string_binary(
        &self,
        cx: &mut Context,
        base_out: i64,
        sd: &Value,
        rm: &Value,
    ) -> Result<String, JsError> {
        let is_exp = !sd.is_undefined();
        let (mut sd, rm) = if is_exp {
            let sd = check_int32(cx, sd, 1, MAX_DIGITS)?;
            (sd, rounding_mode(cx, rm)?)
        } else {
            (cx.precision, cx.rounding)
        };

        let x = self;
        let mut str;
        if !x.is_finite() {
            str = x.non_finite_to_string();
        } else {
            str = x.finite_to_string(false, None);
            let dot = str.find('.');

            let base = if is_exp {
                if base_out == 16 {
                    sd = sd * 4 - 3;
                } else if base_out == 8 {
                    sd = sd * 3 - 2;
                }
                2
            } else {
                base_out
            };

            // Convert the number as an integer, then divide by the base raised to the number of
            // fraction digits.
            let mut y = None;
            if let Some(at) = dot {
                str.remove(at);
                let mut one = from_number(cx, 1.0);
                one.e = str.len() as i64 - at as i64;
                let digits = convert_base(&one.finite_to_string(false, None), 10, base);
                one.e = digits.len() as i64;
                one.d = Some(digits);
                y = Some(one);
            }

            let mut xd = convert_base(&str, 10, base);
            let mut e = xd.len() as i64;
            while xd.len() > 1 && xd.last() == Some(&0) {
                xd.pop();
            }

            if xd[0] == 0 {
                str = if is_exp { "0p+0" } else { "0" }.to_string();
            } else {
                let mut round_up = false;
                match y {
                    None => e -= 1,
                    Some(y) => {
                        let mut t = Decimal::copy_of(cx, x);
                        t.d = Some(xd);
                        t.e = e;
                        let q = divide(
                            cx,
                            &t,
                            &y,
                            DivSpec {
                                pr: Some(sd),
                                rm,
                                dp: false,
                                base: Some(base),
                            },
                        );
                        xd = q.d.unwrap_or_default();
                        e = q.e;
                        round_up = cx.inexact;
                    }
                }

                str = binary_digits(&mut xd, &mut e, sd, rm, x.s, base, round_up);

                if is_exp {
                    if str.len() > 1 {
                        if base_out == 16 || base_out == 8 {
                            let group = if base_out == 16 { 4 } else { 3 };
                            let mut len = str.len() - 1;
                            while len % group != 0 {
                                str.push('0');
                                len += 1;
                            }
                            let out = convert_base(&str, base, base_out);
                            let mut len = out.len();
                            while len > 0 && out[len - 1] == 0 {
                                len -= 1;
                            }
                            // out[0] is always 1
                            str = String::from("1.");
                            for &digit in &out[1..len.max(1)] {
                                str.push_str(numeral(digit));
                            }
                        } else {
                            str.insert(1, '.');
                        }
                    }
                    str.push_str(if e < 0 { "p" } else { "p+" });
                    str.push_str(&e.to_string());
                } else if e < 0 {
                    while {
                        e += 1;
                        e != 0
                    } {
                        str.insert(0, '0');
                    }
                    str.insert_str(0, "0.");
                } else {
                    e += 1;
                    let len = str.len() as i64;
                    if e > len {
                        str.push_str(&zeros(e - len));
                    } else if e < len {
                        str.insert(e as usize, '.');
                    }
                }
            }

            let prefix = match base_out {
                16 => "0x",
                2 => "0b",
                8 => "0o",
                _ => "",
            };
            str.insert_str(0, prefix);
        }

        Ok(if x.s < 0 { format!("-{str}") } else { str })
    }
}

/// Round the digits of a base conversion to `sd` digits and spell them, updating the exponent when
/// rounding carries into a new leading digit.
fn binary_digits(
    xd: &mut Vec<i64>,
    e: &mut i64,
    sd: i64,
    rm: i64,
    sign: i8,
    base: i64,
    round_up: bool,
) -> String {
    let sd_at = sd as usize;
    // The rounding digit, i.e. the digit after the digit that may be rounded up.
    let i = xd.get(sd_at).copied();
    let k = base / 2;
    let round_up = round_up || xd.get(sd_at + 1).is_some();

    let round_up = if rm < 4 {
        (i.is_some() || round_up) && (rm == 0 || rm == if sign < 0 { 3 } else { 2 })
    } else {
        let i = i.unwrap_or(-1);
        i > k
            || i == k
                && (rm == 4
                    || round_up
                    || rm == 6 && sd_at >= 1 && xd.get(sd_at - 1).is_some_and(|d| d & 1 == 1)
                    || rm == if sign < 0 { 8 } else { 7 })
    };

    if xd.len() > sd_at {
        xd.truncate(sd_at);
    }

    if round_up {
        // Rounding up may mean the previous digit has to be rounded up and so on.
        let mut at = sd_at;
        loop {
            if at == 0 {
                break;
            }
            at -= 1;
            if at >= xd.len() {
                break;
            }
            xd[at] += 1;
            if xd[at] < base {
                break;
            }
            xd[at] = 0;
            if at == 0 {
                *e += 1;
                xd.insert(0, 1);
                break;
            }
        }
    }

    let mut len = xd.len();
    while len > 0 && xd[len - 1] == 0 {
        len -= 1;
    }

    // E.g. [4, 11, 15] becomes 4bf.
    xd[..len].iter().map(|&d| numeral(d)).collect()
}

/// The rounding mode argument of a formatting method: `rounding` if omitted, else an integer from 0
/// to 8.
pub(crate) fn rounding_mode(cx: &Context, rm: &Value) -> Result<i64, JsError> {
    if rm.is_undefined() {
        Ok(cx.rounding)
    } else {
        check_int32(cx, rm, 0, 8)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decimal::parse::parse_str;

    fn d(cx: &mut Context, s: &str) -> Decimal {
        parse_str(cx, s).unwrap()
    }

    fn n(v: f64) -> Value {
        Value::Number(v)
    }

    #[test]
    fn fixed_and_exponential() {
        let mut cx = Context::default();
        let x = d(&mut cx, "-0.00001");
        assert_eq!(x.to_fixed(&cx, &n(0.0), &Value::Undefined).unwrap(), "-0");
        let x = d(&mut cx, "123.456");
        assert_eq!(
            x.to_fixed(&cx, &n(5.0), &Value::Undefined).unwrap(),
            "123.45600"
        );
        assert_eq!(x.to_fixed(&cx, &n(1.0), &n(1.0)).unwrap(), "123.4");
        assert_eq!(
            x.to_exponential(&cx, &n(2.0), &Value::Undefined).unwrap(),
            "1.23e+2"
        );
        assert_eq!(
            x.to_exponential(&cx, &Value::Undefined, &Value::Undefined)
                .unwrap(),
            "1.23456e+2"
        );
        assert_eq!(
            x.to_precision(&cx, &n(2.0), &Value::Undefined).unwrap(),
            "1.2e+2"
        );
        assert_eq!(
            x.to_precision(&cx, &n(8.0), &Value::Undefined).unwrap(),
            "123.45600"
        );
        let z = d(&mut cx, "0");
        assert_eq!(
            z.to_fixed(&cx, &n(3.0), &Value::Undefined).unwrap(),
            "0.000"
        );
    }

    #[test]
    fn binary_strings() {
        let mut cx = Context::default();
        let x = d(&mut cx, "255.5");
        let u = Value::Undefined;
        assert_eq!(x.to_string_binary(&mut cx, 16, &u, &u).unwrap(), "0xff.8");
        assert_eq!(
            x.to_string_binary(&mut cx, 2, &u, &u).unwrap(),
            "0b11111111.1"
        );
        assert_eq!(x.to_string_binary(&mut cx, 8, &u, &u).unwrap(), "0o377.4");
        assert_eq!(
            x.to_string_binary(&mut cx, 16, &n(1.0), &u).unwrap(),
            "0x1p+8"
        );
        let y = d(&mut cx, "-0.1");
        assert_eq!(
            y.to_string_binary(&mut cx, 2, &n(5.0), &u).unwrap(),
            "-0b1.101p-4"
        );
    }

    #[test]
    fn fractions() {
        let mut cx = Context::default();
        let x = d(&mut cx, "0.75");
        let f = x.to_fraction(&mut cx, &Value::Undefined).unwrap();
        assert_eq!(f.to_js_string(&cx), "3,4");
        let pi = d(&mut cx, "3.14159265358979323846");
        let f = pi.to_fraction(&mut cx, &n(1000.0)).unwrap();
        assert_eq!(f.to_js_string(&cx), "355,113");
    }

    #[test]
    fn nearest_multiples() {
        let mut cx = Context::default();
        let x = d(&mut cx, "1.39");
        let r = x
            .to_nearest(&mut cx, &Value::String("0.25".into()), &Value::Undefined)
            .unwrap();
        assert_eq!(r.to_js_string(&cx), "1.5");
        let r = x
            .to_nearest(&mut cx, &Value::String("0.25".into()), &n(1.0))
            .unwrap();
        assert_eq!(r.to_js_string(&cx), "1.25");
    }
}
