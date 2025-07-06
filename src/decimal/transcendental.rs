//! Square and cube roots, the natural exponential and logarithm, logarithms to other bases, and
//! powers.

use super::arith::{divide, int_pow, DivSpec, Operand};
use super::digits::{check_rounding_digits, digits_to_string};
use super::parse::{from_number, parse_decimal, parse_numeric};
use super::{word_exponent, Context, Decimal, LN10_PRECISION, MAX_SAFE_INTEGER};
use crate::js::{number, JsError};

/// `str.slice(start, end)` for the ASCII digit strings of Decimals.
pub(crate) fn js_slice(s: &str, start: i64, end: i64) -> &str {
    let len = s.len() as i64;
    let clamp = |i: i64| if i < 0 { (len + i).max(0) } else { i.min(len) };
    let (a, b) = (clamp(start), clamp(end));
    if a >= b {
        ""
    } else {
        &s[a as usize..b as usize]
    }
}

/// `+digitsToString(d).slice(a, b) + 1 == 1e14`: whether the 14 digits from index `a` are all 9.
fn fourteen_nines(d: &[i64], a: i64) -> bool {
    let digits = digits_to_string(d);
    number::string_to_number(js_slice(&digits, a, a + 14)) + 1.0 == 1e14
}

/// `getLn10(Ctor, sd, pr)`: ln(10) to `sd` significant digits, rounded down.
pub(crate) fn get_ln10(cx: &mut Context, sd: i64, pr: Option<i64>) -> Result<Decimal, JsError> {
    if sd > LN10_PRECISION {
        // Reset global state in case the exception is caught.
        cx.external = true;
        if let Some(pr) = pr.filter(|&pr| pr != 0) {
            cx.precision = pr;
        }
        return Err(JsError::precision_limit_exceeded());
    }
    let ln10 = Decimal::copy_of(cx, &cx.ln10);
    Ok(ln10.finalised(cx, Some(sd), 1, true))
}

/// `getPi(Ctor, sd, rm)`: pi to `sd` significant digits using rounding mode `rm`.
pub(crate) fn get_pi(cx: &Context, sd: i64, rm: i64) -> Result<Decimal, JsError> {
    if sd > super::PI_PRECISION {
        return Err(JsError::precision_limit_exceeded());
    }
    let pi = Decimal::copy_of(cx, &cx.pi);
    Ok(pi.finalised(cx, Some(sd), rm, true))
}

/// `naturalExponential(x, sd)`: e^x, rounded to `precision` if `sd` is `None`, else to `sd`
/// significant digits (with rounding digits, unrounded).
pub(crate) fn natural_exponential(cx: &mut Context, x: &Decimal, sd: Option<i64>) -> Decimal {
    let rm = cx.rounding;
    let pr = cx.precision;

    let finite_nonzero = matches!(&x.d, Some(d) if d[0] != 0);
    if !finite_nonzero || x.e > 17 {
        return match &x.d {
            Some(d) if d[0] == 0 => from_number(cx, 1.0),
            Some(_) if x.s < 0 => from_number(cx, 0.0),
            Some(_) => Decimal::infinity(1),
            None if x.s == 0 => Decimal::nan(),
            None if x.s < 0 => from_number(cx, 0.0),
            None => Decimal::copy_of(cx, x),
        };
    }

    let mut wpr = match sd {
        None => {
            cx.external = false;
            pr
        }
        Some(sd) => sd,
    };

    let t = from_number(cx, 0.03125);
    let mut x = x.clone();
    let mut k: i64 = 0;

    // while abs(x) >= 0.1, x = x / 2^5
    while x.e > -2 {
        x = x.times(cx, &t);
        k += 5;
    }

    // 2 * log10(2^k) + 5 (empirically derived) estimates the increase in precision necessary to
    // ensure the first 4 rounding digits are correct.
    let guard =
        number::to_int32(number::log(2f64.powi(k as i32)) / number::LN10 * 2.0 + 5.0) as i64;
    wpr += guard;
    let mut denominator = from_number(cx, 1.0);
    let mut pow = denominator.clone();
    let mut sum = denominator.clone();
    cx.precision = wpr;
    let mut rep: i64 = 0;
    let mut i: i64 = 0;

    loop {
        pow = pow.times(cx, &x).round_to(cx, wpr, 1);
        i += 1;
        denominator = denominator.times_number(cx, i as f64);
        let term = divide(cx, &pow, &denominator, DivSpec::sd(wpr, 1));
        let mut t = sum.plus(cx, &term);

        let t_digits = digits_to_string(t.words());
        let sum_digits = digits_to_string(sum.words());
        if js_slice(&t_digits, 0, wpr) == js_slice(&sum_digits, 0, wpr) {
            let mut j = k;
            while j > 0 {
                j -= 1;
                sum = sum.times(cx, &sum).round_to(cx, wpr, 1);
            }

            // If the first 4 rounding digits are [49]999, repeat the summation with a higher
            // precision. `wpr - guard` is the index of the first rounding digit.
            match sd {
                None => {
                    if rep < 3
                        && check_rounding_digits(sum.words(), wpr - guard, rm, Some(rep != 0))
                    {
                        wpr += 10;
                        cx.precision = wpr;
                        denominator = from_number(cx, 1.0);
                        pow = denominator.clone();
                        t = denominator.clone();
                        i = 0;
                        rep += 1;
                    } else {
                        cx.precision = pr;
                        cx.external = true;
                        return sum.finalised(cx, Some(pr), rm, true);
                    }
                }
                Some(_) => {
                    cx.precision = pr;
                    return sum;
                }
            }
        }

        sum = t;
    }
}

