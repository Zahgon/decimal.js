//! Trigonometric, inverse trigonometric, hyperbolic and inverse hyperbolic functions.

use super::arith::{divide, DivSpec};
use super::parse::{from_number, parse_numeric};
use super::transcendental::get_pi;
use super::{ceil_words, Context, Decimal, LOG_BASE, PI_PRECISION};
use crate::js::{number, JsError};

/// `tinyPow(b, e)`: b^e by repeated floating-point multiplication (e >= 1).
fn tiny_pow(b: f64, e: i64) -> f64 {
    let mut n = b;
    for _ in 1..e {
        n *= b;
    }
    n
}

/// `isOdd(n)`: whether the last digit word of `n` is odd.
fn is_odd(n: &Decimal) -> bool {
    n.words().last().copied().unwrap_or(0) & 1 == 1
}

/// `Math.max(x.e, x.sd())` for a finite non-zero `x`.
fn magnitude(x: &Decimal) -> i64 {
    x.e.max(x.sd_count())
}

/// `1.4 * Math.sqrt(len)`, capped at 16 and truncated: the number of argument halvings (by 3) for
/// the sine series.
fn sine_reductions(len: usize) -> i64 {
    let k = 1.4 * (len as f64).sqrt();
    if k > 16.0 {
        16
    } else {
        number::to_int32(k) as i64
    }
}

/// The argument scale factor of the cosine series: `k` and the string of `1 / 4^k`.
fn cosine_scale(len: usize) -> (i64, String) {
    if len < 32 {
        let k = (len as f64 / 3.0).ceil() as i64;
        (k, number::to_string(1.0 / tiny_pow(4.0, k)))
    } else {
        (16, "2.3283064365386962890625e-10".to_string())
    }
}

/// `taylorSeries(Ctor, n, x, y, isHyperbolic)`: the series y - x^n/n! + x^(n+2)/(n+2)! - … (or with
/// all terms added if `hyperbolic`), summed to the precision of the context.
fn taylor_series(
    cx: &mut Context,
    mut n: i64,
    x: &Decimal,
    y: &Decimal,
    hyperbolic: bool,
) -> Decimal {
    let pr = cx.precision;
    let k = ceil_words(pr);

    cx.external = false;
    let x2 = x.times(cx, x);
    let mut u = Decimal::copy_of(cx, y);
    let mut y = y.clone();

    loop {
        let den = from_number(cx, (n * (n + 1)) as f64);
        n += 2;
        let mut t = divide(cx, &u.times(cx, &x2), &den, DivSpec::sd(pr, 1));
        u = if hyperbolic {
            y.plus(cx, &t)
        } else {
            y.minus(cx, &t)
        };
        let den = from_number(cx, (n * (n + 1)) as f64);
        n += 2;
        y = divide(cx, &t.times(cx, &x2), &den, DivSpec::sd(pr, 1));
        t = u.plus(cx, &y);

        let td = t.words();
        if td.len() as i64 > k {
            let ud = u.words();
            let mut j = k;
            while j >= 0 && td.get(j as usize) == ud.get(j as usize) {
                j -= 1;
            }
            if j == -1 {
                cx.external = true;
                let mut t = t;
                if let Some(d) = t.d.as_mut() {
                    d.truncate(k as usize + 1);
                }
                return t;
            }
        }

        u = y;
        y = t;
    }
}

/// `toLessThanHalfPi(Ctor, x)`: |x| reduced to the range [0, pi/2], recording the quadrant of x.
fn to_less_than_half_pi(cx: &mut Context, x: &Decimal) -> Result<Decimal, JsError> {
    let is_neg = x.s < 0;
    let pi = get_pi(cx, cx.precision, 1)?;
    let half_pi = pi.times_number(cx, 0.5);

    let x = x.abs(cx);

    if x.lte(cx, &half_pi) {
        cx.quadrant = if is_neg { 4 } else { 1 };
        return Ok(x);
    }

    let t = x.div_to_int(cx, &pi);

    let x = if t.is_zero() {
        cx.quadrant = if is_neg { 3 } else { 2 };
        x
    } else {
        let x = x.minus(cx, &t.times(cx, &pi));

        // 0 <= x < pi
        if x.lte(cx, &half_pi) {
            cx.quadrant = match (is_odd(&t), is_neg) {
                (true, true) => 2,
                (true, false) => 3,
                (false, true) => 4,
                (false, false) => 1,
            };
            return Ok(x);
        }

        cx.quadrant = match (is_odd(&t), is_neg) {
            (true, true) => 1,
            (true, false) => 4,
            (false, true) => 3,
            (false, false) => 2,
        };
        x
    };

    Ok(x.minus(cx, &pi).abs(cx))
}

