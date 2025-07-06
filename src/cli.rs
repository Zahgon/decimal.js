//! The `decimal` command: JSON-lines requests on standard input, one result line per request.
//!
//! A request is a JSON object with exactly one of these keys:
//!
//! - `{"set": CONFIG}`: `Decimal.set(CONFIG)`; prints `ok`.
//! - `{"show": "config"}`: prints the configuration as JSON.
//! - `{"x": VALUE, "op": NAME, "args": [ARG, ...]}` or `{"x": VALUE, "ops": [[NAME, ARG, ...], ...]}`:
//!   `new Decimal(VALUE)`, then the instance method `NAME` (or each method of `ops` on the previous
//!   result).
//! - `{"static": NAME, "args": [ARG, ...], "ops": [...]}`: the static function `Decimal.NAME`, then
//!   the optional ops.
//!
//! JSON values become JavaScript values as `JSON.parse` makes them, except objects with exactly one
//! of the keys `$number` (`Number(TEXT)`), `$bigint` (`BigInt(DIGITS)`), `$decimal`
//! (`new Decimal(VALUE)`, made when the request runs) and `$undefined` (`undefined`).
//!
//! All requests of one run share one Decimal configuration. A request that throws prints
//! `NAME: MESSAGE`; a request that breaks the rules prints `ShimError: MESSAGE`. Either way the run
//! continues with the next line.

use std::io::{self, Read, Write};

use crate::decimal::{self, call_method, call_static, is_method, is_static, set_config, Context};
use crate::js::json::{self, Json, JsonObject};
use crate::js::{number, BigInt, JsError, Value};

/// The text of `--help`, also written to standard error after a usage error.
pub const USAGE: &str = "Usage: decimal < REQUESTS\n\
Reads one JSON request per line from standard input and prints one result line per request.\n";

/// Why a request produced no value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Failure {
    /// The request breaks the rules of the command.
    Shim(String),
    /// decimal.js (or the JavaScript engine under it) threw.
    Thrown(JsError),
}

impl From<JsError> for Failure {
    fn from(e: JsError) -> Failure {
        Failure::Thrown(e)
    }
}

impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Failure::Shim(message) => write!(f, "ShimError: {message}"),
            Failure::Thrown(e) => write!(f, "{e}"),
        }
    }
}

fn shim(message: impl Into<String>) -> Failure {
    Failure::Shim(message.into())
}

/// The request kinds, in the order they are looked for.
const KINDS: [&str; 4] = ["set", "show", "x", "static"];

/// The keys a request of each kind may have.
fn allowed_keys(kind: &str) -> &'static [&'static str] {
    match kind {
        "set" => &["set"],
        "show" => &["show"],
        "x" => &["x", "op", "args", "ops"],
        _ => &["static", "args", "ops"],
    }
}

/// One run of the command: the shared Decimal configuration.
#[derive(Debug, Default)]
pub struct Session {
    cx: Context,
}

impl Session {
    /// A session with the default configuration.
    pub fn new() -> Session {
        Session::default()
    }

    /// The configuration and module state.
    pub fn context(&self) -> &Context {
        &self.cx
    }

    /// The output line of one input line (which is not blank).
    pub fn line(&mut self, line: &str) -> String {
        let Ok(request) = json::parse(line) else {
            return "ShimError: invalid JSON".to_string();
        };
        match self.handle(&request) {
            Ok(out) => out,
            Err(failure) => failure.to_string(),
        }
    }

    /// The result of one request.
    pub fn handle(&mut self, request: &Json) -> Result<String, Failure> {
        let Json::Object(req) = request else {
            return Err(shim("a request must be a JSON object"));
        };
        let kinds: Vec<&str> = KINDS.iter().copied().filter(|k| req.has(k)).collect();
        let [kind] = kinds[..] else {
            return Err(shim("a request needs exactly one of set, show, x, static"));
        };
        for key in req.keys() {
            if !allowed_keys(kind).contains(&key) {
                return Err(shim(format!("unexpected key for {kind}: {key}")));
            }
        }

        match kind {
            "set" => {
                let config = self.to_js(req.get("set").unwrap_or(&Json::Null))?;
                set_config(&mut self.cx, &config)?;
                return Ok("ok".to_string());
            }
            "show" => {
                if req.get("show") != Some(&Json::String("config".to_string())) {
                    return Err(shim("show needs \"config\""));
                }
                return Ok(self.cx.config_json());
            }
            _ => {}
        }

        let mut ops: Vec<&[Json]> = Vec::new();
        let mut value = if kind == "x" {
            if req.has("op") && req.has("ops") {
                return Err(shim("op and ops cannot be combined"));
            }
            if req.has("args") && !req.has("op") {
                return Err(shim("args needs op"));
            }
            let op = match req.get("op") {
                None => None,
                Some(Json::String(name)) => Some(name.as_str()),
                Some(_) => return Err(shim("op must be a string")),
            };
            if req.has("ops") {
                ops = op_list(req)?;
            }
            let args = arg_list(req)?;
            let x = self.to_js(req.get("x").unwrap_or(&Json::Null))?;
            let value = Value::Decimal(decimal::new_decimal(&mut self.cx, &x)?);
            match op {
                Some(name) => self.call(value, name, args)?,
                None => value,
            }
        } else {
            let name = match req.get("static") {
                Some(Json::String(name)) if is_static(name) => name.as_str(),
                other => {
                    let name = other.map_or_else(|| "undefined".to_string(), render_json);
                    return Err(shim(format!("unknown static function: {name}")));
                }
            };
            if req.has("ops") {
                ops = op_list(req)?;
            }
            let args = arg_list(req)?;
            let args = args
                .iter()
                .map(|a| self.to_js(a))
                .collect::<Result<Vec<_>, _>>()?;
            call_static(&mut self.cx, name, &args)?
        };

        for op in ops {
            let Json::String(name) = &op[0] else {
                unreachable!("op_list checks the method names");
            };
            value = self.call(value, name, &op[1..])?;
        }

        Ok(render(&self.cx, &value))
    }