/// `naturalLogarithm(y, sd)`: ln(y), rounded to `precision` if `sd` is `None`, else to `sd`
/// significant digits (with rounding digits, unrounded).
pub(crate) fn natural_logarithm(
    cx: &mut Context,
    y: &Decimal,
    sd: Option<i64>,
) -> Result<Decimal, JsError> {
    let guard: i64 = 10;
    let rm = cx.rounding;
    let pr = cx.precision;

    // Is y negative or Infinity, NaN, 0 or 1?
    let xd = match &y.d {
        Some(d) if y.s > 0 && d[0] != 0 && !(y.e == 0 && d[0] == 1 && d.len() == 1) => d.clone(),
        _ => {
            return Ok(match &y.d {
                Some(d) if d[0] == 0 => Decimal::infinity(-1),
                _ if y.s != 1 => Decimal::nan(),
                Some(_) => from_number(cx, 0.0),
                None => Decimal::copy_of(cx, y),
            });
        }
    };

    let mut wpr = match sd {
        None => {
            cx.external = false;
            pr
        }
        Some(sd) => sd,
    };

    wpr += guard;
    cx.precision = wpr;
    let mut c = digits_to_string(&xd);
    let mut c0 = c.as_bytes()[0];
    let mut x = y.clone();
    let mut n: i64 = 1;
    let mut e = y.e;

    if (e as f64).abs() < 1.5e15 {
        // Multiply the argument by itself until the leading digits of the significand are 7, 8,
        // 9, 10, 11, 12 or 13, then separate out the power of 10.
        let second = |c: &str| c.as_bytes().get(1).map_or(0, |b| b - b'0');
        while c0 < b'7' && c0 != b'1' || c0 == b'1' && second(&c) > 3 {
            x = x.times(cx, y);
            c = digits_to_string(x.words());
            c0 = c.as_bytes()[0];
            n += 1;
        }

        e = x.e;

        if c0 > b'1' {
            x = parse_decimal(cx, 1, &format!("0.{c}"));
            e += 1;
        } else {
            x = parse_decimal(cx, 1, &format!("{}.{}", c0 as char, &c[1..]));
        }
    } else {
        // The reduction above may overflow for a massive exponent, so instead use
        // ln(x*10^e) = ln(x) + e*ln(10).
        let t = get_ln10(cx, wpr + 2, Some(pr))?.times(cx, &from_number(cx, e as f64));
        let reduced = parse_decimal(cx, 1, &format!("{}.{}", c0 as char, &c[1..]));
        let x = natural_logarithm(cx, &reduced, Some(wpr - guard))?.plus(cx, &t);
        cx.precision = pr;
        return Ok(match sd {
            None => {
                cx.external = true;
                x.finalised(cx, Some(pr), rm, true)
            }
            Some(_) => x,
        });
    }

    // x1 is x reduced to a value near 1.
    let x1 = x.clone();

    // ln(y) = ln((1 + x)/(1 - x)) = 2(x + x^3/3 + x^5/5 + x^7/7 + ...), x = (y - 1)/(y + 1)
    let one = from_number(cx, 1.0);
    let mut xr = divide(
        cx,
        &x.minus(cx, &one),
        &x.plus(cx, &one),
        DivSpec::sd(wpr, 1),
    );
    let mut sum = xr.clone();
    let mut numerator = xr.clone();
    let mut x2 = xr.times(cx, &xr).round_to(cx, wpr, 1);
    let mut denominator: i64 = 3;
    let mut rep: Option<bool> = None;

    loop {
        numerator = numerator.times(cx, &x2).round_to(cx, wpr, 1);
        let den = from_number(cx, denominator as f64);
        let term = divide(cx, &numerator, &den, DivSpec::sd(wpr, 1));
        let mut t = sum.plus(cx, &term);

        let t_digits = digits_to_string(t.words());
        let sum_digits = digits_to_string(sum.words());
        if js_slice(&t_digits, 0, wpr) == js_slice(&sum_digits, 0, wpr) {
            sum = sum.times_number(cx, 2.0);

            // Reverse the argument reduction. e is checked to be non-zero because -0 + 0 = +0 and
            // -0 needs to stay -0 for correct rounding.
            if e != 0 {
                let ln10 = get_ln10(cx, wpr + 2, Some(pr))?;
                sum = sum.plus(cx, &ln10.times(cx, &from_number(cx, e as f64)));
            }
            sum = divide(cx, &sum, &from_number(cx, n as f64), DivSpec::sd(wpr, 1));

            // If the first 4 rounding digits are [49]999 (or 9999 after a repeat), restart the
            // summation with a higher precision. `wpr - guard` is the index of the first rounding
            // digit.
            match sd {
                None => {
                    if check_rounding_digits(sum.words(), wpr - guard, rm, rep) {
                        wpr += guard;
                        cx.precision = wpr;
                        xr = divide(
                            cx,
                            &x1.minus(cx, &one),
                            &x1.plus(cx, &one),
                            DivSpec::sd(wpr, 1),
                        );
                        t = xr.clone();
                        numerator = xr.clone();
                        x2 = xr.times(cx, &xr).round_to(cx, wpr, 1);
                        denominator = 1;
                        rep = Some(true);
                    } else {
                        cx.precision = pr;
                        cx.external = true;
                        return Ok(sum.finalised(cx, Some(pr), rm, true));
                    }
                }
                Some(_) => {
                    cx.precision = pr;
                    return Ok(sum);
                }
            }
        }

        sum = t;
        denominator += 2;
    }
}

