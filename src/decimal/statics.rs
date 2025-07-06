//! The static functions of the Decimal constructor and `Decimal.set`.

use super::arith::Converted;
use super::methods::{arg, call_method};
use super::parse::{construct, from_number};
use super::{trig, Context, Decimal, EXP_LIMIT, MAX_DIGITS};
use crate::js::{JsError, Value};

/// The static functions the `decimal` command offers.
const STATICS: &[&str] = &[
    "abs",
    "acos",
    "acosh",
    "add",
    "asin",
    "asinh",
    "atan",
    "atanh",
    "atan2",
    "cbrt",
    "ceil",
    "clamp",
    "cos",
    "cosh",
    "div",
    "exp",
    "floor",
    "hypot",
    "isDecimal",
    "ln",
    "log",
    "log10",
    "log2",
    "max",
    "min",
    "mod",
    "mul",
    "pow",
    "round",
    "sign",
    "sin",
    "sinh",
    "sqrt",
    "sub",
    "sum",
    "tan",
    "tanh",
    "trunc",
];

/// The value `String(Object.prototype.toString.call(decimal))`, by which decimal.js also
/// recognises Decimals of other constructors.
const TAG: &str = "[object Decimal]";

/// Whether `name` is a static function of Decimal.
pub fn is_static(name: &str) -> bool {
    STATICS.contains(&name)
}

/// Call the static function `Decimal[name]` with `args`.
///
/// # Panics
///
/// If `name` is not a static function (see [`is_static`]).
pub fn call_static(cx: &mut Context, name: &str, args: &[Value]) -> Result<Value, JsError> {
    let decimal = |d: Decimal| Ok(Value::Decimal(d));
    match name {
        "atan2" => {
            let y = construct(cx, arg(args, 0))?;
            let x = construct(cx, arg(args, 1))?;
            decimal(trig::atan2(cx, &y, &x)?)
        }
        "ceil" => integer(cx, arg(args, 0), 2),
        "floor" => integer(cx, arg(args, 0), 3),
        "round" => {
            let rm = cx.rounding;
            integer(cx, arg(args, 0), rm)
        }
        "trunc" => integer(cx, arg(args, 0), 1),
        "hypot" => decimal(hypot(cx, args)?),
        "isDecimal" => Ok(Value::Bool(is_decimal_instance(arg(args, 0)))),
        "log2" => method(cx, args, "log", &[Value::Number(2.0)]),
        "log10" => method(cx, args, "log", &[Value::Number(10.0)]),
        "max" => decimal(max_or_min(cx, args, -1.0)?),
        "min" => decimal(max_or_min(cx, args, 1.0)?),
        "sign" => {
            let x = construct(cx, arg(args, 0))?;
            Ok(Value::Number(match x.first_word() {
                Some(0) => 0.0 * x.sign_number(),
                Some(_) => x.sign_number(),
                None if x.s == 0 => f64::NAN,
                None => x.sign_number(),
            }))
        }
        "sum" => decimal(sum(cx, args)?),
        "abs" | "acos" | "acosh" | "asin" | "asinh" | "atan" | "atanh" | "cbrt" | "cos"
        | "cosh" | "exp" | "ln" | "sin" | "sinh" | "sqrt" | "tan" | "tanh" => {
            method(cx, args, name, &[])
        }
        "add" => method(cx, args, "plus", following(args, 1)),
        "sub" => method(cx, args, "minus", following(args, 1)),
        "mul" => method(cx, args, "times", following(args, 1)),
        "div" | "mod" | "pow" | "log" => method(cx, args, name, following(args, 1)),
        "clamp" => method(cx, args, "clamp", following(args, 2)),
        _ => panic!("not a Decimal static function: {name}"),
    }
}

/// Up to `n` arguments after the first.
fn following(args: &[Value], n: usize) -> &[Value] {
    let start = args.len().min(1);
    &args[start..args.len().min(1 + n)]
}

/// `new this(x)[name](...rest)`.
fn method(cx: &mut Context, args: &[Value], name: &str, rest: &[Value]) -> Result<Value, JsError> {
    let x = construct(cx, arg(args, 0))?;
    call_method(cx, &x, name, rest)
}

