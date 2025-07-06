//! Comparison, addition, subtraction, multiplication, division and integer powers.

use super::digits::{digit_count, get_base10_exponent, truncate};
use super::parse::from_number;
use super::{ceil_words, word_exponent, Context, Decimal, BASE, LOG_BASE};
use crate::js::JsError;

/// The rounding of a division: `divide(x, y, pr, rm, dp, base)`.
#[derive(Clone, Copy, Debug)]
pub(crate) struct DivSpec {
    /// The number of significant digits (or decimal places if `dp`); `None` for `precision`.
    pub pr: Option<i64>,
    /// The rounding mode (ignored when `pr` is `None`, which uses `rounding`).
    pub rm: i64,
    /// Whether `pr` counts decimal places rather than significant digits.
    pub dp: bool,
    /// The base of the digit arrays when dividing for a base conversion.
    pub base: Option<i64>,
}

impl DivSpec {
    /// `divide(x, y)`: `precision` significant digits, rounding mode `rounding`.
    pub(crate) const DEFAULT: DivSpec = DivSpec {
        pr: None,
        rm: 0,
        dp: false,
        base: None,
    };

    /// `divide(x, y, sd, rm)`.
    pub(crate) fn sd(sd: i64, rm: i64) -> DivSpec {
        DivSpec {
            pr: Some(sd),
            rm,
            dp: false,
            base: None,
        }
    }

    /// `divide(x, y, dp, rm, 1)`.
    pub(crate) fn dp(dp: i64, rm: i64) -> DivSpec {
        DivSpec {
            pr: Some(dp),
            rm,
            dp: true,
            base: None,
        }
    }
}

/// The operand of a binary method, which decimal.js converts with `new Ctor(y)`.
///
/// A Decimal operand is copied by that conversion, which makes it Infinity or zero when its
/// exponent is outside `[minE, maxE]`; a [`Converted`] operand is what the conversion made of a
/// JavaScript value and is used as it is.
pub trait Operand {
    /// `new Ctor(y)`.
    fn convert(self, cx: &Context) -> Decimal;
}

impl Operand for &Decimal {
    fn convert(self, cx: &Context) -> Decimal {
        Decimal::copy_of(cx, self)
    }
}

/// The Decimal that `new Ctor(v)` made from a method argument `v` that was not a Decimal.
pub struct Converted(pub Decimal);

impl Operand for Converted {
    fn convert(self, _cx: &Context) -> Decimal {
        self.0
    }
}

impl Decimal {
    /// `x.cmp(y)`: 1, -1, 0, or NaN if either value is NaN.
    pub fn cmp_value(&self, cx: &Context, y: impl Operand) -> f64 {
        let y = y.convert(cx);
        let (xs, ys) = (self.s, y.s);
        let (Some(xd), Some(yd)) = (&self.d, &y.d) else {
            if xs == 0 || ys == 0 {
                return f64::NAN;
            }
            if xs != ys {
                return f64::from(xs);
            }
            if self.d.is_none() && y.d.is_none() {
                return 0.0;
            }
            return if self.d.is_none() ^ (xs < 0) {
                1.0
            } else {
                -1.0
            };
        };

        if xd[0] == 0 || yd[0] == 0 {
            return if xd[0] != 0 {
                f64::from(xs)
            } else if yd[0] != 0 {
                f64::from(-ys)
            } else {
                0.0
            };
        }

        if xs != ys {
            return f64::from(xs);
        }

        let flip = |greater: bool| if greater ^ (xs < 0) { 1.0 } else { -1.0 };

        if self.e != y.e {
            return flip(self.e > y.e);
        }

        for (a, b) in xd.iter().zip(yd.iter()) {
            if a != b {
                return flip(a > b);
            }
        }

        if xd.len() == yd.len() {
            0.0
        } else {
            flip(xd.len() > yd.len())
        }
    }

    /// `x.eq(y)`.
    pub(crate) fn eq_value(&self, cx: &Context, y: &Decimal) -> bool {
        self.cmp_value(cx, y) == 0.0
    }

