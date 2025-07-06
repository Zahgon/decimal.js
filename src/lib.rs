//! decimal.js in Rust: an arbitrary-precision decimal type with the exact results, formatting and
//! errors of decimal.js v10.6.0, and the `decimal` command that serves it over JSON lines.
//!
//! The library has three layers:
//!
//! - [`js`]: the pieces of a JavaScript engine decimal.js relies on (number to string conversion,
//!   `Math.pow` and `Math.log` with V8's results, `JSON.parse`, values, errors);
//! - [`decimal`]: the port of decimal.js itself, a [`decimal::Decimal`] and the shared
//!   [`decimal::Context`];
//! - [`cli`]: the request protocol of the `decimal` command.

pub mod cli;
pub mod decimal;
pub mod js;
