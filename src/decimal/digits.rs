//! Helpers on the base 1e7 digit words of a Decimal.

#[cfg(test)]
use super::BASE;
use super::LOG_BASE;

/// `10^n` for `0 <= n <= 18`, `None` beyond (where JavaScript's `Math.pow(10, n)` stops being an
/// exact integer that matters for 7-digit words).
pub(crate) fn pow10(n: i64) -> Option<i64> {
    if (0..=18).contains(&n) {
        Some(10i64.pow(n as u32))
    } else {
        None
    }
}

/// `w / Math.pow(10, n) | 0` for a word `w >= 0` and `n >= 0`.
pub(crate) fn div_pow10(w: i64, n: i64) -> i64 {
    match pow10(n) {
        Some(p) => w / p,
        None => 0,
    }
}

/// `w % Math.pow(10, n)` for a word `w >= 0` and `n >= 0`.
pub(crate) fn rem_pow10(w: i64, n: i64) -> i64 {
    match pow10(n) {
        Some(p) => w % p,
        None => w,
    }
}

/// The number of decimal digits of a word (1 for 0).
pub(crate) fn digit_count(mut w: i64) -> i64 {
    let mut n = 1;
    while w >= 10 {
        w /= 10;
        n += 1;
    }
    n
}

/// `getZeroString(k)`: `k` zeros.
pub(crate) fn zeros(k: i64) -> String {
    if k > 0 {
        "0".repeat(k as usize)
    } else {
        String::new()
    }
}

/// `digitsToString(d)`: the significant digits of the words, without trailing zeros.
pub(crate) fn digits_to_string(d: &[i64]) -> String {
    let Some(&first) = d.first() else {
        return String::new();
    };
    let last = d.len() - 1;
    let mut out = String::new();
    let mut w = first;
    if last > 0 {
        out.push_str(&first.to_string());
        for &word in &d[1..last] {
            let ws = word.to_string();
            out.push_str(&zeros(LOG_BASE - ws.len() as i64));
            out.push_str(&ws);
        }
        w = d[last];
        let ws = w.to_string();
        out.push_str(&zeros(LOG_BASE - ws.len() as i64));
    } else if w == 0 {
        return "0".to_string();
    }
    while w != 0 && w % 10 == 0 {
        w /= 10;
    }
    out.push_str(&w.to_string());
    out
}

/// `getPrecision(digits)`: the number of significant digits.
pub(crate) fn get_precision(d: &[i64]) -> i64 {
    let w = d.len() as i64 - 1;
    let mut len = w * LOG_BASE + 1;
    let mut last = d[w as usize];
    if last != 0 {
        while last % 10 == 0 {
            last /= 10;
            len -= 1;
        }
        len += digit_count(d[0]) - 1;
    }
    len
}

/// `getBase10Exponent(digits, e)`: the base-10 exponent of words whose first word has the base 1e7
/// exponent `e`.
///
/// decimal.js keeps exponents as JavaScript numbers; exponents beyond `i64` saturate here, far outside
/// any `minE`/`maxE`, so the range checks still turn such values into zero or Infinity.
pub(crate) fn get_base10_exponent(d: &[i64], e: i64) -> i64 {
    e.saturating_mul(LOG_BASE)
        .saturating_add(d.first().map_or(1, |&w| digit_count(w)) - 1)
}

/// `truncate(arr, len)`: shorten the words to `len`, reporting whether any were removed.
pub(crate) fn truncate(arr: &mut Vec<i64>, len: i64) -> bool {
    let len = len.max(0) as usize;
    if arr.len() > len {
        arr.truncate(len);
        true
    } else {
        false
    }
}

/// The value of the character `c` among `0123456789abcdef`, or -1 (`NUMERALS.indexOf(c)`).
fn numeral_value(c: char) -> i64 {
    match c {
        '0'..='9' => c as i64 - '0' as i64,
        'a'..='f' => c as i64 - 'a' as i64 + 10,
        _ => -1,
    }
}

/// The character of a digit value (`NUMERALS.charAt(n)`), empty for values outside 0 to 15.
pub(crate) fn numeral(n: i64) -> &'static str {
    const NUMERALS: [&str; 16] = [
        "0", "1", "2", "3", "4", "5", "6", "7", "8", "9", "a", "b", "c", "d", "e", "f",
    ];
    usize::try_from(n)
        .ok()
        .and_then(|i| NUMERALS.get(i))
        .copied()
        .unwrap_or("")
}