    /// `x.gt(y)`.
    pub(crate) fn gt(&self, cx: &Context, y: &Decimal) -> bool {
        self.cmp_value(cx, y) > 0.0
    }

    /// `x.lt(y)`.
    pub(crate) fn lt(&self, cx: &Context, y: &Decimal) -> bool {
        self.cmp_value(cx, y) < 0.0
    }

    /// `x.lte(y)`.
    pub(crate) fn lte(&self, cx: &Context, y: &Decimal) -> bool {
        self.cmp_value(cx, y) < 1.0
    }

    /// `x.abs()`.
    pub(crate) fn abs(&self, cx: &Context) -> Decimal {
        let mut x = Decimal::copy_of(cx, self);
        if x.s < 0 {
            x.s = 1;
        }
        x.checked(cx)
    }

    /// `x.neg()`.
    pub(crate) fn neg(&self, cx: &Context) -> Decimal {
        let mut x = Decimal::copy_of(cx, self);
        x.s = -x.s;
        x.checked(cx)
    }

    /// `x.plus(y)`.
    pub(crate) fn plus(&self, cx: &Context, y: impl Operand) -> Decimal {
        let mut y = y.convert(cx);
        let x = self;

        let (Some(xd0), Some(yd0)) = (&x.d, &y.d) else {
            if x.s == 0 || y.s == 0 {
                return Decimal::nan();
            }
            if x.d.is_none() {
                return Decimal::copy_of(
                    cx,
                    &if y.d.is_some() || x.s == y.s {
                        x.clone()
                    } else {
                        Decimal::nan()
                    },
                );
            }
            return y;
        };

        if x.s != y.s {
            y.s = -y.s;
            return x.minus(cx, &y);
        }

        let pr = cx.precision;
        let rm = cx.rounding;

        if xd0[0] == 0 || yd0[0] == 0 {
            if yd0[0] == 0 {
                y = Decimal::copy_of(cx, x);
            }
            return if cx.external {
                y.round_to(cx, pr, rm)
            } else {
                y
            };
        }

        let mut k = word_exponent(x.e);
        let mut e = word_exponent(y.e);
        let mut xd = xd0.clone();
        let mut yd = yd0.clone();
        let mut i = k - e;

        if i != 0 {
            let (d, len) = if i < 0 {
                i = -i;
                let len = yd.len() as i64;
                (&mut xd, len)
            } else {
                e = k;
                let len = xd.len() as i64;
                (&mut yd, len)
            };

            // Limit the number of zeros prepended to max(ceil(pr / LOG_BASE), len) + 1.
            k = ceil_words(pr);
            let len = if k > len { k + 1 } else { len + 1 };

            if i > len {
                i = len;
                d.truncate(1);
            }

            d.splice(0..0, std::iter::repeat_n(0, i as usize));
        }

        // xd is the longer array; only start adding at yd.len() - 1.
        if xd.len() < yd.len() {
            std::mem::swap(&mut xd, &mut yd);
        }
        let mut carry = 0;
        for i in (0..yd.len()).rev() {
            let t = xd[i] + yd[i] + carry;
            carry = t / BASE;
            xd[i] = t % BASE;
        }

        if carry != 0 {
            xd.insert(0, carry);
            e += 1;
        }

        while xd.len() > 1 && xd.last() == Some(&0) {
            xd.pop();
        }

        y.e = get_base10_exponent(&xd, e);
        y.d = Some(xd);

        if cx.external {
            y.round_to(cx, pr, rm)
        } else {
            y
        }
    }