/// `sine(Ctor, x)` for 0 <= x <= pi/2, using sin(x) = 3sin(x/3) - 4sin^3(x/3) to reduce the
/// argument.
fn sine(cx: &mut Context, x: &Decimal) -> Decimal {
    let len = x.words().len();

    if len < 3 {
        return if x.is_zero() {
            x.clone()
        } else {
            taylor_series(cx, 2, x, x, false)
        };
    }

    let mut k = sine_reductions(len);
    let x = x.times_number(cx, 1.0 / tiny_pow(5.0, k));
    let mut x = taylor_series(cx, 2, &x, &x, false);

    // Reverse argument reduction
    let d5 = from_number(cx, 5.0);
    let d16 = from_number(cx, 16.0);
    let d20 = from_number(cx, 20.0);
    while k > 0 {
        k -= 1;
        let sin2_x = x.times(cx, &x);
        let inner = d16.times(cx, &sin2_x).minus(cx, &d20);
        x = x.times(cx, &d5.plus(cx, &sin2_x.times(cx, &inner)));
    }

    x
}

/// `cosine(Ctor, x)` for 0 <= x <= pi/2, using cos(4x) = 8(cos^4(x) - cos^2(x)) + 1 to reduce the
/// argument.
fn cosine(cx: &mut Context, x: &Decimal) -> Decimal {
    if x.is_zero() {
        return x.clone();
    }

    let (k, y) = cosine_scale(x.words().len());

    cx.precision += k;

    let scale = parse_numeric(cx, &y);
    let scaled = x.times(cx, &scale);
    let one = from_number(cx, 1.0);
    let mut x = taylor_series(cx, 1, &scaled, &one, false);

    // Reverse argument reduction
    for _ in 0..k {
        let cos2x = x.times(cx, &x);
        x = cos2x
            .times(cx, &cos2x)
            .minus(cx, &cos2x)
            .times_number(cx, 8.0)
            .plus_number(cx, 1.0);
    }

    cx.precision -= k;

    x
}

impl Decimal {
    /// `x.sin()`.
    pub(crate) fn sin(&self, cx: &mut Context) -> Result<Decimal, JsError> {
        if !self.is_finite() {
            return Ok(Decimal::nan());
        }
        if self.is_zero() {
            return Ok(Decimal::copy_of(cx, self));
        }

        let pr = cx.precision;
        let rm = cx.rounding;
        cx.precision = pr + magnitude(self) + LOG_BASE;
        cx.rounding = 1;

        let reduced = to_less_than_half_pi(cx, self)?;
        let x = sine(cx, &reduced);

        cx.precision = pr;
        cx.rounding = rm;

        let x = if cx.quadrant > 2 { x.neg(cx) } else { x };
        Ok(x.finalised(cx, Some(pr), rm, true))
    }

    /// `x.cos()`.
    pub(crate) fn cos(&self, cx: &mut Context) -> Result<Decimal, JsError> {
        let Some(first) = self.first_word() else {
            return Ok(Decimal::nan());
        };

        // cos(0) = cos(-0) = 1
        if first == 0 {
            return Ok(from_number(cx, 1.0));
        }

        let pr = cx.precision;
        let rm = cx.rounding;
        cx.precision = pr + magnitude(self) + LOG_BASE;
        cx.rounding = 1;

        let reduced = to_less_than_half_pi(cx, self)?;
        let x = cosine(cx, &reduced);

        cx.precision = pr;
        cx.rounding = rm;

        let x = if cx.quadrant == 2 || cx.quadrant == 3 {
            x.neg(cx)
        } else {
            x
        };
        Ok(x.finalised(cx, Some(pr), rm, true))
    }

