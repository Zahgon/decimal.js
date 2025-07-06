//! An arbitrary-precision decimal type, ported from decimal.js v10.6.0
//! (https://github.com/MikeMcl/decimal.js).
//!
//! Copyright (c) 2025 Michael Mclaughlin <M8ch88l@gmail.com>
//! MIT Licence (see LICENCE.md)
//!
//! A [`Decimal`] is an immutable-looking value whose operations read and update one shared
//! [`Context`]: the configuration of the Decimal constructor (`precision`, `rounding`, `toExpNeg`,
//! `toExpPos`, `minE`, `maxE`, `modulo`, `crypto`) and the module state decimal.js keeps between
//! calls (`external`, `quadrant`, `inexact`). Operations change the configuration while they run
//! and, when they throw, leave it as it was at the throw, exactly as decimal.js does.

mod arith;
mod digits;
mod finalise;
mod format;
mod methods;
mod parse;
mod statics;
mod transcendental;
mod trig;

pub use arith::{Converted, Operand};
pub use methods::{call_method, is_method};
pub use statics::{call_static, is_static, set_config};

use crate::js::{JsError, Value};

/// The maximum exponent magnitude, and the limit of `toExpNeg`, `toExpPos`, `minE` and `maxE`.
pub const EXP_LIMIT: i64 = 9_000_000_000_000_000;

/// The limit of `precision` and of the digit arguments of the formatting methods.
pub const MAX_DIGITS: i64 = 1_000_000_000;

/// The base of the digit words of a Decimal.
pub(crate) const BASE: i64 = 10_000_000;

/// The number of decimal digits in a full digit word.
pub(crate) const LOG_BASE: i64 = 7;

/// `Number.MAX_SAFE_INTEGER`.
pub(crate) const MAX_SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;

/// The natural logarithm of 10 (1025 digits).
pub(crate) const LN10: &str = "2.3025850929940456840179914546843642076011014886287729760333279009675726096773524802359972050895982983419677840422862486334095254650828067566662873690987816894829072083255546808437998948262331985283935053089653777326288461633662222876982198867465436674744042432743651550489343149393914796194044002221051017141748003688084012647080685567743216228355220114804663715659121373450747856947683463616792101806445070648000277502684916746550586856935673420670581136429224554405758925724208241314695689016758940256776311356919292033376587141660230105703089634572075440370847469940168269282808481184289314848524948644871927809676271275775397027668605952496716674183485704422507197965004714951050492214776567636938662976979522110718264549734772662425709429322582798502585509785265383207606726317164309505995087807523710333101197857547331541421808427543863591778117054309827482385045648019095610299291824318237525357709750539565187697510374970888692180205189339507238539205144634197265287286965110862571492198849978748873771345686209167058";

/// Pi (1025 digits).
pub(crate) const PI: &str = "3.1415926535897932384626433832795028841971693993751058209749445923078164062862089986280348253421170679821480865132823066470938446095505822317253594081284811174502841027019385211055596446229489549303819644288109756659334461284756482337867831652712019091456485669234603486104543266482133936072602491412737245870066063155881748815209209628292540917153643678925903600113305305488204665213841469519415116094330572703657595919530921861173819326117931051185480744623799627495673518857527248912279381830119491298336733624406566430860213949463952247371907021798609437027705392171762931767523846748184676694051320005681271452635608277857713427577896091736371787214684409012249534301465495853710507922796892589235420199561121290219608640344181598136297747713099605187072113499999983729780499510597317328160963185950244594553469083026425223082533446850352619311881710100031378387528865875332083814206171776691473035982534904287554687311595628638823537875937519577818577805321712268066130019278766111959092164201989380952572010654858632789";

/// The number of significant digits of `LN10`.
pub(crate) const LN10_PRECISION: i64 = 1025;

/// The number of significant digits of `PI`.
pub(crate) const PI_PRECISION: i64 = 1025;