    /// `x.minus(y)`.
    pub(crate) fn minus(&self, cx: &Context, y: impl Operand) -> Decimal {
        let mut y = y.convert(cx);
        let x = self;

        let (Some(xd0), Some(yd0)) = (&x.d, &y.d) else {
            if x.s == 0 || y.s == 0 {
                return Decimal::nan();
            }
            if x.d.is_some() {
                y.s = -y.s;
                return y;
            }
            return Decimal::copy_of(
                cx,
                &if y.d.is_some() || x.s != y.s {
                    x.clone()
                } else {
                    Decimal::nan()
                },
            );
        };

        if x.s != y.s {
            y.s = -y.s;
            return x.plus(cx, &y);
        }

        let pr = cx.precision;
        let rm = cx.rounding;

        if xd0[0] == 0 || yd0[0] == 0 {
            if yd0[0] != 0 {
                y.s = -y.s;
            } else if xd0[0] != 0 {
                y = Decimal::copy_of(cx, x);
            } else {
                return Decimal::zero(if rm == 3 { -1 } else { 1 });
            }
            return if cx.external {
                y.round_to(cx, pr, rm)
            } else {
                y
            };
        }

        let mut e = word_exponent(y.e);
        let xe = word_exponent(x.e);
        let mut xd = xd0.clone();
        let mut yd = yd0.clone();
        let mut k = xe - e;
        let mut x_lt_y;

        if k != 0 {
            x_lt_y = k < 0;
            let (d, len) = if x_lt_y {
                k = -k;
                let len = yd.len() as i64;
                (&mut xd, len)
            } else {
                e = xe;
                let len = xd.len() as i64;
                (&mut yd, len)
            };

            // Limit the number of zeros prepended to max(ceil(pr / LOG_BASE), len) + 2.
            let i = ceil_words(pr).max(len) + 2;
            if k > i {
                k = i;
                d.truncate(1);
            }

            d.splice(0..0, std::iter::repeat_n(0, k as usize));
        } else {
            let len = xd.len().min(yd.len());
            x_lt_y = xd.len() < yd.len();
            for i in 0..len {
                if xd[i] != yd[i] {
                    x_lt_y = xd[i] < yd[i];
                    break;
                }
            }
            k = 0;
        }

        if x_lt_y {
            std::mem::swap(&mut xd, &mut yd);
            y.s = -y.s;
        }

        // Append zeros to xd if shorter; subtraction only needs to start at yd.len().
        if yd.len() > xd.len() {
            xd.resize(yd.len(), 0);
        }
        let mut len = xd.len();

        let mut i = yd.len();
        while i as i64 > k {
            i -= 1;
            if xd[i] < yd[i] {
                let mut j = i;
                while j > 0 {
                    j -= 1;
                    if xd[j] != 0 {
                        break;
                    }
                    xd[j] = BASE - 1;
                }
                xd[j] -= 1;
                xd[i] += BASE;
            }
            xd[i] -= yd[i];
        }

        while len > 0 && xd[len - 1] == 0 {
            xd.pop();
            len -= 1;
        }

        while xd.first() == Some(&0) {
            xd.remove(0);
            e -= 1;
        }

        if xd.first().is_none_or(|&w| w == 0) {
            return Decimal::zero(if rm == 3 { -1 } else { 1 });
        }

        y.e = get_base10_exponent(&xd, e);
        y.d = Some(xd);

        if cx.external {
            y.round_to(cx, pr, rm)
        } else {
            y
        }
    }