    /// `x.tan()`.
    pub(crate) fn tan(&self, cx: &mut Context) -> Result<Decimal, JsError> {
        if !self.is_finite() {
            return Ok(Decimal::nan());
        }
        if self.is_zero() {
            return Ok(Decimal::copy_of(cx, self));
        }

        let pr = cx.precision;
        let rm = cx.rounding;
        cx.precision = pr + 10;
        cx.rounding = 1;

        let mut x = self.sin(cx)?;
        x.s = 1;
        let one = from_number(cx, 1.0);
        let cos = one.minus(cx, &x.times(cx, &x)).sqrt(cx);
        let x = divide(cx, &x, &cos, DivSpec::sd(pr + 10, 0));

        cx.precision = pr;
        cx.rounding = rm;

        let x = if cx.quadrant == 2 || cx.quadrant == 4 {
            x.neg(cx)
        } else {
            x
        };
        Ok(x.finalised(cx, Some(pr), rm, true))
    }

    /// `x.acos()`.
    pub(crate) fn acos(&self, cx: &mut Context) -> Result<Decimal, JsError> {
        let x = self;
        let one = from_number(cx, 1.0);
        let k = x.abs(cx).cmp_value(cx, &one);
        let pr = cx.precision;
        let rm = cx.rounding;

        if k != -1.0 {
            return if k == 0.0 {
                // |x| is 1
                if x.is_neg() {
                    get_pi(cx, pr, rm)
                } else {
                    Ok(from_number(cx, 0.0))
                }
            } else {
                // |x| > 1 or x is NaN
                Ok(Decimal::nan())
            };
        }

        if x.is_zero() {
            return Ok(get_pi(cx, pr + 4, rm)?.times_number(cx, 0.5));
        }

        // acos(x) = 2*atan(sqrt((1-x)/(1+x)))
        cx.precision = pr + 6;
        cx.rounding = 1;

        let numerator = one.minus(cx, x);
        let denominator = x.plus_number(cx, 1.0);
        let x = numerator.div(cx, &denominator).sqrt(cx).atan(cx)?;

        cx.precision = pr;
        cx.rounding = rm;

        Ok(x.times_number(cx, 2.0))
    }

    /// `x.asin()`.
    pub(crate) fn asin(&self, cx: &mut Context) -> Result<Decimal, JsError> {
        let x = self;

        if x.is_zero() {
            return Ok(Decimal::copy_of(cx, x));
        }

        let one = from_number(cx, 1.0);
        let k = x.abs(cx).cmp_value(cx, &one);
        let pr = cx.precision;
        let rm = cx.rounding;

        if k != -1.0 {
            // |x| is 1
            if k == 0.0 {
                let mut half_pi = get_pi(cx, pr + 4, rm)?.times_number(cx, 0.5);
                half_pi.s = x.s;
                return Ok(half_pi);
            }

            // |x| > 1 or x is NaN
            return Ok(Decimal::nan());
        }

        // asin(x) = 2*atan(x/(1+sqrt(1-x^2)))
        cx.precision = pr + 6;
        cx.rounding = 1;

        let root = one.minus(cx, &x.times(cx, x)).sqrt(cx).plus_number(cx, 1.0);
        let x = x.div(cx, &root).atan(cx)?;

        cx.precision = pr;
        cx.rounding = rm;

        Ok(x.times_number(cx, 2.0))
    }