/// A decimal number: NaN, ±Infinity, ±0 or a finite non-zero value.
///
/// The representation is that of decimal.js: a sign `s` (1 or -1; 0 stands for NaN), a base-10
/// exponent `e`, and the significand as base 1e7 words `d`, most significant first, the first word
/// without leading zeros and the last without trailing zero words. `d` is `None` for NaN and
/// ±Infinity, and `[0]` for ±0.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Decimal {
    pub(crate) s: i8,
    pub(crate) e: i64,
    pub(crate) d: Option<Vec<i64>>,
}

impl Decimal {
    /// NaN.
    pub fn nan() -> Decimal {
        Decimal {
            s: 0,
            e: 0,
            d: None,
        }
    }

    /// Infinity with the sign of `s`.
    pub fn infinity(s: i8) -> Decimal {
        Decimal {
            s: if s < 0 { -1 } else { 1 },
            e: 0,
            d: None,
        }
    }

    /// Zero with the sign of `s`.
    pub fn zero(s: i8) -> Decimal {
        Decimal {
            s: if s < 0 { -1 } else { 1 },
            e: 0,
            d: Some(vec![0]),
        }
    }

    /// `x.isFinite()`.
    pub fn is_finite(&self) -> bool {
        self.d.is_some()
    }

    /// The digit words where decimal.js reads `x.d.length` without checking that `x` is finite: for
    /// NaN and the infinities that read is JavaScript's `TypeError` on `null`.
    pub(crate) fn words_or_null_error(&self) -> Result<&[i64], JsError> {
        self.d
            .as_deref()
            .ok_or_else(|| JsError::null_property("length"))
    }

    /// [`Decimal::words_or_null_error`] for an update of the digit words in place.
    pub(crate) fn words_mut_or_null_error(&mut self) -> Result<&mut Vec<i64>, JsError> {
        self.d
            .as_mut()
            .ok_or_else(|| JsError::null_property("length"))
    }

    /// `x.isNaN()`.
    pub fn is_nan(&self) -> bool {
        self.s == 0
    }

    /// `x.isZero()`.
    pub fn is_zero(&self) -> bool {
        matches!(&self.d, Some(d) if d.first() == Some(&0))
    }

    /// `x.isNeg()`.
    pub fn is_neg(&self) -> bool {
        self.s < 0
    }

    /// `x.isPos()`.
    pub fn is_pos(&self) -> bool {
        self.s > 0
    }

    /// The sign as a JavaScript number: 1, -1 or NaN.
    pub(crate) fn sign_number(&self) -> f64 {
        match self.s {
            0 => f64::NAN,
            s => f64::from(s),
        }
    }

    /// The digit words of a finite Decimal.
    pub(crate) fn words(&self) -> &[i64] {
        self.d.as_deref().unwrap_or(&[])
    }

    /// The first digit word, or `None` for NaN and ±Infinity (`x.d && x.d[0]`).
    pub(crate) fn first_word(&self) -> Option<i64> {
        self.d.as_ref().and_then(|d| d.first().copied())
    }
}

/// The configuration of the Decimal constructor and the module state of decimal.js.
#[derive(Clone, Debug)]
pub struct Context {
    /// The maximum number of significant digits of the result of an operation.
    pub precision: i64,
    /// The rounding mode, 0 to 8.
    pub rounding: i64,
    /// The exponent at and beneath which `toString` uses exponential notation.
    pub to_exp_neg: i64,
    /// The exponent at and above which `toString` uses exponential notation.
    pub to_exp_pos: i64,
    /// The exponent beneath which a result underflows to zero.
    pub min_e: i64,
    /// The exponent above which a result overflows to Infinity.
    pub max_e: i64,
    /// The modulo mode, 0 to 9.
    pub modulo: i64,
    /// Whether random numbers come from a cryptographically secure source.
    pub crypto: bool,
    /// Whether results are rounded and range-checked (false inside multi-step calculations).
    pub(crate) external: bool,
    /// The quadrant found by the last argument reduction of `sin`, `cos` or `tan` (0: none yet).
    pub(crate) quadrant: i64,
    /// Whether the last base conversion division was inexact.
    pub(crate) inexact: bool,
    ln10: Decimal,
    pi: Decimal,
}

