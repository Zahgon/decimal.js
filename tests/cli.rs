//! End-to-end tests of the `decimal` command: the process interface and request streams, with
//! expected values from the decimal.js test suite.

use std::io::Write;
use std::process::{Command, Output, Stdio};

fn decimal(args: &[&str], input: &[u8]) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_decimal"))
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn decimal");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(input)
        .expect("write requests");
    child.wait_with_output().expect("wait for decimal")
}

/// The output lines of a request stream, which must succeed without writing to standard error.
fn lines(input: &str) -> Vec<String> {
    let out = decimal(&[], input.as_bytes());
    assert_eq!(out.status.code(), Some(0));
    assert!(out.stderr.is_empty());
    String::from_utf8(out.stdout)
        .expect("UTF-8 output")
        .lines()
        .map(str::to_string)
        .collect()
}

/// The result of one method call on `new Decimal(x)`.
fn op(x: &str, name: &str, args: &str) -> String {
    let request = format!("{{\"x\":{x},\"op\":\"{name}\",\"args\":[{args}]}}\n");
    let mut out = lines(&request);
    assert_eq!(out.len(), 1, "{request}");
    out.remove(0)
}

#[test]
fn help_and_usage_errors() {
    let out = decimal(&["--help"], b"");
    assert_eq!(out.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&out.stdout).starts_with("Usage: decimal < REQUESTS\n"));

    let out = decimal(&["--precision", "5"], b"{\"x\":1}\n");
    assert_eq!(out.status.code(), Some(2));
    assert!(out.stdout.is_empty());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.starts_with("decimal: unexpected argument: --precision\nUsage: decimal"));
}

#[test]
fn empty_input_prints_nothing() {
    assert!(lines("").is_empty());
    assert!(lines("\n \t\n\r\n").is_empty());
}

#[test]
fn line_handling() {
    let out = decimal(
        &[],
        b"{\"x\":\"1.50\"}\r\n\xef\xbb\xbf{\"x\":2}\n{\"x\":\"\xff\"}\n{\"x\":3}",
    );
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "1.5\nShimError: invalid JSON\nError: [DecimalError] Invalid argument: \u{fffd}\n3\n"
    );
}

#[test]
fn configuration_persists_across_requests() {
    let out = lines(concat!(
        "{\"set\":{\"precision\":10,\"rounding\":1}}\n",
        "{\"x\":2,\"op\":\"sqrt\"}\n",
        "{\"set\":{\"precision\":0}}\n",
        "{\"x\":2,\"op\":\"sqrt\"}\n",
        "{\"set\":{\"defaults\":true}}\n",
        "{\"x\":2,\"op\":\"sqrt\"}\n",
    ));
    assert_eq!(
        out,
        [
            "ok",
            "1.414213562",
            "Error: [DecimalError] Invalid argument: precision: 0",
            "1.414213562",
            "ok",
            "1.4142135623730950488",
        ]
    );
}

#[test]
fn chained_operations_and_statics() {
    let out = lines(concat!(
        "{\"x\":\"0.1\",\"ops\":[[\"plus\",\"0.2\"],[\"times\",3],[\"toFixed\",2]]}\n",
        "{\"static\":\"max\",\"args\":[1,\"5\",{\"$decimal\":\"-7\"}],\"ops\":[[\"neg\"]]}\n",
        "{\"static\":\"sign\",\"args\":[{\"$number\":\"-0\"}]}\n",
        "{\"x\":\"7\",\"ops\":[[\"cmp\",8],[\"abs\"]]}\n",
        "{\"x\":{\"$bigint\":\"-123456789012345678901234567890\"},\"op\":\"toExponential\",\"args\":[3]}\n",
    ));
    assert_eq!(
        out,
        [
            "0.90",
            "-5",
            "-0",
            "ShimError: not a Decimal: -1",
            "-1.235e+29",
        ]
    );
}