/// `finalise(x = new this(x), x.e + 1, rm)`.
fn integer(cx: &mut Context, x: &Value, rm: i64) -> Result<Value, JsError> {
    let x = construct(cx, x)?;
    let sd = x.e + 1;
    Ok(Value::Decimal(x.finalised(cx, Some(sd), rm, false)))
}

/// `isDecimalInstance(obj)`.
fn is_decimal_instance(v: &Value) -> bool {
    match v {
        Value::Decimal(_) => true,
        Value::Object(entries) => entries
            .iter()
            .any(|(k, v)| k == "toStringTag" && matches!(v, Value::String(s) if s == TAG)),
        _ => false,
    }
}

/// `Decimal.hypot(...args)`: the square root of the sum of the squares of the arguments.
fn hypot(cx: &mut Context, args: &[Value]) -> Result<Decimal, JsError> {
    let mut t = from_number(cx, 0.0);

    cx.external = false;

    for v in args {
        let n = construct(cx, v)?;
        if !n.is_finite() {
            if n.s != 0 {
                cx.external = true;
                return Ok(Decimal::infinity(1));
            }
            t = n;
        } else if t.is_finite() {
            t = t.plus(cx, &n.times(cx, &n));
        }
    }

    cx.external = true;

    Ok(t.sqrt(cx))
}

/// `maxOrMin(Ctor, args, n)`: the maximum (`n` = -1) or minimum (`n` = 1) of the arguments.
fn max_or_min(cx: &mut Context, args: &[Value], n: f64) -> Result<Decimal, JsError> {
    let mut x = construct(cx, arg(args, 0))?;

    for v in args.iter().skip(1) {
        let y = construct(cx, v)?;

        // NaN?
        if y.s == 0 {
            x = y;
            break;
        }

        let k = x.cmp_value(cx, &y);

        if k == n || k == 0.0 && f64::from(x.s) == n {
            x = y;
        }
    }

    Ok(x)
}

/// `Decimal.sum(...args)`.
fn sum(cx: &mut Context, args: &[Value]) -> Result<Decimal, JsError> {
    let mut x = construct(cx, arg(args, 0))?;

    cx.external = false;
    let mut i = 0;
    while x.s != 0 {
        i += 1;
        let Some(v) = args.get(i) else { break };
        let y = construct(cx, v)?;
        x = x.plus(cx, Converted(y));
    }
    cx.external = true;

    let (pr, rm) = (cx.precision, cx.rounding);
    Ok(x.finalised(cx, Some(pr), rm, false))
}

/// The value of property `key` of a configuration object, `undefined` if it has none.
fn property<'a>(obj: &'a Value, key: &str) -> &'a Value {
    match obj {
        Value::Object(entries) => entries
            .iter()
            .find(|(k, _)| k == key)
            .map_or(&Value::Undefined, |(_, v)| v),
        _ => &Value::Undefined,
    }
}