/// `convertBase(str, baseIn, baseOut)`: the digits of `str` (in base `base_in`) in base `base_out`,
/// most significant first.
pub(crate) fn convert_base(s: &str, base_in: i64, base_out: i64) -> Vec<i64> {
    let mut arr: Vec<i64> = vec![0];
    for c in s.chars() {
        for v in arr.iter_mut() {
            *v *= base_in;
        }
        arr[0] += numeral_value(c);
        let mut j = 0;
        while j < arr.len() {
            if arr[j] > base_out - 1 {
                if j + 1 == arr.len() {
                    arr.push(0);
                }
                arr[j + 1] += arr[j] / base_out;
                arr[j] %= base_out;
            }
            j += 1;
        }
    }
    arr.reverse();
    arr
}

/// `checkRoundingDigits(d, i, rm, repeating)`: whether the rounding digits of `d` starting at the
/// digit index `i` are at a rounding boundary (5 digits are checked if `repeating` is `None`, 4
/// otherwise).
pub(crate) fn check_rounding_digits(d: &[i64], i: i64, rm: i64, repeating: Option<bool>) -> bool {
    let mut i = i - (digit_count(d[0]) - 1);
    let di: usize;
    i -= 1;
    if i < 0 {
        i += LOG_BASE;
        di = 0;
    } else {
        di = ((i + 1 + LOG_BASE - 1) / LOG_BASE) as usize;
        i %= LOG_BASE;
    }
    let k = 10i64.pow((LOG_BASE - i) as u32);
    let mut rd = d.get(di).map_or(0, |w| w % k);
    let next = d.get(di + 1).copied();
    match repeating {
        None => {
            if i < 3 {
                if i == 0 {
                    rd /= 100;
                } else if i == 1 {
                    rd /= 10;
                }
                rm < 4 && rd == 99999 || rm > 3 && rd == 49999 || rd == 50000 || rd == 0
            } else {
                let next_digits = next.map_or(0, |w| w / k / 100);
                (rm < 4 && rd + 1 == k || rm > 3 && rd + 1 == k / 2)
                    && next_digits == 10i64.pow((i - 2) as u32) - 1
                    || (rd == k / 2 || rd == 0) && next_digits == 0
            }
        }
        Some(repeating) => {
            if i < 4 {
                if i == 0 {
                    rd /= 1000;
                } else if i == 1 {
                    rd /= 100;
                } else if i == 2 {
                    rd /= 10;
                }
                (repeating || rm < 4) && rd == 9999 || !repeating && rm > 3 && rd == 4999
            } else {
                let next_digits = next.map_or(0, |w| w / k / 1000);
                ((repeating || rm < 4) && rd + 1 == k || (!repeating && rm > 3) && rd + 1 == k / 2)
                    && next_digits == 10i64.pow((i - 3) as u32) - 1
            }
        }
    }
}

/// The value of a JavaScript number that holds a digit word (`+str.slice(...)`).
pub(crate) fn word_value(digits: &str) -> i64 {
    digits
        .bytes()
        .fold(0, |acc, b| acc * 10 + i64::from(b - b'0'))
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn digits_strings() {
        assert_eq!(digits_to_string(&[0]), "0");
        assert_eq!(digits_to_string(&[12, 3400000]), "1234");
        assert_eq!(digits_to_string(&[1, 50, 7]), "100000500000007");
        assert_eq!(digits_to_string(&[5000]), "5");
    }

    #[test]
    fn precision_of_words() {
        assert_eq!(get_precision(&[0]), 1);
        assert_eq!(get_precision(&[12, 3400000]), 4);
        assert_eq!(get_precision(&[1000]), 1);
        assert_eq!(get_precision(&[1234567]), 7);
    }

    #[test]
    fn base_conversion() {
        assert_eq!(convert_base("255", 10, 16), [15, 15]);
        assert_eq!(convert_base("ff", 16, 10), [2, 5, 5]);
        assert_eq!(convert_base("0", 10, 2), [0]);
        assert_eq!(convert_base("12345678", 10, BASE), [1, 2345678]);
    }

    #[test]
    fn rounding_digit_checks() {
        // 1.2345|49999: at a boundary for the half modes after 5 significant digits
        assert!(check_rounding_digits(&[1, 2345499, 9900000], 5, 4, None));
        assert!(!check_rounding_digits(&[1, 2345123, 4500000], 5, 4, None));
        assert!(check_rounding_digits(&[1, 2349999], 4, 1, Some(false)));
    }
}