impl Decimal {
    /// `x.exp()`.
    pub(crate) fn exp(&self, cx: &mut Context) -> Decimal {
        natural_exponential(cx, self, None)
    }

    /// `x.ln()`.
    pub(crate) fn ln(&self, cx: &mut Context) -> Result<Decimal, JsError> {
        natural_logarithm(cx, self, None)
    }

    /// `x.log(base)`: the logarithm to `base` (10 if `None`), `base` being `new Ctor(base)`.
    pub(crate) fn log(&self, cx: &mut Context, base: Option<Decimal>) -> Result<Decimal, JsError> {
        let arg = self;
        let pr = cx.precision;
        let rm = cx.rounding;
        let guard = 5;

        let (base, is_base10) = match base {
            None => (from_number(cx, 10.0), true),
            Some(base) => {
                let one = from_number(cx, 1.0);
                if base.s < 0 || base.first_word().is_none_or(|w| w == 0) || base.eq_value(cx, &one)
                {
                    return Ok(Decimal::nan());
                }
                let ten = from_number(cx, 10.0);
                let is_base10 = base.eq_value(cx, &ten);
                (base, is_base10)
            }
        };

        let one = from_number(cx, 1.0);
        let d = match &arg.d {
            Some(d) if arg.s > 0 && d[0] != 0 && !arg.eq_value(cx, &one) => d.clone(),
            _ => {
                return Ok(match &arg.d {
                    Some(d) if d[0] == 0 => Decimal::infinity(-1),
                    _ if arg.s != 1 => Decimal::nan(),
                    Some(_) => from_number(cx, 0.0),
                    None => Decimal::infinity(1),
                });
            }
        };

        // The result has a non-terminating decimal expansion if the base is 10 and arg is not an
        // integer power of 10.
        let mut inf = false;
        if is_base10 {
            if d.len() > 1 {
                inf = true;
            } else {
                let mut k = d[0];
                while k % 10 == 0 {
                    k /= 10;
                }
                inf = k != 1;
            }
        }

        cx.external = false;
        let mut sd = pr + guard;
        let num = natural_logarithm(cx, arg, Some(sd))?;
        let denominator = if is_base10 {
            get_ln10(cx, sd + 10, None)?
        } else {
            natural_logarithm(cx, &base, Some(sd))?
        };

        // The result will have 5 rounding digits.
        let mut r = divide(cx, &num, &denominator, DivSpec::sd(sd, 1));

        // At a rounding boundary ([49]9999 or [50]0000), calculate 10 further digits; if the
        // expansion is infinite, repeat until it is clear that the result is above or below the
        // boundary.
        let mut k = pr;
        if check_rounding_digits(r.words(), k, rm, None) {
            loop {
                sd += 10;
                let num = natural_logarithm(cx, arg, Some(sd))?;
                let denominator = if is_base10 {
                    get_ln10(cx, sd + 10, None)?
                } else {
                    natural_logarithm(cx, &base, Some(sd))?
                };
                r = divide(cx, &num, &denominator, DivSpec::sd(sd, 1));

                if !inf {
                    // Check for 14 nines from the 2nd rounding digit, as the first may be 4.
                    if fourteen_nines(r.words(), k + 1) {
                        r = r.round_to(cx, pr + 1, 0);
                    }
                    break;
                }

                k += 10;
                if !check_rounding_digits(r.words(), k, rm, None) {
                    break;
                }
            }
        }

        cx.external = true;

        Ok(r.round_to(cx, pr, rm))
    }

