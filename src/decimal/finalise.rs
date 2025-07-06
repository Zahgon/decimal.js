//! `finalise`: rounding to a number of significant digits, and the overflow and underflow checks.

use super::digits::{digit_count, div_pow10, pow10, rem_pow10};
use super::{Context, Decimal, BASE, LOG_BASE};

impl Decimal {
    /// `finalise(x, sd, rm, isTruncated)` in place: round to `sd` significant digits (no rounding if
    /// `sd` is `None`) using rounding mode `rm`, where `is_truncated` tells that there are non-zero
    /// digits beyond the stored ones. Then, when results are external, overflow to Infinity or
    /// underflow to zero.
    pub(crate) fn finalise_mut(
        &mut self,
        cx: &Context,
        sd: Option<i64>,
        rm: i64,
        is_truncated: bool,
    ) {
        if let Some(sd) = sd {
            let s = self.s;
            let e = self.e;
            let Some(xd) = self.d.as_mut() else {
                return;
            };
            match round_words(xd, e, s, sd, rm, is_truncated) {
                Rounded::Skipped => {}
                Rounded::Done { e } => self.e = e,
                Rounded::Final { e } => {
                    self.e = e;
                    return;
                }
            }
        }
        if cx.external && self.d.is_some() {
            if self.e > cx.max_e {
                self.d = None;
                self.e = 0;
            } else if self.e < cx.min_e {
                self.e = 0;
                self.d = Some(vec![0]);
            }
        }
    }

    /// `finalise(x, sd, rm, isTruncated)`, returning the rounded value.
    pub(crate) fn finalised(
        mut self,
        cx: &Context,
        sd: Option<i64>,
        rm: i64,
        is_truncated: bool,
    ) -> Decimal {
        self.finalise_mut(cx, sd, rm, is_truncated);
        self
    }

    /// `finalise(x, sd, rm)`.
    pub(crate) fn round_to(self, cx: &Context, sd: i64, rm: i64) -> Decimal {
        self.finalised(cx, Some(sd), rm, false)
    }

    /// `finalise(x)`: only the overflow and underflow checks.
    pub(crate) fn checked(self, cx: &Context) -> Decimal {
        self.finalised(cx, None, 0, false)
    }
}

enum Rounded {
    /// The rounding digit lies beyond the stored digits: nothing to round.
    Skipped,
    /// Rounded; the overflow checks follow.
    Done { e: i64 },
    /// The result is zero or a single unit, returned without the overflow checks.
    Final { e: i64 },
}

fn round_words(xd: &mut Vec<i64>, e: i64, s: i8, sd: i64, rm: i64, is_truncated: bool) -> Rounded {
    // rd: the rounding digit, i.e. the digit after the digit that may be rounded up.
    // w: the word of xd containing rd. xdi: the index of w within xd. digits: the number of digits
    // of w. i: the index of rd within w if all words had 7 digits. j: the actual index of rd
    // within w (negative if rd is a leading zero).
    let mut digits = digit_count(xd[0]);
    let mut i = sd - digits;
    let j: i64;
    let w: i64;
    let rd: i64;
    let mut xdi: i64;

    if i < 0 {
        i += LOG_BASE;
        j = sd;
        xdi = 0;
        w = xd[0];
        rd = div_pow10(w, digits - j - 1) % 10;
    } else {
        xdi = (i + 1 + LOG_BASE - 1) / LOG_BASE;
        let k = xd.len() as i64;
        if xdi >= k {
            if !is_truncated {
                return Rounded::Skipped;
            }
            xd.resize(xdi as usize + 1, 0);
            w = 0;
            rd = 0;
            digits = 1;
            i %= LOG_BASE;
            j = i - LOG_BASE + 1;
        } else {
            w = xd[xdi as usize];
            digits = digit_count(w);
            i %= LOG_BASE;
            j = i - LOG_BASE + digits;
            rd = if j < 0 {
                0
            } else {
                div_pow10(w, digits - j - 1) % 10
            };
        }
    }

    let is_truncated = is_truncated
        || sd < 0
        || xd.get(xdi as usize + 1).is_some()
        || if j < 0 {
            w != 0
        } else {
            rem_pow10(w, digits - j - 1) != 0
        };

    let round_up = if rm < 4 {
        (rd != 0 || is_truncated) && (rm == 0 || rm == if s < 0 { 3 } else { 2 })
    } else {
        rd > 5
            || rd == 5
                && (rm == 4
                    || is_truncated
                    || rm == 6 && previous_digit_is_odd(xd, xdi, i, j, w, digits)
                    || rm == if s < 0 { 8 } else { 7 })
    };

    if sd < 1 || xd[0] == 0 {
        xd.clear();
        if round_up {
            // Convert sd to decimal places: 1, 0.1, 0.01, 0.001 etc.
            let sd = sd - (e + 1);
            xd.push(pow10((LOG_BASE - sd % LOG_BASE) % LOG_BASE).unwrap_or(1));
            return Rounded::Final { e: -sd };
        }
        xd.push(0);
        return Rounded::Final { e: 0 };
    }

    let mut e = e;
    let mut k: i64;
    if i == 0 {
        xd.truncate(xdi as usize);
        k = 1;
        xdi -= 1;
    } else {
        xd.truncate(xdi as usize + 1);
        k = pow10(LOG_BASE - i).unwrap_or(1);
        xd[xdi as usize] = if j > 0 {
            div_pow10(w, digits - j) % pow10(j).unwrap_or(i64::MAX) * k
        } else {
            0
        };
    }

    if round_up {
        loop {
            if xdi == 0 {
                let before = digit_count(xd[0]);
                xd[0] += k;
                let after = digit_count(xd[0]);
                if before != after {
                    e += 1;
                    if xd[0] == BASE {
                        xd[0] = 1;
                    }
                }
                break;
            }
            let at = xdi as usize;
            xd[at] += k;
            if xd[at] != BASE {
                break;
            }
            xd[at] = 0;
            xdi -= 1;
            k = 1;
        }
    }

    while xd.len() > 1 && xd.last() == Some(&0) {
        xd.pop();
    }
    Rounded::Done { e }
}