    /// `x.atan()`.
    pub(crate) fn atan(&self, cx: &mut Context) -> Result<Decimal, JsError> {
        let mut x = self.clone();
        let pr = cx.precision;
        let rm = cx.rounding;

        if !x.is_finite() {
            if x.s == 0 {
                return Ok(Decimal::nan());
            }
            if pr + 4 <= PI_PRECISION {
                let mut r = get_pi(cx, pr + 4, rm)?.times_number(cx, 0.5);
                r.s = x.s;
                return Ok(r);
            }
        } else if x.is_zero() {
            return Ok(Decimal::copy_of(cx, &x));
        } else {
            let one = from_number(cx, 1.0);
            if x.abs(cx).eq_value(cx, &one) && pr + 4 <= PI_PRECISION {
                let mut r = get_pi(cx, pr + 4, rm)?.times_number(cx, 0.25);
                r.s = x.s;
                return Ok(r);
            }
        }

        let wpr = pr + 10;
        cx.precision = wpr;
        cx.rounding = 1;

        // atan(x) = x - x^3/3 + x^5/5 - x^7/7 + ...
        // Argument reduction: atan(x) = 2 * atan(x / (1 + sqrt(1 + x^2)))
        let k = (number::to_int32(wpr as f64 / LOG_BASE as f64 + 2.0) as i64).min(28);

        for _ in 0..k {
            let one = from_number(cx, 1.0);
            let denominator = x.times(cx, &x).plus(cx, &one).sqrt(cx).plus(cx, &one);
            x = x.div(cx, &denominator);
        }

        cx.external = false;

        let j = ceil_words(wpr);
        let mut n: i64 = 1;
        let x2 = x.times(cx, &x);
        let mut r = Decimal::copy_of(cx, &x);
        let mut px = x.clone();

        // atan(x) = x - x^3/3 + x^5/5 - x^7/7 + ...
        loop {
            px = px.times(cx, &x2);
            n += 2;
            let den = from_number(cx, n as f64);
            let term = px.div(cx, &den);
            let t = r.minus(cx, &term);

            px = px.times(cx, &x2);
            n += 2;
            let den = from_number(cx, n as f64);
            let term = px.div(cx, &den);
            r = t.plus(cx, &term);

            let Some(rd) = r.d.as_ref() else {
                // decimal.js reads `r.d[j]` of NaN here.
                return Err(JsError::type_error(format!(
                    "Cannot read properties of null (reading '{j}')"
                )));
            };
            if rd.len() as i64 > j {
                let td = t.words();
                let mut i = j;
                while i >= 0 && rd.get(i as usize) == td.get(i as usize) {
                    i -= 1;
                }
                if i == -1 {
                    break;
                }
            }
        }

        if k != 0 {
            r = r.times_number(cx, f64::from(2i32 << (k - 1)));
        }

        cx.external = true;

        cx.precision = pr;
        cx.rounding = rm;
        Ok(r.finalised(cx, Some(pr), rm, true))
    }

    /// `x.cosh()`.
    pub(crate) fn cosh(&self, cx: &mut Context) -> Decimal {
        let x = self;
        let one = from_number(cx, 1.0);

        if !x.is_finite() {
            return if x.s != 0 {
                Decimal::infinity(1)
            } else {
                Decimal::nan()
            };
        }
        if x.is_zero() {
            return one;
        }

        let pr = cx.precision;
        let rm = cx.rounding;
        cx.precision = pr + magnitude(x) + 4;
        cx.rounding = 1;

        // cosh(x) = 1 + x^2/2! + x^4/4! + x^6/6! + ...
        // Argument reduction: cosh(4x) = 1 - 8cosh^2(x) + 8cosh^4(x) + 1
        let (k, n) = cosine_scale(x.words().len());

        let scale = parse_numeric(cx, &n);
        let scaled = x.times(cx, &scale);
        let start = from_number(cx, 1.0);
        let mut x = taylor_series(cx, 1, &scaled, &start, true);

        // Reverse argument reduction
        let d8 = from_number(cx, 8.0);
        for _ in 0..k {
            let cosh2_x = x.times(cx, &x);
            let inner = d8.minus(cx, &cosh2_x.times(cx, &d8));
            x = one.minus(cx, &cosh2_x.times(cx, &inner));
        }

        cx.precision = pr;
        cx.rounding = rm;
        x.finalised(cx, Some(pr), rm, true)
    }