    /// `x.sqrt()`.
    pub(crate) fn sqrt(&self, cx: &mut Context) -> Decimal {
        let x = self;
        let d = match &x.d {
            Some(d) if x.s == 1 && d[0] != 0 => d.clone(),
            _ => {
                return match &x.d {
                    _ if x.s == 0 => Decimal::nan(),
                    None if x.s < 0 => Decimal::nan(),
                    Some(d) if x.s < 0 && d[0] != 0 => Decimal::nan(),
                    Some(_) => Decimal::copy_of(cx, x),
                    None => Decimal::infinity(1),
                };
            }
        };
        let mut e = x.e;

        cx.external = false;

        // Initial estimate.
        let s = number::sqrt(x.to_number(cx));

        // Math.sqrt underflow/overflow? Pass x to Math.sqrt as an integer, then adjust the
        // exponent of the result.
        let mut r = if s == 0.0 || s == f64::INFINITY {
            let mut n = digits_to_string(&d);
            if (n.len() as i64 + e) % 2 == 0 {
                n.push('0');
            }
            let s = number::sqrt(number::string_to_number(&n));
            e = (e + 1).div_euclid(2) - i64::from(e < 0 || e % 2 != 0);
            let text = if s == f64::INFINITY {
                format!("5e{e}")
            } else {
                let n = number::to_exponential(s);
                format!("{}{}", &n[..n.find('e').map_or(0, |i| i + 1)], e)
            };
            parse_numeric(cx, &text)
        } else {
            parse_numeric(cx, &number::to_string(s))
        };

        let e = cx.precision;
        let mut sd = e + 3;
        let mut rep = false;
        let mut m = false;

        // Newton-Raphson iteration.
        loop {
            let mut t = r;
            let quotient = divide(cx, x, &t, DivSpec::sd(sd + 2, 1));
            r = t.plus(cx, &quotient).times_number(cx, 0.5);

            let n = digits_to_string(r.words());
            if js_slice(&digits_to_string(t.words()), 0, sd) == js_slice(&n, 0, sd) {
                let n = js_slice(&n, sd - 3, sd + 1).to_string();

                // The 4th rounding digit may be in error by -1, so if the 4 rounding digits are
                // 9999 or 4999, i.e. approaching a rounding boundary, continue the iteration.
                if n == "9999" || !rep && n == "4999" {
                    // On the first iteration only, check whether rounding up gives the exact
                    // result, as the nines may repeat infinitely.
                    if !rep {
                        t.finalise_mut(cx, Some(e + 1), 0, false);
                        if t.times(cx, &t).eq_value(cx, x) {
                            r = t;
                            break;
                        }
                    }
                    sd += 4;
                    rep = true;
                } else {
                    // If the rounding digits are null, 0{0,4} or 50{0,3}, check for an exact
                    // result; if it is not exact, there are further digits.
                    if is_zero_text(&n) || is_zero_text(js_slice(&n, 1, 4)) && n.starts_with('5') {
                        // Truncate to the first rounding digit.
                        r.finalise_mut(cx, Some(e + 1), 1, false);
                        m = !r.times(cx, &r).eq_value(cx, x);
                    }
                    break;
                }
            }
        }

        cx.external = true;

        r.finalised(cx, Some(e), cx.rounding, m)
    }

