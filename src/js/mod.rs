//! The parts of a JavaScript engine that decimal.js depends on: number conversions and `Math`
//! functions with V8's exact results, `JSON.parse`, values and their conversions, and errors.

pub mod error;
pub mod fdlibm;
pub mod json;
pub mod number;
pub mod value;

pub use error::JsError;
pub use value::{BigInt, Value};