impl Default for Context {
    fn default() -> Context {
        let mut cx = Context {
            precision: 20,
            rounding: 4,
            to_exp_neg: -7,
            to_exp_pos: 21,
            min_e: -EXP_LIMIT,
            max_e: EXP_LIMIT,
            modulo: 1,
            crypto: false,
            external: true,
            quadrant: 0,
            inexact: false,
            ln10: Decimal::nan(),
            pi: Decimal::nan(),
        };
        cx.ln10 = parse::parse_decimal(&cx, 1, LN10);
        cx.pi = parse::parse_decimal(&cx, 1, PI);
        cx
    }
}

impl Context {
    /// A context with the default configuration.
    pub fn new() -> Context {
        Context::default()
    }

    /// Restore the default configuration (`Decimal.set({ defaults: true })` with no other keys).
    pub fn reset_defaults(&mut self) {
        let defaults = Context::default();
        self.precision = defaults.precision;
        self.rounding = defaults.rounding;
        self.to_exp_neg = defaults.to_exp_neg;
        self.to_exp_pos = defaults.to_exp_pos;
        self.min_e = defaults.min_e;
        self.max_e = defaults.max_e;
        self.modulo = defaults.modulo;
        self.crypto = defaults.crypto;
    }

    /// The configuration as `{"precision":…,"rounding":…,…,"crypto":…}` JSON.
    pub fn config_json(&self) -> String {
        format!(
            "{{\"precision\":{},\"rounding\":{},\"toExpNeg\":{},\"toExpPos\":{},\"minE\":{},\"maxE\":{},\"modulo\":{},\"crypto\":{}}}",
            self.precision,
            self.rounding,
            self.to_exp_neg,
            self.to_exp_pos,
            self.min_e,
            self.max_e,
            self.modulo,
            self.crypto
        )
    }
}

/// Construct a Decimal from a JavaScript value: `new Decimal(v)`.
pub fn new_decimal(cx: &mut Context, v: &Value) -> Result<Decimal, JsError> {
    parse::construct(cx, v)
}

/// The Decimal of a JavaScript number: `new Decimal(n)`.
pub fn from_number(cx: &Context, n: f64) -> Decimal {
    parse::from_number(cx, n)
}

/// `Math.floor(e / LOG_BASE)`: the exponent of the digit word that holds the digit of exponent `e`.
pub(crate) fn word_exponent(e: i64) -> i64 {
    e.div_euclid(LOG_BASE)
}

/// `Math.ceil(n / LOG_BASE)` for an integer `n`.
pub(crate) fn ceil_words(n: i64) -> i64 {
    -(-n).div_euclid(LOG_BASE)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn word_exponents_floor() {
        assert_eq!(word_exponent(-1), -1);
        assert_eq!(word_exponent(-7), -1);
        assert_eq!(word_exponent(-8), -2);
        assert_eq!(word_exponent(6), 0);
        assert_eq!(word_exponent(7), 1);
        assert_eq!(ceil_words(0), 0);
        assert_eq!(ceil_words(1), 1);
        assert_eq!(ceil_words(7), 1);
        assert_eq!(ceil_words(8), 2);
        assert_eq!(ceil_words(-8), -1);
    }

    #[test]
    fn default_config_json() {
        assert_eq!(
            Context::default().config_json(),
            "{\"precision\":20,\"rounding\":4,\"toExpNeg\":-7,\"toExpPos\":21,\"minE\":-9000000000000000,\"maxE\":9000000000000000,\"modulo\":1,\"crypto\":false}"
        );
    }
}