    /// `call(value, name, args)`: the method `name` of the Decimal `value`, its arguments converted
    /// when it runs.
    fn call(&mut self, value: Value, name: &str, args: &[Json]) -> Result<Value, Failure> {
        if !is_method(name) {
            return Err(shim(format!("unknown method: {name}")));
        }
        let Value::Decimal(x) = value else {
            return Err(shim(format!("not a Decimal: {}", render(&self.cx, &value))));
        };
        let args = args
            .iter()
            .map(|a| self.to_js(a))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(call_method(&mut self.cx, &x, name, &args)?)
    }

    /// `toJS(v)`: the JavaScript value of a JSON value.
    pub fn to_js(&mut self, v: &Json) -> Result<Value, Failure> {
        Ok(match v {
            Json::Null => Value::Null,
            Json::Bool(b) => Value::Bool(*b),
            Json::Number(n) => Value::Number(*n),
            Json::String(s) => Value::String(s.clone()),
            Json::Array(items) => Value::Array(
                items
                    .iter()
                    .map(|item| self.to_js(item))
                    .collect::<Result<_, _>>()?,
            ),
            Json::Object(obj) => {
                if let Some(special) = self.special(obj)? {
                    return Ok(special);
                }
                let mut entries = Vec::with_capacity(obj.len());
                for (k, v) in obj.entries() {
                    entries.push((k.clone(), self.to_js(v)?));
                }
                Value::Object(entries)
            }
        })
    }

    /// The value of an object with exactly one of the keys `$number`, `$bigint`, `$decimal` and
    /// `$undefined`.
    fn special(&mut self, obj: &JsonObject) -> Result<Option<Value>, Failure> {
        let [(key, a)] = obj.entries() else {
            return Ok(None);
        };
        Ok(Some(match key.as_str() {
            "$number" => match a {
                Json::String(text) => Value::Number(number::string_to_number(text)),
                _ => return Err(shim("$number needs a string")),
            },
            "$bigint" => match a {
                Json::String(text) => match BigInt::parse(text) {
                    Some(b) => Value::BigInt(b),
                    None => return Err(shim("$bigint needs a decimal integer string")),
                },
                _ => return Err(shim("$bigint needs a decimal integer string")),
            },
            "$decimal" => {
                let v = self.to_js(a)?;
                Value::Decimal(decimal::new_decimal(&mut self.cx, &v)?)
            }
            "$undefined" => match a {
                Json::Bool(true) => Value::Undefined,
                _ => return Err(shim("$undefined needs true")),
            },
            _ => return Ok(None),
        }))
    }
}

/// `opList(req)`: each op is an array that starts with a method name.
fn op_list(req: &JsonObject) -> Result<Vec<&[Json]>, Failure> {
    let Some(Json::Array(ops)) = req.get("ops") else {
        return Err(shim("ops must be an array"));
    };
    ops.iter()
        .map(|op| match op {
            Json::Array(items) if matches!(items.first(), Some(Json::String(_))) => {
                Ok(items.as_slice())
            }
            _ => Err(shim(
                "each op must be an array that starts with a method name",
            )),
        })
        .collect()
}

/// `argList(req)`: the arguments, none if there is no `args`.
fn arg_list(req: &JsonObject) -> Result<&[Json], Failure> {
    match req.get("args") {
        None => Ok(&[]),
        Some(Json::Array(args)) => Ok(args),
        Some(_) => Err(shim("args must be an array")),
    }
}

/// `render(v)`: a Decimal as its `toString()`, a number as `String(n)` (negative zero as `-0`), an
/// array as its rendered elements joined with commas, anything else as `String(v)`.
pub fn render(cx: &Context, v: &Value) -> String {
    match v {
        Value::Decimal(d) => d.to_js_string(cx),
        Value::Number(n) => render_number(*n),
        Value::Array(items) => items
            .iter()
            .map(|item| render(cx, item))
            .collect::<Vec<_>>()
            .join(","),
        other => other.to_js_string(cx),
    }
}