    /// `x.cbrt()`.
    pub(crate) fn cbrt(&self, cx: &mut Context) -> Decimal {
        let x = self;
        let Some(d) = x.d.clone().filter(|d| d[0] != 0) else {
            return Decimal::copy_of(cx, x);
        };
        cx.external = false;

        // Initial estimate.
        let sign = x.sign_number();
        let mut s = sign * number::pow(sign * x.to_number(cx), 1.0 / 3.0);

        // Math.cbrt underflow/overflow? Pass x to Math.pow as an integer, then adjust the
        // exponent of the result.
        let mut r = if s == 0.0 || s.is_nan() || s.abs() == f64::INFINITY {
            let mut n = digits_to_string(&d);
            let mut e = x.e;

            // Adjust the exponent of n so it is a multiple of 3 away from the exponent of x.
            let rem = (e - n.len() as i64 + 1) % 3;
            if rem != 0 {
                n.push_str(if rem == 1 || rem == -2 { "0" } else { "00" });
            }
            s = number::pow(number::string_to_number(&n), 1.0 / 3.0);

            // Rarely, e may be one less than the result exponent value.
            e = (e + 1).div_euclid(3) - i64::from(e % 3 == if e < 0 { -1 } else { 2 });

            let text = if s == f64::INFINITY {
                format!("5e{e}")
            } else {
                let n = number::to_exponential(s);
                format!("{}{}", &n[..n.find('e').map_or(0, |i| i + 1)], e)
            };
            let mut r = parse_numeric(cx, &text);
            r.s = x.s;
            r
        } else {
            parse_numeric(cx, &number::to_string(s))
        };

        let e = cx.precision;
        let mut sd = e + 3;
        let mut rep = false;
        let mut m = false;

        // Halley's method.
        loop {
            let mut t = r;
            let t3 = t.times(cx, &t).times(cx, &t);
            let t3plusx = t3.plus(cx, x);
            let numerator = t3plusx.plus(cx, x).times(cx, &t);
            let denominator = t3plusx.plus(cx, &t3);
            r = divide(cx, &numerator, &denominator, DivSpec::sd(sd + 2, 1));

            let n = digits_to_string(r.words());
            if js_slice(&digits_to_string(t.words()), 0, sd) == js_slice(&n, 0, sd) {
                let n = js_slice(&n, sd - 3, sd + 1).to_string();

                // The 4th rounding digit may be in error by -1, so if the 4 rounding digits are
                // 9999 or 4999, i.e. approaching a rounding boundary, continue the iteration.
                if n == "9999" || !rep && n == "4999" {
                    // On the first iteration only, check whether rounding up gives the exact
                    // result, as the nines may repeat infinitely.
                    if !rep {
                        t.finalise_mut(cx, Some(e + 1), 0, false);
                        if t.times(cx, &t).times(cx, &t).eq_value(cx, x) {
                            r = t;
                            break;
                        }
                    }
                    sd += 4;
                    rep = true;
                } else {
                    // If the rounding digits are null, 0{0,4} or 50{0,3}, check for an exact
                    // result; if it is not exact, there are further digits.
                    if is_zero_text(&n) || is_zero_text(js_slice(&n, 1, 4)) && n.starts_with('5') {
                        // Truncate to the first rounding digit.
                        r.finalise_mut(cx, Some(e + 1), 1, false);
                        m = !r.times(cx, &r).times(cx, &r).eq_value(cx, x);
                    }
                    break;
                }
            }
        }

        cx.external = true;

        r.finalised(cx, Some(e), cx.rounding, m)
    }