#[test]
fn fractions() {
    assert_eq!(op("\"0.1\"", "toFraction", ""), "1,10");
    assert_eq!(op("\"-0.625\"", "toFraction", ""), "-5,8");
    assert_eq!(op("\"5.1582612935891\"", "toFraction", "\"3\""), "5,1");
    assert_eq!(
        op("\"8.14969395596340\"", "toFraction", "\"4682\""),
        "14645,1797"
    );
    assert_eq!(
        op("\"4.28004634702\"", "toFraction", "\"82418\""),
        "350921,81990"
    );
    assert_eq!(
        op("\"1\"", "toFraction", "\"0.5\""),
        "Error: [DecimalError] Invalid argument: 0.5"
    );
}

#[test]
fn bases() {
    assert_eq!(op("\"255.5\"", "toHex", ""), "0xff.8");
    assert_eq!(op("\"-0\"", "toBinary", ""), "-0b0");
    assert_eq!(op("\"0.1\"", "toBinary", "5"), "0b1.101p-4");
    assert_eq!(op("\"0x1.8p+1\"", "toString", ""), "3");
    assert_eq!(op("\"0o17.4\"", "toString", ""), "15.5");
    assert_eq!(op("\"0b1_0_1\"", "toString", ""), "5");
}

#[test]
fn based_literals_outside_the_exponent_limits() {
    let out = lines(concat!(
        "{\"set\":{\"precision\":5,\"maxE\":1}}\n",
        "{\"x\":\"1.00\",\"op\":\"div\",\"args\":[\"0B10100101110100.111111010\"]}\n",
        "{\"x\":\"1\",\"op\":\"div\",\"args\":[{\"$decimal\":\"0b10100101110100.1\"}]}\n",
        "{\"x\":\"0x1ff\"}\n",
        "{\"x\":\"0x1ff\",\"op\":\"pow\",\"args\":[\"0.5\"]}\n",
        "{\"static\":\"sum\",\"args\":[\"1\",\"0x1ff\",\"-0x1f0\"]}\n",
        "{\"set\":{\"maxE\":0}}\n",
        "{\"x\":\"0x.8\"}\n",
        "{\"x\":\"1e5\",\"op\":\"times\",\"args\":[\"1\"]}\n",
        "{\"x\":\"2\",\"op\":\"sqrt\"}\n",
        "{\"x\":\"1e5\",\"op\":\"times\",\"args\":[\"1\"]}\n",
    ));
    let type_error = "TypeError: Cannot read properties of null (reading 'length')";
    assert_eq!(
        out,
        [
            "ok",
            "0.000094224",
            "0",
            "511",
            type_error,
            "Infinity",
            "ok",
            type_error,
            "100000",
            "1.4142",
            "Infinity"
        ]
    );
}

#[test]
fn transcendental_functions() {
    assert_eq!(op("1", "exp", ""), "2.7182818284590452354");
    assert_eq!(op("\"1000\"", "log", ""), "3");
    assert_eq!(op("\"8\"", "log", "2"), "3");
    assert_eq!(op("\"-8\"", "cbrt", ""), "-2");
    assert_eq!(op("2", "pow", "\"0.5\""), "1.4142135623730950488");
    assert_eq!(op("1", "atan", ""), "0.78539816339744830962");
    assert_eq!(op("\"0.5\"", "atanh", ""), "0.5493061443340548457");
}

#[test]
fn errors_name_their_argument() {
    assert_eq!(
        op("1", "toFixed", "2.5"),
        "Error: [DecimalError] Invalid argument: 2.5"
    );
    assert_eq!(
        op("1", "toDP", "{\"$bigint\":\"2\"}"),
        "TypeError: Cannot mix BigInt and other types, use explicit conversions"
    );
    assert_eq!(
        op("1", "plus", "[1,[2,null]]"),
        "Error: [DecimalError] Invalid argument: 1,2,"
    );
    assert_eq!(
        lines("{\"set\":{\"precision\":{\"$bigint\":\"5\"}}}\n"),
        ["TypeError: Cannot convert a BigInt value to a number"]
    );
}
