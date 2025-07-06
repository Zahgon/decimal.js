//! The instance methods of Decimal, by name, as the `decimal` command calls them.

use super::arith::Converted;
use super::parse::construct;
use super::{word_exponent, Context, Decimal};
use crate::js::{JsError, Value};

/// The instance methods: each name and the method it is (aliases share an entry).
const METHODS: &[(&str, Method)] = &[
    ("absoluteValue", Method::Abs),
    ("abs", Method::Abs),
    ("ceil", Method::Ceil),
    ("clampedTo", Method::Clamp),
    ("clamp", Method::Clamp),
    ("comparedTo", Method::Cmp),
    ("cmp", Method::Cmp),
    ("cosine", Method::Cos),
    ("cos", Method::Cos),
    ("cubeRoot", Method::Cbrt),
    ("cbrt", Method::Cbrt),
    ("decimalPlaces", Method::Dp),
    ("dp", Method::Dp),
    ("dividedBy", Method::Div),
    ("div", Method::Div),
    ("dividedToIntegerBy", Method::DivToInt),
    ("divToInt", Method::DivToInt),
    ("equals", Method::Eq),
    ("eq", Method::Eq),
    ("floor", Method::Floor),
    ("greaterThan", Method::Gt),
    ("gt", Method::Gt),
    ("greaterThanOrEqualTo", Method::Gte),
    ("gte", Method::Gte),
    ("hyperbolicCosine", Method::Cosh),
    ("cosh", Method::Cosh),
    ("hyperbolicSine", Method::Sinh),
    ("sinh", Method::Sinh),
    ("hyperbolicTangent", Method::Tanh),
    ("tanh", Method::Tanh),
    ("inverseCosine", Method::Acos),
    ("acos", Method::Acos),
    ("inverseHyperbolicCosine", Method::Acosh),
    ("acosh", Method::Acosh),
    ("inverseHyperbolicSine", Method::Asinh),
    ("asinh", Method::Asinh),
    ("inverseHyperbolicTangent", Method::Atanh),
    ("atanh", Method::Atanh),
    ("inverseSine", Method::Asin),
    ("asin", Method::Asin),
    ("inverseTangent", Method::Atan),
    ("atan", Method::Atan),
    ("isFinite", Method::IsFinite),
    ("isInteger", Method::IsInt),
    ("isInt", Method::IsInt),
    ("isNaN", Method::IsNaN),
    ("isNegative", Method::IsNeg),
    ("isNeg", Method::IsNeg),
    ("isPositive", Method::IsPos),
    ("isPos", Method::IsPos),
    ("isZero", Method::IsZero),
    ("lessThan", Method::Lt),
    ("lt", Method::Lt),
    ("lessThanOrEqualTo", Method::Lte),
    ("lte", Method::Lte),
    ("logarithm", Method::Log),
    ("log", Method::Log),
    ("minus", Method::Minus),
    ("sub", Method::Minus),
    ("modulo", Method::Mod),
    ("mod", Method::Mod),
    ("naturalExponential", Method::Exp),
    ("exp", Method::Exp),
    ("naturalLogarithm", Method::Ln),
    ("ln", Method::Ln),
    ("negated", Method::Neg),
    ("neg", Method::Neg),
    ("plus", Method::Plus),
    ("add", Method::Plus),
    ("precision", Method::Sd),
    ("sd", Method::Sd),
    ("round", Method::Round),
    ("sine", Method::Sin),
    ("sin", Method::Sin),
    ("squareRoot", Method::Sqrt),
    ("sqrt", Method::Sqrt),
    ("tangent", Method::Tan),
    ("tan", Method::Tan),
    ("times", Method::Times),
    ("mul", Method::Times),
    ("toBinary", Method::ToBinary),
    ("toDecimalPlaces", Method::ToDp),
    ("toDP", Method::ToDp),
    ("toExponential", Method::ToExponential),
    ("toFixed", Method::ToFixed),
    ("toFraction", Method::ToFraction),
    ("toHexadecimal", Method::ToHex),
    ("toHex", Method::ToHex),
    ("toNearest", Method::ToNearest),
    ("toNumber", Method::ToNumber),
    ("toOctal", Method::ToOctal),
    ("toPower", Method::Pow),
    ("pow", Method::Pow),
    ("toPrecision", Method::ToPrecision),
    ("toSignificantDigits", Method::ToSd),
    ("toSD", Method::ToSd),
    ("toString", Method::ToString),
    ("truncated", Method::Trunc),
    ("trunc", Method::Trunc),
    ("valueOf", Method::ValueOf),
    ("toJSON", Method::ValueOf),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Method {
    Abs,
    Ceil,
    Clamp,
    Cmp,
    Cos,
    Cbrt,
    Dp,
    Div,
    DivToInt,
    Eq,
    Floor,
    Gt,
    Gte,
    Cosh,
    Sinh,
    Tanh,
    Acos,
    Acosh,
    Asinh,
    Atanh,
    Asin,
    Atan,
    IsFinite,
    IsInt,
    IsNaN,
    IsNeg,
    IsPos,
    IsZero,
    Lt,
    Lte,
    Log,
    Minus,
    Mod,
    Exp,
    Ln,
    Neg,
    Plus,
    Sd,
    Round,
    Sin,
    Sqrt,
    Tan,
    Times,
    ToBinary,
    ToDp,
    ToExponential,
    ToFixed,
    ToFraction,
    ToHex,
    ToNearest,
    ToNumber,
    ToOctal,
    Pow,
    ToPrecision,
    ToSd,
    ToString,
    Trunc,
    ValueOf,
}

fn lookup(name: &str) -> Option<Method> {
    METHODS
        .iter()
        .find(|(n, _)| *n == name)
        .map(|&(_, method)| method)
}

/// Whether `name` is an instance method of Decimal (including aliases).
pub fn is_method(name: &str) -> bool {
    lookup(name).is_some()
}

/// The argument at `i`, `undefined` if there are fewer arguments.
pub(crate) fn arg(args: &[Value], i: usize) -> &Value {
    args.get(i).unwrap_or(&Value::Undefined)
}

/// The result of `cmp` (-1, 0, 1 or NaN) after converting the argument.
fn compare(cx: &mut Context, x: &Decimal, y: &Value) -> Result<f64, JsError> {
    let y = construct(cx, y)?;
    Ok(x.cmp_value(cx, Converted(y)))
}

/// `x.isInteger()`.
pub(crate) fn is_integer(x: &Decimal) -> bool {
    match &x.d {
        Some(d) => word_exponent(x.e) > d.len() as i64 - 2,
        None => false,
    }
}

/// Call the instance method `name` of `x` with `args`, as `x[name](...args)`.
///
/// # Panics
///
/// If `name` is not a method (see [`is_method`]).
pub fn call_method(
    cx: &mut Context,
    x: &Decimal,
    name: &str,
    args: &[Value],
) -> Result<Value, JsError> {
    let method = lookup(name).unwrap_or_else(|| panic!("not a Decimal method: {name}"));
    let a0 = arg(args, 0);
    let a1 = arg(args, 1);

    let decimal = |d: Decimal| Ok(Value::Decimal(d));

    match method {
        Method::Abs => decimal(x.abs(cx)),
        Method::Ceil => decimal(x.to_integer(cx, 2)),
        Method::Floor => decimal(x.to_integer(cx, 3)),
        Method::Round => {
            let rm = cx.rounding;
            decimal(x.to_integer(cx, rm))
        }
        Method::Trunc => decimal(x.to_integer(cx, 1)),
        Method::Clamp => {
            let min = construct(cx, a0)?;
            let max = construct(cx, a1)?;
            if min.s == 0 || max.s == 0 {
                return decimal(Decimal::nan());
            }
            if min.gt(cx, &max) {
                return Err(JsError::invalid_argument(&max.value_of(cx)));
            }
            if x.cmp_value(cx, &min) < 0.0 {
                decimal(min)
            } else if x.cmp_value(cx, &max) > 0.0 {
                decimal(max)
            } else {
                decimal(Decimal::copy_of(cx, x))
            }
        }
        Method::Cmp => Ok(Value::Number(compare(cx, x, a0)?)),
        Method::Eq => Ok(Value::Bool(compare(cx, x, a0)? == 0.0)),
        Method::Gt => Ok(Value::Bool(compare(cx, x, a0)? > 0.0)),
        Method::Gte => {
            let k = compare(cx, x, a0)?;
            Ok(Value::Bool(k == 1.0 || k == 0.0))
        }
        Method::Lt => Ok(Value::Bool(compare(cx, x, a0)? < 0.0)),
        Method::Lte => {
            let k = compare(cx, x, a0)?;
            Ok(Value::Bool(k == -1.0 || k == 0.0))
        }
        Method::Cos => decimal(x.cos(cx)?),
        Method::Sin => decimal(x.sin(cx)?),
        Method::Tan => decimal(x.tan(cx)?),
        Method::Acos => decimal(x.acos(cx)?),
        Method::Asin => decimal(x.asin(cx)?),
        Method::Atan => decimal(x.atan(cx)?),
        Method::Cosh => decimal(x.cosh(cx)),
        Method::Sinh => decimal(x.sinh(cx)),
        Method::Tanh => decimal(x.tanh(cx)),
        Method::Acosh => decimal(x.acosh(cx)?),
        Method::Asinh => decimal(x.asinh(cx)?),
        Method::Atanh => decimal(x.atanh(cx)?),
        Method::Cbrt => decimal(x.cbrt(cx)),
        Method::Sqrt => decimal(x.sqrt(cx)),
        Method::Exp => decimal(x.exp(cx)),
        Method::Ln => decimal(x.ln(cx)?),
        Method::Log => {
            if a0.is_nullish() {
                decimal(x.log(cx, None)?)
            } else {
                let base = construct(cx, a0)?;
                decimal(x.log(cx, Some(base))?)
            }
        }
        Method::Pow => {
            let y = construct(cx, a0)?;
            decimal(x.pow(cx, Converted(y))?)
        }
        Method::Dp => Ok(Value::Number(x.decimal_places())),
        Method::Sd => Ok(Value::Number(x.significant_digits(cx, a0)?)),
        Method::Div => {
            let y = construct(cx, a0)?;
            decimal(x.div(cx, Converted(y)))
        }
        Method::DivToInt => {
            let y = construct(cx, a0)?;
            decimal(x.div_to_int(cx, Converted(y)))
        }
        Method::Mod => {
            let y = construct(cx, a0)?;
            decimal(x.modulo(cx, Converted(y)))
        }
        Method::Minus => {
            let y = construct(cx, a0)?;
            decimal(x.minus(cx, Converted(y)))
        }
        Method::Plus => {
            let y = construct(cx, a0)?;
            decimal(x.plus(cx, Converted(y)))
        }
        Method::Times => {
            let y = construct(cx, a0)?;
            decimal(x.times(cx, Converted(y)))
        }
        Method::Neg => decimal(x.neg(cx)),
        Method::IsFinite => Ok(Value::Bool(x.is_finite())),
        Method::IsInt => Ok(Value::Bool(is_integer(x))),
        Method::IsNaN => Ok(Value::Bool(x.is_nan())),
        Method::IsNeg => Ok(Value::Bool(x.is_neg())),
        Method::IsPos => Ok(Value::Bool(x.is_pos())),
        Method::IsZero => Ok(Value::Bool(x.is_zero())),
        Method::ToBinary => Ok(Value::String(x.to_string_binary(cx, 2, a0, a1)?)),
        Method::ToHex => Ok(Value::String(x.to_string_binary(cx, 16, a0, a1)?)),
        Method::ToOctal => Ok(Value::String(x.to_string_binary(cx, 8, a0, a1)?)),
        Method::ToDp => decimal(x.to_dp(cx, a0, a1)?),
        Method::ToSd => decimal(x.to_sd(cx, a0, a1)?),
        Method::ToExponential => Ok(Value::String(x.to_exponential(cx, a0, a1)?)),
        Method::ToFixed => Ok(Value::String(x.to_fixed(cx, a0, a1)?)),
        Method::ToPrecision => Ok(Value::String(x.to_precision(cx, a0, a1)?)),
        Method::ToFraction => x.to_fraction(cx, a0),
        Method::ToNearest => decimal(x.to_nearest(cx, a0, a1)?),
        Method::ToNumber => Ok(Value::Number(x.to_number(cx))),
        Method::ToString => Ok(Value::String(x.to_js_string(cx))),
        Method::ValueOf => Ok(Value::String(x.value_of(cx))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(cx: &mut Context, s: &str) -> Decimal {
        construct(cx, &Value::String(s.to_string())).unwrap()
    }

    fn call(cx: &mut Context, x: &str, name: &str, args: &[Value]) -> String {
        let x = d(cx, x);
        match call_method(cx, &x, name, args) {
            Ok(v) => v.to_js_string(cx),
            Err(e) => e.to_string(),
        }
    }

    #[test]
    fn aliases_are_methods() {
        assert!(is_method("toJSON"));
        assert!(is_method("clampedTo"));
        assert!(!is_method("constructor"));
        assert!(!is_method("toLocaleString"));
    }

    #[test]
    fn methods_convert_their_arguments() {
        let mut cx = Context::default();
        let s = |v: &str| Value::String(v.to_string());
        assert_eq!(call(&mut cx, "1.5", "plus", &[s("2.25")]), "3.75");
        assert_eq!(call(&mut cx, "1", "cmp", &[Value::Number(f64::NAN)]), "NaN");
        assert_eq!(
            call(&mut cx, "1", "plus", &[]),
            "Error: [DecimalError] Invalid argument: undefined"
        );
        assert_eq!(call(&mut cx, "5", "clamp", &[s("1"), s("3")]), "3");
        assert_eq!(
            call(&mut cx, "5", "clamp", &[s("3"), s("1")]),
            "Error: [DecimalError] Invalid argument: 1"
        );
        assert_eq!(call(&mut cx, "2", "gte", &[s("2")]), "true");
        assert_eq!(call(&mut cx, "1e21", "isInt", &[]), "true");
        assert_eq!(call(&mut cx, "1.5", "isInt", &[]), "false");
    }
}