    /// `x.pow(y)`.
    pub(crate) fn pow(&self, cx: &mut Context, y: impl Operand) -> Result<Decimal, JsError> {
        let y = y.convert(cx);
        let yn = y.to_number(cx);

        // Either ±Infinity, NaN or ±0?
        let (Some(_), Some(yd)) = (self.d.as_ref().filter(|d| d[0] != 0), y.d.clone()) else {
            return Ok(from_number(cx, number::pow(self.to_number(cx), yn)));
        };
        if yd[0] == 0 {
            return Ok(from_number(cx, number::pow(self.to_number(cx), yn)));
        }

        let mut x = Decimal::copy_of(cx, self);
        let one = from_number(cx, 1.0);

        if x.eq_value(cx, &one) {
            return Ok(x);
        }

        let pr = cx.precision;
        let rm = cx.rounding;

        if y.eq_value(cx, &one) {
            return Ok(x.round_to(cx, pr, rm));
        }

        // y exponent
        let e = word_exponent(y.e);

        // If y is a small integer use the 'exponentiation by squaring' algorithm.
        let k = if yn < 0.0 { -yn } else { yn };
        if e >= yd.len() as i64 - 1 && k <= MAX_SAFE_INTEGER {
            let r = int_pow(cx, &x, k, pr as f64)?;
            return Ok(if y.s < 0 {
                from_number(cx, 1.0).div(cx, &r)
            } else {
                r.round_to(cx, pr, rm)
            });
        }

        let mut s = x.s;

        // if x is negative
        if s < 0 {
            // if y is not an integer
            if e < yd.len() as i64 - 1 {
                return Ok(Decimal::nan());
            }

            // The result is positive if x is negative and the last digit of integer y is even.
            if yd.get(e as usize).copied().unwrap_or(0) & 1 == 0 {
                s = 1;
            }

            // if x.eq(-1)
            if x.e == 0 && x.words() == [1] {
                x.s = s;
                return Ok(x);
            }
        }

        // Estimate the result exponent: x^y = 10^e, where e = y * log10(x).
        let k = number::pow(x.to_number(cx), yn);
        let e: f64 = if k == 0.0 || !k.is_finite() {
            let words = x.words_or_null_error()?;
            let significand = number::string_to_number(&format!("0.{}", digits_to_string(words)));
            (yn * (number::log(significand) / number::LN10 + x.e as f64 + 1.0)).floor()
        } else {
            let estimate = parse_numeric(cx, &number::to_string(k));
            if estimate.is_finite() {
                estimate.e as f64
            } else {
                f64::NAN
            }
        };

        // Overflow/underflow?
        if e > cx.max_e as f64 + 1.0 || e < cx.min_e as f64 - 1.0 {
            return Ok(if e > 0.0 {
                Decimal::infinity(s)
            } else {
                from_number(cx, 0.0)
            });
        }

        cx.external = false;
        x.s = 1;
        cx.rounding = 1;

        // Extra guard digits ensure five correct rounding digits from naturalLogarithm(x).
        let k = (number::to_string(e).len() as i64).min(12);

        // r = x^y = exp(y*ln(x))
        let ln = natural_logarithm(cx, &x, Some(pr + k))?;
        let product = y.times(cx, &ln);
        let mut r = natural_exponential(cx, &product, Some(pr));

        // r may be Infinity, e.g. (0.9999999999999999).pow(-1e+40)
        if r.is_finite() {
            // Truncate to the required precision plus five rounding digits.
            r = r.round_to(cx, pr + 5, 1);

            // If the rounding digits are [49]9999 or [50]0000, increase the precision by 10 and
            // recalculate the result.
            if check_rounding_digits(r.words(), pr, rm, None) {
                let e = pr + 10;

                // Truncate to the increased precision plus five rounding digits.
                let ln = natural_logarithm(cx, &x, Some(e + k))?;
                let product = y.times(cx, &ln);
                r = natural_exponential(cx, &product, Some(e)).round_to(cx, e + 5, 1);

                // Check for 14 nines from the 2nd rounding digit (the first may be 4 or 9).
                if fourteen_nines(r.words(), pr + 1) {
                    r = r.round_to(cx, pr + 1, 0);
                }
            }
        }

        r.s = s;
        cx.external = true;
        cx.rounding = rm;

        Ok(r.round_to(cx, pr, rm))
    }
}