    /// `x.times(y)`.
    pub(crate) fn times(&self, cx: &Context, y: impl Operand) -> Decimal {
        let mut y = y.convert(cx);
        let x = self;
        y.s *= x.s;

        let (xd, yd) = match (&x.d, &y.d) {
            (Some(xd), Some(yd)) if xd[0] != 0 && yd[0] != 0 => (xd, yd),
            _ => {
                let x_zero = x.first_word() == Some(0);
                let y_zero = y.first_word() == Some(0);
                if y.s == 0 || x_zero && y.d.is_none() || y_zero && x.d.is_none() {
                    return Decimal::nan();
                }
                return if x.d.is_none() || y.d.is_none() {
                    Decimal::infinity(y.s)
                } else {
                    Decimal::zero(y.s)
                };
            }
        };

        let mut e = word_exponent(x.e).saturating_add(word_exponent(y.e));
        let (xd, yd) = if xd.len() < yd.len() {
            (yd, xd)
        } else {
            (xd, yd)
        };
        let xdl = xd.len();
        let ydl = yd.len();

        let mut r = vec![0i64; xdl + ydl];
        let mut carry = 0;
        for i in (0..ydl).rev() {
            carry = 0;
            let mut k = xdl + i;
            while k > i {
                let t = r[k] + yd[i] * xd[k - i - 1] + carry;
                r[k] = t % BASE;
                carry = t / BASE;
                k -= 1;
            }
            r[k] = (r[k] + carry) % BASE;
        }

        while r.len() > 1 && r.last() == Some(&0) {
            r.pop();
        }

        if carry != 0 {
            e += 1;
        } else {
            r.remove(0);
        }

        y.e = get_base10_exponent(&r, e);
        y.d = Some(r);

        if cx.external {
            y.round_to(cx, cx.precision, cx.rounding)
        } else {
            y
        }
    }

    /// `x.times(n)` for a JavaScript number `n`.
    pub(crate) fn times_number(&self, cx: &Context, n: f64) -> Decimal {
        self.times(cx, &from_number(cx, n))
    }

    /// `x.plus(n)` for a JavaScript number `n`.
    pub(crate) fn plus_number(&self, cx: &Context, n: f64) -> Decimal {
        self.plus(cx, &from_number(cx, n))
    }

    /// `x.div(y)`.
    pub(crate) fn div(&self, cx: &mut Context, y: impl Operand) -> Decimal {
        let y = y.convert(cx);
        divide(cx, self, &y, DivSpec::DEFAULT)
    }

    /// `x.divToInt(y)`.
    pub(crate) fn div_to_int(&self, cx: &mut Context, y: impl Operand) -> Decimal {
        let y = y.convert(cx);
        let q = divide(cx, self, &y, DivSpec::dp(0, 1));
        q.round_to(cx, cx.precision, cx.rounding)
    }

    /// `x.mod(y)`.
    pub(crate) fn modulo(&self, cx: &mut Context, y: impl Operand) -> Decimal {
        let x = self;
        let y = y.convert(cx);

        if x.d.is_none() || y.s == 0 || y.first_word() == Some(0) {
            return Decimal::nan();
        }

        if y.d.is_none() || x.first_word() == Some(0) {
            return Decimal::copy_of(cx, x).round_to(cx, cx.precision, cx.rounding);
        }

        cx.external = false;

        let mut q = if cx.modulo == 9 {
            // Euclidian division: q = sign(y) * floor(x / abs(y))
            let abs_y = y.abs(cx);
            let mut q = divide(cx, x, &abs_y, DivSpec::dp(0, 3));
            q.s *= y.s;
            q
        } else {
            divide(cx, x, &y, DivSpec::dp(0, cx.modulo))
        };

        q = q.times(cx, &y);

        cx.external = true;

        x.minus(cx, &q)
    }
}

/// `intPow(Ctor, x, n, pr)`: `x^n` for an integer `n >= 0` by repeated squaring, keeping
/// `ceil(pr / LOG_BASE + 4)` words.
///
/// decimal.js truncates the digits of every product, so a product that is NaN or Infinity throws the
/// TypeError of reading `null.length`, and `external` stays false.
pub(crate) fn int_pow(cx: &mut Context, x: &Decimal, n: f64, pr: f64) -> Result<Decimal, JsError> {
    let mut is_truncated = false;
    let mut r = from_number(cx, 1.0);
    let mut x = x.clone();
    let mut n = n;
    let k = (pr / LOG_BASE as f64 + 4.0).ceil() as i64;

    cx.external = false;

    loop {
        if n % 2.0 != 0.0 {
            r = r.times(cx, &x);
            let d = r.words_mut_or_null_error()?;
            if truncate(d, k) {
                is_truncated = true;
            }
        }

        n = (n / 2.0).floor();
        if n == 0.0 {
            // To ensure correct rounding when r.d is truncated, increment the last word if it is
            // zero.
            let d = r.words_mut_or_null_error()?;
            if is_truncated {
                if let Some(last) = d.last_mut().filter(|w| **w == 0) {
                    *last += 1;
                }
            }
            break;
        }

        x = x.times(cx, &x);
        let d = x.words_mut_or_null_error()?;
        truncate(d, k);
    }

    cx.external = true;

    Ok(r)
}