/// Whether the digit to the left of the rounding digit is odd (for ROUND_HALF_EVEN).
fn previous_digit_is_odd(xd: &[i64], xdi: i64, i: i64, j: i64, w: i64, digits: i64) -> bool {
    let digit = if i > 0 {
        if j > 0 {
            div_pow10(w, digits - j)
        } else {
            0
        }
    } else {
        usize::try_from(xdi - 1)
            .ok()
            .and_then(|at| xd.get(at))
            .copied()
            .unwrap_or(0)
    };
    (digit % 10) & 1 == 1
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dec(s: i8, e: i64, d: &[i64]) -> Decimal {
        Decimal {
            s,
            e,
            d: Some(d.to_vec()),
        }
    }

    #[test]
    fn rounds_half_up_within_a_word() {
        let cx = Context::default();
        // 1.2345 to 4 digits -> 1.235
        let x = dec(1, 0, &[1, 2345000]).round_to(&cx, 4, 4);
        assert_eq!(x, dec(1, 0, &[1, 2350000]));
        // 9.9999 to 2 digits -> 10
        let x = dec(1, 0, &[9, 9999000]).round_to(&cx, 2, 4);
        assert_eq!(x, dec(1, 1, &[10]));
    }

    #[test]
    fn rounds_to_zero_or_unit_below_the_first_digit() {
        let cx = Context::default();
        // 0.004 rounded to 0 decimal places (sd = -2) with ROUND_UP -> 1
        let x = dec(1, -3, &[40000]).round_to(&cx, -2, 0);
        assert_eq!(x, dec(1, 0, &[1]));
        let x = dec(1, -3, &[40000]).round_to(&cx, -2, 1);
        assert_eq!(x, dec(1, 0, &[0]));
    }

    #[test]
    fn half_even_looks_at_the_previous_digit() {
        let cx = Context::default();
        let x = dec(1, 0, &[2, 5000000]).round_to(&cx, 1, 6);
        assert_eq!(x, dec(1, 0, &[2]));
        let x = dec(1, 0, &[3, 5000000]).round_to(&cx, 1, 6);
        assert_eq!(x, dec(1, 0, &[4]));
    }

    #[test]
    fn overflow_and_underflow() {
        let cx = Context {
            max_e: 3,
            min_e: -3,
            ..Context::default()
        };
        assert_eq!(dec(1, 4, &[10000]).checked(&cx), Decimal::infinity(1));
        assert_eq!(dec(-1, -4, &[1000]).checked(&cx), Decimal::zero(-1));
    }
}