fn render_number(n: f64) -> String {
    if n == 0.0 && n.is_sign_negative() {
        "-0".to_string()
    } else {
        number::to_string(n)
    }
}

/// `render` of a JSON value as `JSON.parse` makes it.
fn render_json(v: &Json) -> String {
    match v {
        Json::Null => "null".to_string(),
        Json::Bool(b) => b.to_string(),
        Json::Number(n) => render_number(*n),
        Json::String(s) => s.clone(),
        Json::Array(items) => items.iter().map(render_json).collect::<Vec<_>>().join(","),
        Json::Object(_) => "[object Object]".to_string(),
    }
}

/// Whether a line is skipped: empty or only spaces and tabs.
fn is_blank(line: &str) -> bool {
    line.bytes().all(|b| b == b' ' || b == b'\t')
}

/// Run the command on `input`, writing results to `out` and a usage error to `err`; returns the
/// exit status.
pub fn run(
    args: &[String],
    input: &mut impl Read,
    out: &mut impl Write,
    err: &mut impl Write,
) -> io::Result<i32> {
    if let Some(first) = args.first() {
        if args.len() == 1 && first == "--help" {
            out.write_all(USAGE.as_bytes())?;
            return Ok(0);
        }
        write!(err, "decimal: unexpected argument: {first}\n{USAGE}")?;
        return Ok(2);
    }

    let mut bytes = Vec::new();
    input.read_to_end(&mut bytes)?;
    let text = String::from_utf8_lossy(&bytes);

    let mut session = Session::new();
    for line in text.split('\n') {
        let line = line.strip_suffix('\r').unwrap_or(line);
        if is_blank(line) {
            continue;
        }
        let result = session.line(line);
        out.write_all(result.as_bytes())?;
        out.write_all(b"\n")?;
    }
    out.flush()?;
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run_text(input: &str) -> String {
        let mut out = Vec::new();
        let mut err = Vec::new();
        let status = run(&[], &mut input.as_bytes(), &mut out, &mut err).unwrap();
        assert_eq!(status, 0);
        assert!(err.is_empty());
        String::from_utf8(out).unwrap()
    }

    #[test]
    fn requests_share_a_configuration() {
        let out = run_text(
            "{\"set\":{\"precision\":5}}\n\n  \t\n{\"x\":\"1\",\"op\":\"div\",\"args\":[3]}\r\n{\"show\":\"config\"}\n",
        );
        assert_eq!(
            out,
            "ok\n0.33333\n{\"precision\":5,\"rounding\":4,\"toExpNeg\":-7,\"toExpPos\":21,\"minE\":-9000000000000000,\"maxE\":9000000000000000,\"modulo\":1,\"crypto\":false}\n"
        );
    }

    #[test]
    fn shim_errors() {
        let out = run_text(
            "[1]\n{}\n{\"x\":1,\"y\":2}\n{\"x\":1,\"args\":[]}\n{\"x\":1,\"op\":\"nope\"}\n{\"static\":-0}\nnot json\n{\"x\":1,\"ops\":[[\"cmp\",2],[\"abs\"]]}\n",
        );
        assert_eq!(
            out,
            "ShimError: a request must be a JSON object\n\
             ShimError: a request needs exactly one of set, show, x, static\n\
             ShimError: unexpected key for x: y\n\
             ShimError: args needs op\n\
             ShimError: unknown method: nope\n\
             ShimError: unknown static function: -0\n\
             ShimError: invalid JSON\n\
             ShimError: not a Decimal: -1\n"
        );
    }

    #[test]
    fn special_values() {
        let out = run_text(
            "{\"x\":{\"$number\":\"-0\"}}\n{\"x\":{\"$bigint\":\"-12\"},\"op\":\"plus\",\"args\":[{\"$decimal\":\"0.5\"}]}\n{\"x\":\"2\",\"op\":\"toFraction\"}\n{\"x\":\"0.75\",\"op\":\"toFraction\"}\n{\"x\":true}\n{\"static\":\"sign\",\"args\":[{\"$number\":\"-0\"}]}\n",
        );
        assert_eq!(
            out,
            "0\n-11.5\n2,1\n3,4\nError: [DecimalError] Invalid argument: true\n-0\n"
        );
    }

    #[test]
    fn usage() {
        let mut out = Vec::new();
        let mut err = Vec::new();
        let args = ["-x".to_string()];
        let status = run(&args, &mut "".as_bytes(), &mut out, &mut err).unwrap();
        assert_eq!(status, 2);
        assert!(out.is_empty());
        assert_eq!(
            String::from_utf8(err).unwrap(),
            format!("decimal: unexpected argument: -x\n{USAGE}")
        );
    }
}