/// `divide(x, y, pr, rm, dp, base)`: long division of digit arrays in base 1e7 (or in a small base
/// for base conversions), rounded as `spec` asks.
pub(crate) fn divide(cx: &mut Context, x: &Decimal, y: &Decimal, spec: DivSpec) -> Decimal {
    let sign: i8 = if x.s == y.s && x.s != 0 { 1 } else { -1 };

    let (xd, yd) = match (&x.d, &y.d) {
        (Some(xd), Some(yd)) if xd[0] != 0 && yd[0] != 0 => (xd.clone(), yd.clone()),
        _ => {
            let both_zero_or_infinite = match (&x.d, &y.d) {
                (Some(xd), Some(yd)) => xd[0] == yd[0],
                (Some(_), None) => false,
                (None, yd) => yd.is_none(),
            };
            if x.s == 0 || y.s == 0 || both_zero_or_infinite {
                return Decimal::nan();
            }
            return if x.first_word() == Some(0) || y.d.is_none() {
                Decimal::zero(sign)
            } else {
                Decimal::infinity(sign)
            };
        }
    };

    let (base, log_base, mut e) = match spec.base {
        Some(base) => (base, 1, x.e - y.e),
        None => (
            BASE,
            LOG_BASE,
            word_exponent(x.e).saturating_sub(word_exponent(y.e)),
        ),
    };

    let mut xd = xd;
    let mut yd = yd;
    let mut yl = yd.len();
    let mut xl = xd.len();
    let mut qd: Vec<i64> = Vec::new();

    // The result exponent may be one less than e. The digits of a Decimal from toStringBinary may
    // have trailing zeros.
    let mut i = 0;
    while i < yd.len() && yd[i] == xd.get(i).copied().unwrap_or(0) {
        i += 1;
    }
    if i < yd.len() && yd[i] > xd.get(i).copied().unwrap_or(0) {
        e -= 1;
    }

    let (pr, rm, sd) = match spec.pr {
        None => (cx.precision, cx.rounding, cx.precision),
        Some(pr) if spec.dp => (pr, spec.rm, pr + (x.e - y.e) + 1),
        Some(pr) => (pr, spec.rm, pr),
    };

    let more: bool;
    if sd < 0 {
        qd.push(1);
        more = true;
    } else {
        // Convert precision in number of base 10 digits to base 1e7 digits.
        let mut sd = crate::js::number::to_int32(sd as f64 / log_base as f64 + 2.0) as i64;
        let mut i = 0usize;

        if yl == 1 {
            let mut k = 0i64;
            let yd0 = yd[0];
            sd += 1;
            loop {
                if !(i < xl || k != 0) {
                    break;
                }
                let go = sd != 0;
                sd -= 1;
                if !go {
                    break;
                }
                let t = k * base + xd.get(i).copied().unwrap_or(0);
                if i < qd.len() {
                    qd[i] = t / yd0;
                } else {
                    qd.push(t / yd0);
                }
                k = t % yd0;
                i += 1;
            }
            more = k != 0 || i < xl;
        } else {
            // Normalise xd and yd so the highest order digit of yd is >= base / 2.
            let k = base / (yd[0] + 1);
            if k > 1 {
                yd = multiply_integer(&yd, k, base);
                xd = multiply_integer(&xd, k, base);
                yl = yd.len();
                xl = xd.len();
            }

            let mut xi = yl;
            let mut rem: Vec<i64> = xd[..yl.min(xd.len())].to_vec();
            // `rem[0]` of decimal.js can become undefined: the remainder ran past the digits.
            let mut rem_undefined = false;
            let mut rem_l = rem.len();

            // Add zeros to make the remainder as long as the divisor.
            while rem_l < yl {
                rem.push(0);
                rem_l += 1;
            }

            let mut yz = yd.clone();
            yz.insert(0, 0);
            let mut yd0 = yd[0];
            if yd[1] >= base / 2 {
                yd0 += 1;
            }

            loop {
                let mut k = 0i64;

                let mut cmp = compare(&yd, &rem, yl, rem_l);

                if cmp < 0 {
                    let mut rem0 = rem[0];
                    if yl != rem_l {
                        rem0 = rem0 * base + rem.get(1).copied().unwrap_or(0);
                    }

                    // k will be how many times the divisor goes into the current remainder.
                    k = rem0 / yd0;

                    let mut prod: Vec<i64>;
                    if k > 1 {
                        if k >= base {
                            k = base - 1;
                        }

                        prod = multiply_integer(&yd, k, base);
                        let prod_l = prod.len();
                        rem_l = rem.len();

                        cmp = compare(&prod, &rem, prod_l, rem_l);

                        if cmp == 1 {
                            k -= 1;
                            let b = if yl < prod_l { &yz } else { &yd };
                            subtract(&mut prod, b, prod_l, base);
                        }
                    } else {
                        // cmp is -1. If k is 0, there is no need to compare yd and rem again
                        // below, so change cmp to 1 to avoid it.
                        if k == 0 {
                            cmp = 1;
                            k = 1;
                        }
                        prod = yd.clone();
                    }

                    if prod.len() < rem_l {
                        prod.insert(0, 0);
                    }

                    subtract(&mut rem, &prod, rem_l, base);

                    if cmp == -1 {
                        rem_l = rem.len();
                        cmp = compare(&yd, &rem, yl, rem_l);
                        if cmp < 1 {
                            k += 1;
                            let b = if yl < rem_l { &yz } else { &yd };
                            subtract(&mut rem, b, rem_l, base);
                        }
                    }

                    rem_l = rem.len();
                } else if cmp == 0 {
                    k += 1;
                    rem = vec![0];
                }

                if i < qd.len() {
                    qd[i] = k;
                } else {
                    qd.push(k);
                }
                i += 1;

                if cmp != 0 && rem[0] != 0 {
                    let next = xd.get(xi).copied().unwrap_or(0);
                    if rem_l < rem.len() {
                        rem[rem_l] = next;
                    } else {
                        rem.push(next);
                    }
                    rem_l += 1;
                } else {
                    match xd.get(xi) {
                        Some(&w) => rem = vec![w],
                        None => rem_undefined = true,
                    }
                    rem_l = 1;
                }

                let more_digits = xi < xl || !rem_undefined;
                xi += 1;
                if !more_digits {
                    break;
                }
                let go = sd != 0;
                sd -= 1;
                if !go {
                    break;
                }
            }

            more = !rem_undefined;
        }

        if qd.first() == Some(&0) {
            qd.remove(0);
        }
    }

    let mut q = Decimal {
        s: sign,
        e: 0,
        d: None,
    };

    if log_base == 1 {
        q.e = e;
        q.d = Some(qd);
        cx.inexact = more;
        q
    } else {
        let digits = qd.first().map_or(1, |&w| digit_count(w));
        q.e = digits + e * log_base - 1;
        if qd.is_empty() {
            qd.push(0);
        }
        q.d = Some(qd);
        let sd = if spec.dp { pr + q.e + 1 } else { pr };
        q.finalised(cx, Some(sd), rm, more)
    }
}