    /// `x.sinh()`.
    pub(crate) fn sinh(&self, cx: &mut Context) -> Decimal {
        let x = self;

        if !x.is_finite() || x.is_zero() {
            return Decimal::copy_of(cx, x);
        }

        let pr = cx.precision;
        let rm = cx.rounding;
        cx.precision = pr + magnitude(x) + 4;
        cx.rounding = 1;
        let len = x.words().len();

        let x = if len < 3 {
            taylor_series(cx, 2, x, x, true)
        } else {
            // Alternative argument reduction: sinh(3x) = sinh(x)(3 + 4sinh^2(x))
            // i.e. sinh(5x) = sinh(x)(5 + sinh^2(x)(20 + 16sinh^2(x)))
            let mut k = sine_reductions(len);
            let x = x.times_number(cx, 1.0 / tiny_pow(5.0, k));
            let mut x = taylor_series(cx, 2, &x, &x, true);

            // Reverse argument reduction
            let d5 = from_number(cx, 5.0);
            let d16 = from_number(cx, 16.0);
            let d20 = from_number(cx, 20.0);
            while k > 0 {
                k -= 1;
                let sinh2_x = x.times(cx, &x);
                let inner = d16.times(cx, &sinh2_x).plus(cx, &d20);
                x = x.times(cx, &d5.plus(cx, &sinh2_x.times(cx, &inner)));
            }
            x
        };

        cx.precision = pr;
        cx.rounding = rm;

        x.finalised(cx, Some(pr), rm, true)
    }

    /// `x.tanh()`.
    pub(crate) fn tanh(&self, cx: &mut Context) -> Decimal {
        let x = self;

        if !x.is_finite() {
            return from_number(cx, x.sign_number());
        }

        // tanh(0) = 0
        if x.is_zero() {
            return Decimal::copy_of(cx, x);
        }

        let pr = cx.precision;
        let rm = cx.rounding;
        cx.precision = pr + 7;
        cx.rounding = 1;

        let sinh = x.sinh(cx);
        let cosh = x.cosh(cx);
        cx.precision = pr;
        cx.rounding = rm;
        divide(cx, &sinh, &cosh, DivSpec::sd(pr, rm))
    }

    /// `x.acosh()`.
    pub(crate) fn acosh(&self, cx: &mut Context) -> Result<Decimal, JsError> {
        let x = self;
        let one = from_number(cx, 1.0);

        if x.lte(cx, &one) {
            return Ok(if x.eq_value(cx, &one) {
                from_number(cx, 0.0)
            } else {
                Decimal::nan()
            });
        }
        if !x.is_finite() {
            return Ok(Decimal::copy_of(cx, x));
        }

        let pr = cx.precision;
        let rm = cx.rounding;
        cx.precision = pr + x.e.abs().max(x.sd_count()) + 4;
        cx.rounding = 1;
        cx.external = false;

        // acosh(x) = ln(x + sqrt(x^2 - 1))
        let x = x.times(cx, x).minus(cx, &one).sqrt(cx).plus(cx, x);

        cx.external = true;
        cx.precision = pr;
        cx.rounding = rm;

        x.ln(cx)
    }

    /// `x.asinh()`.
    pub(crate) fn asinh(&self, cx: &mut Context) -> Result<Decimal, JsError> {
        let x = self;

        if !x.is_finite() || x.is_zero() {
            return Ok(Decimal::copy_of(cx, x));
        }

        let pr = cx.precision;
        let rm = cx.rounding;
        cx.precision = pr + 2 * x.e.abs().max(x.sd_count()) + 6;
        cx.rounding = 1;
        cx.external = false;

        // asinh(x) = ln(x + sqrt(x^2 + 1))
        let one = from_number(cx, 1.0);
        let x = x.times(cx, x).plus(cx, &one).sqrt(cx).plus(cx, x);

        cx.external = true;
        cx.precision = pr;
        cx.rounding = rm;

        x.ln(cx)
    }

    /// `x.atanh()`.
    pub(crate) fn atanh(&self, cx: &mut Context) -> Result<Decimal, JsError> {
        let x = self;

        if !x.is_finite() {
            return Ok(Decimal::nan());
        }
        if x.e >= 0 {
            let one = from_number(cx, 1.0);
            return Ok(if x.abs(cx).eq_value(cx, &one) {
                Decimal::infinity(x.s)
            } else if x.is_zero() {
                Decimal::copy_of(cx, x)
            } else {
                Decimal::nan()
            });
        }

        let pr = cx.precision;
        let rm = cx.rounding;
        let xsd = x.sd_count();

        if xsd.max(pr) < 2 * -x.e - 1 {
            return Ok(Decimal::copy_of(cx, x).finalised(cx, Some(pr), rm, true));
        }

        let wpr = xsd - x.e;
        cx.precision = wpr;

        // atanh(x) = 0.5 * ln((1 + x) / (1 - x))
        let one = from_number(cx, 1.0);
        let numerator = x.plus(cx, &one);
        let denominator = one.minus(cx, x);
        let x = divide(cx, &numerator, &denominator, DivSpec::sd(wpr + pr, 1));

        cx.precision = pr + 4;
        cx.rounding = 1;

        let x = x.ln(cx)?;

        cx.precision = pr;
        cx.rounding = rm;

        Ok(x.times_number(cx, 0.5))
    }
}

