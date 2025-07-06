# decimal

An arbitrary-precision Decimal type in Rust: a port of [decimal.js](https://github.com/MikeMcl/decimal.js)
v10.6.0 that gives the same results, digit for digit, as the JavaScript library, together with the
`decimal` command, which serves the library over JSON lines.

## Features

- Integers and fractions of any size, with a precision of up to 1e9 significant digits
- The full decimal.js API: arithmetic, comparisons, the nine rounding modes, `exp`, `ln`, `log`,
  `pow`, `sqrt`, `cbrt`, trigonometric, inverse trigonometric, hyperbolic and inverse hyperbolic
  functions, `toFraction`, `toNearest` and the ten modulo modes
- Decimal, hexadecimal, binary and octal input and output, binary exponents (`0x1.8p+3`) and
  numeric separators
- decimal.js's configuration (`precision`, `rounding`, `toExpNeg`, `toExpPos`, `minE`, `maxE`,
  `modulo`, `crypto`) and its error messages
- No dependencies outside the Rust standard library

## Build and test

```sh
cargo build --release
cargo test
```

The binary is `target/release/decimal`.

## The `decimal` command

`decimal` reads one JSON request per line from standard input and prints one result line per
request. All requests of one run share one configuration.

```sh
$ printf '%s\n' '{"set":{"precision":10}}' '{"x":"1","op":"div","args":[3]}' \
    '{"static":"max","args":[1,"5",{"$decimal":"-7"}]}' | decimal
ok
0.3333333333
5
```

A request is a JSON object with exactly one of these keys:

| request | effect |
| :-- | :-- |
| `{"set": CONFIG}` | `Decimal.set(CONFIG)`; prints `ok` |
| `{"show": "config"}` | prints the configuration as JSON |
| `{"x": VALUE, "op": NAME, "args": [ARG, ...]}` | `new Decimal(VALUE)`, then the method `NAME` |
| `{"x": VALUE, "ops": [[NAME, ARG, ...], ...]}` | `new Decimal(VALUE)`, then each method on the previous result |
| `{"static": NAME, "args": [ARG, ...], "ops": [...]}` | the static function `Decimal.NAME`, then the optional ops |

Arguments are JSON values, except objects with exactly one of the keys `$number` (`Number(TEXT)`),
`$bigint` (a BigInt), `$decimal` (a Decimal, made when the request runs) and `$undefined`.
Results are printed as decimal.js would show them: Decimals with `toString()`, numbers as
JavaScript prints them (negative zero as `-0`), arrays joined with commas. An exception prints
`NAME: MESSAGE`, for example `Error: [DecimalError] Invalid argument: precision: 0`, and a request
that breaks these rules prints `ShimError: MESSAGE`.

## The library

```rust
use decimal::cli::Session;

let mut session = Session::new();
assert_eq!(session.line(r#"{"x":"0.1","op":"plus","args":["0.2"]}"#), "0.3");
```

`decimal::decimal` holds the port itself: `Decimal`, the shared `Context` (the configuration and
the state decimal.js keeps between operations), `new_decimal`, `call_method`, `call_static` and
`set_config`. `decimal::js` holds the parts of JavaScript that decimal.js relies on: number to
string conversion, `Math.pow` and `Math.log` with the results of V8, `JSON.parse`, values and
errors.

## Licence

The MIT Licence. See [LICENCE.md](LICENCE.md).