/// `Decimal.set(obj)`: configure the constructor. The properties are checked and applied in the
/// order precision, rounding, toExpNeg, toExpPos, maxE, minE, modulo, crypto; the first invalid
/// one throws, leaving those before it applied.
pub fn set_config(cx: &mut Context, obj: &Value) -> Result<(), JsError> {
    if !obj.truthy() || obj.type_of() != "object" {
        return Err(JsError::decimal("Object expected"));
    }
    let use_defaults = matches!(property(obj, "defaults"), Value::Bool(true));
    let mut defaults = Context::default();

    type Field = fn(&mut Context) -> &mut i64;
    let ps: [(&str, i64, i64, Field); 7] = [
        ("precision", 1, MAX_DIGITS, |c| &mut c.precision),
        ("rounding", 0, 8, |c| &mut c.rounding),
        ("toExpNeg", -EXP_LIMIT, 0, |c| &mut c.to_exp_neg),
        ("toExpPos", 0, EXP_LIMIT, |c| &mut c.to_exp_pos),
        ("maxE", 0, EXP_LIMIT, |c| &mut c.max_e),
        ("minE", -EXP_LIMIT, 0, |c| &mut c.min_e),
        ("modulo", 0, 9, |c| &mut c.modulo),
    ];

    for (p, min, max, field) in ps {
        if use_defaults {
            *field(cx) = *field(&mut defaults);
        }
        let v = property(obj, p);
        if v.is_undefined() {
            continue;
        }
        // mathfloor(v) === v: only an integral number (or ±Infinity) equals its floor.
        let n = match v {
            Value::Number(n) if n.floor() == *n => *n,
            Value::BigInt(_) => return Err(JsError::bigint_to_number()),
            _ => return Err(invalid_property(cx, p, v)),
        };
        if n >= min as f64 && n <= max as f64 {
            *field(cx) = n as i64;
        } else {
            return Err(invalid_property(cx, p, v));
        }
    }

    if use_defaults {
        cx.crypto = defaults.crypto;
    }
    let v = property(obj, "crypto");
    match v {
        Value::Undefined => {}
        Value::Bool(b) => cx.crypto = *b,
        Value::Number(n) if *n == 0.0 || *n == 1.0 => cx.crypto = *n == 1.0,
        _ => return Err(invalid_property(cx, "crypto", v)),
    }

    Ok(())
}

fn invalid_property(cx: &Context, p: &str, v: &Value) -> JsError {
    JsError::invalid_argument(&format!("{p}: {}", v.concat_string(cx)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn obj(entries: &[(&str, Value)]) -> Value {
        Value::Object(
            entries
                .iter()
                .map(|(k, v)| (k.to_string(), v.clone()))
                .collect(),
        )
    }

    fn run(cx: &mut Context, name: &str, args: &[Value]) -> String {
        match call_static(cx, name, args) {
            Ok(v) => v.to_js_string(cx),
            Err(e) => e.to_string(),
        }
    }

    #[test]
    fn configuration_applies_in_order() {
        let mut cx = Context::default();
        let err = set_config(
            &mut cx,
            &obj(&[
                ("rounding", Value::Number(9.0)),
                ("precision", Value::Number(5.0)),
            ]),
        )
        .unwrap_err();
        assert_eq!(
            err.to_string(),
            "Error: [DecimalError] Invalid argument: rounding: 9"
        );
        assert_eq!(cx.precision, 5);
        assert_eq!(cx.rounding, 4);

        let err = set_config(&mut cx, &Value::Number(1.0)).unwrap_err();
        assert_eq!(err.to_string(), "Error: [DecimalError] Object expected");

        set_config(
            &mut cx,
            &obj(&[
                ("defaults", Value::Bool(true)),
                ("crypto", Value::Number(1.0)),
            ]),
        )
        .unwrap();
        assert_eq!(cx.precision, 20);
        assert!(cx.crypto);

        let err = set_config(&mut cx, &obj(&[("minE", Value::String("-5".into()))])).unwrap_err();
        assert_eq!(err.message, "[DecimalError] Invalid argument: minE: -5");
    }

    #[test]
    fn statics() {
        let mut cx = Context::default();
        let n = Value::Number;
        assert_eq!(run(&mut cx, "max", &[n(1.0), n(3.0), n(2.0)]), "3");
        assert_eq!(run(&mut cx, "min", &[n(1.0), n(f64::NAN), n(0.0)]), "NaN");
        assert_eq!(run(&mut cx, "sum", &[n(1.0), n(2.0), n(3.5)]), "6.5");
        assert_eq!(run(&mut cx, "hypot", &[n(3.0), n(4.0)]), "5");
        assert_eq!(run(&mut cx, "sign", &[Value::String("-0".into())]), "0");
        assert_eq!(run(&mut cx, "log2", &[n(8.0)]), "3");
        assert_eq!(run(&mut cx, "add", &[n(0.1), n(0.2)]), "0.3");
        assert_eq!(run(&mut cx, "round", &[Value::String("2.5".into())]), "3");
        assert_eq!(run(&mut cx, "isDecimal", &[n(1.0)]), "false");
        assert_eq!(
            run(&mut cx, "sum", &[]),
            "Error: [DecimalError] Invalid argument: undefined"
        );
    }
}