/// `multiplyInteger(x, k, base)`: a copy of the digits `x` times the small integer `k`.
fn multiply_integer(x: &[i64], k: i64, base: i64) -> Vec<i64> {
    let mut x = x.to_vec();
    let mut carry = 0;
    for v in x.iter_mut().rev() {
        let temp = *v * k + carry;
        *v = temp % base;
        carry = temp / base;
    }
    if carry != 0 {
        x.insert(0, carry);
    }
    x
}

/// `compare(a, b, aL, bL)`.
fn compare(a: &[i64], b: &[i64], al: usize, bl: usize) -> i64 {
    if al != bl {
        return if al > bl { 1 } else { -1 };
    }
    for i in 0..al {
        let (x, y) = (
            a.get(i).copied().unwrap_or(0),
            b.get(i).copied().unwrap_or(0),
        );
        if x != y {
            return if x > y { 1 } else { -1 };
        }
    }
    0
}

/// `subtract(a, b, aL, base)`: `a -= b` over the first `aL` digits, then remove leading zeros.
fn subtract(a: &mut Vec<i64>, b: &[i64], al: usize, base: i64) {
    let mut borrow = 0;
    let mut l = al;
    while l > 0 {
        l -= 1;
        let bv = b.get(l).copied().unwrap_or(0);
        if l >= a.len() {
            a.resize(l + 1, 0);
        }
        a[l] -= borrow;
        borrow = i64::from(a[l] < bv);
        a[l] = borrow * base + a[l] - bv;
    }
    while a.len() > 1 && a[0] == 0 {
        a.remove(0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decimal::parse::parse_str;

    fn d(cx: &mut Context, s: &str) -> Decimal {
        parse_str(cx, s).unwrap()
    }

    fn show(cx: &Context, x: &Decimal) -> String {
        x.to_js_string(cx)
    }

    #[test]
    fn addition_and_subtraction() {
        let mut cx = Context::default();
        let a = d(&mut cx, "0.1");
        let b = d(&mut cx, "0.2");
        assert_eq!(show(&cx, &a.plus(&cx, &b)), "0.3");
        let a = d(&mut cx, "1e30");
        let b = d(&mut cx, "1");
        assert_eq!(show(&cx, &a.plus(&cx, &b)), "1e+30");
        assert_eq!(show(&cx, &b.minus(&cx, &a)), "-1e+30");
        let a = d(&mut cx, "123456789.123456789");
        let b = d(&mut cx, "123456789.123456788");
        assert_eq!(show(&cx, &a.minus(&cx, &b)), "1e-9");
        cx.rounding = 3;
        assert_eq!(show(&cx, &a.minus(&cx, &a)), "0");
        assert!(a.minus(&cx, &a).is_neg());
    }

    #[test]
    fn multiplication_and_division() {
        let mut cx = Context::default();
        let a = d(&mut cx, "12345678.9");
        let b = d(&mut cx, "98765.4321");
        assert_eq!(show(&cx, &a.times(&cx, &b)), "1219326311126.35269");
        let one = d(&mut cx, "1");
        let three = d(&mut cx, "3");
        let r = one.div(&mut cx, &three);
        assert_eq!(show(&cx, &r), "0.33333333333333333333");
        let big = d(&mut cx, "1234567890123456789");
        let div = d(&mut cx, "987654321987");
        let r = big.div(&mut cx, &div);
        assert_eq!(show(&cx, &r), "1249999.9874852031365");
    }

    #[test]
    fn modulo_follows_the_mode() {
        let mut cx = Context::default();
        let a = d(&mut cx, "-7");
        let b = d(&mut cx, "3");
        let r = a.modulo(&mut cx, &b);
        assert_eq!(show(&cx, &r), "-1");
        cx.modulo = 3;
        let r = a.modulo(&mut cx, &b);
        assert_eq!(show(&cx, &r), "2");
        cx.modulo = 9;
        let nb = d(&mut cx, "-3");
        let r = a.modulo(&mut cx, &nb);
        assert_eq!(show(&cx, &r), "2");
    }

    #[test]
    fn integer_powers() {
        let mut cx = Context::default();
        let two = d(&mut cx, "2");
        let r = int_pow(&mut cx, &two, 100.0, 20.0)
            .unwrap()
            .round_to(&cx, 20, 4);
        assert_eq!(show(&cx, &r), "1.2676506002282294015e+30");
        assert!(cx.external);
    }
}