/// `Decimal.atan2(y, x)`: the angle of the point (x, y) in radians, in the range -pi to pi.
pub(crate) fn atan2(cx: &mut Context, y: &Decimal, x: &Decimal) -> Result<Decimal, JsError> {
    let pr = cx.precision;
    let rm = cx.rounding;
    let wpr = pr + 4;

    // Either NaN
    if y.s == 0 || x.s == 0 {
        return Ok(Decimal::nan());
    }

    // Both ±Infinity
    if !y.is_finite() && !x.is_finite() {
        let factor = if x.s > 0 { 0.25 } else { 0.75 };
        let mut r = get_pi(cx, wpr, 1)?.times_number(cx, factor);
        r.s = y.s;
        return Ok(r);
    }

    // x is ±Infinity or y is ±0
    if !x.is_finite() || y.is_zero() {
        let mut r = if x.s < 0 {
            get_pi(cx, pr, rm)?
        } else {
            from_number(cx, 0.0)
        };
        r.s = y.s;
        return Ok(r);
    }

    // y is ±Infinity or x is ±0
    if !y.is_finite() || x.is_zero() {
        let mut r = get_pi(cx, wpr, 1)?.times_number(cx, 0.5);
        r.s = y.s;
        return Ok(r);
    }

    if x.s < 0 {
        cx.precision = wpr;
        cx.rounding = 1;
        let q = divide(cx, y, x, DivSpec::sd(wpr, 1));
        let q = Decimal::copy_of(cx, &q);
        let r = q.atan(cx)?;
        let pi = get_pi(cx, wpr, 1)?;
        cx.precision = pr;
        cx.rounding = rm;
        return Ok(if y.s < 0 {
            r.minus(cx, &pi)
        } else {
            r.plus(cx, &pi)
        });
    }

    let q = divide(cx, y, x, DivSpec::sd(wpr, 1));
    let q = Decimal::copy_of(cx, &q);
    q.atan(cx)
}

#[cfg(test)]
mod tests {
    use super::super::parse::parse_str;
    use super::*;

    fn d(cx: &mut Context, s: &str) -> Decimal {
        parse_str(cx, s).unwrap()
    }

    #[test]
    fn small_powers() {
        assert_eq!(tiny_pow(4.0, 3), 64.0);
        assert_eq!(number::to_string(1.0 / tiny_pow(4.0, 2)), "0.0625");
        assert_eq!(sine_reductions(3), 2);
        assert_eq!(sine_reductions(200), 16);
    }

    #[test]
    fn circular_functions() {
        let mut cx = Context::default();
        let one = d(&mut cx, "1");
        assert_eq!(
            one.sin(&mut cx).unwrap().to_js_string(&cx),
            "0.84147098480789650665"
        );
        assert_eq!(
            one.cos(&mut cx).unwrap().to_js_string(&cx),
            "0.5403023058681397174"
        );
        assert_eq!(
            one.atan(&mut cx).unwrap().to_js_string(&cx),
            "0.78539816339744830962"
        );
        assert!(cx.external);
    }

    #[test]
    fn hyperbolic_functions() {
        let mut cx = Context::default();
        let zero = d(&mut cx, "0");
        assert_eq!(zero.cosh(&mut cx).to_js_string(&cx), "1");
        let inf = d(&mut cx, "-Infinity");
        assert_eq!(inf.tanh(&mut cx).to_js_string(&cx), "-1");
        let half = d(&mut cx, "0.5");
        assert_eq!(
            half.atanh(&mut cx).unwrap().to_js_string(&cx),
            "0.5493061443340548457"
        );
    }
}