/// `!+text`: whether a string of digits (possibly empty) has the value zero.
fn is_zero_text(text: &str) -> bool {
    text.bytes().all(|b| b == b'0')
}
#[cfg(test)]
mod tests {
    use super::super::parse::parse_str;
    use super::*;

    fn d(cx: &mut Context, s: &str) -> Decimal {
        parse_str(cx, s).unwrap()
    }

    #[test]
    fn roots() {
        let mut cx = Context::default();
        let two = d(&mut cx, "2");
        assert_eq!(two.sqrt(&mut cx).to_js_string(&cx), "1.4142135623730950488");
        let x = d(&mut cx, "1e-400");
        assert_eq!(x.sqrt(&mut cx).to_js_string(&cx), "1e-200");
        let x = d(&mut cx, "-27");
        assert_eq!(x.cbrt(&mut cx).to_js_string(&cx), "-3");
        assert!(cx.external);
    }

    #[test]
    fn exponentials_and_logarithms() {
        let mut cx = Context::default();
        let one = d(&mut cx, "1");
        assert_eq!(one.exp(&mut cx).to_js_string(&cx), "2.7182818284590452354");
        let ten = d(&mut cx, "10");
        assert_eq!(
            ten.ln(&mut cx).unwrap().to_js_string(&cx),
            "2.302585092994045684"
        );
        let x = d(&mut cx, "1000");
        assert_eq!(x.log(&mut cx, None).unwrap().to_js_string(&cx), "3");
        let two = d(&mut cx, "2");
        let x = d(&mut cx, "1024");
        assert_eq!(x.log(&mut cx, Some(two)).unwrap().to_js_string(&cx), "10");
    }

    #[test]
    fn powers() {
        let mut cx = Context::default();
        let two = d(&mut cx, "2");
        let half = d(&mut cx, "0.5");
        assert_eq!(
            two.pow(&mut cx, &half).unwrap().to_js_string(&cx),
            "1.4142135623730950488"
        );
        let neg = d(&mut cx, "-2");
        let three = d(&mut cx, "3");
        assert_eq!(neg.pow(&mut cx, &three).unwrap().to_js_string(&cx), "-8");
        let m = d(&mut cx, "-3");
        assert_eq!(two.pow(&mut cx, &m).unwrap().to_js_string(&cx), "0.125");
    }

    #[test]
    fn slices_like_javascript() {
        assert_eq!(js_slice("12345", 1, 3), "23");
        assert_eq!(js_slice("12345", 3, 99), "45");
        assert_eq!(js_slice("12345", 7, 9), "");
        assert_eq!(js_slice("12345", -2, 5), "45");
    }
}
