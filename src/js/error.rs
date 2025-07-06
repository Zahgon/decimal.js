//! Thrown JavaScript errors: a constructor name (`Error`, `TypeError`) and a message.

use std::fmt;

/// The prefix decimal.js puts in front of all its error messages.
pub const DECIMAL_ERROR: &str = "[DecimalError] ";

/// An exception as JavaScript code would throw it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JsError {
    pub name: &'static str,
    pub message: String,
}

impl JsError {
    /// `new Error(message)`.
    pub fn error(message: impl Into<String>) -> JsError {
        JsError {
            name: "Error",
            message: message.into(),
        }
    }

    /// `new TypeError(message)`.
    pub fn type_error(message: impl Into<String>) -> JsError {
        JsError {
            name: "TypeError",
            message: message.into(),
        }
    }

    /// decimal.js: `Error(invalidArgument + text)`.
    pub fn invalid_argument(text: &str) -> JsError {
        JsError::error(format!("{DECIMAL_ERROR}Invalid argument: {text}"))
    }

    /// decimal.js: `Error(precisionLimitExceeded)`.
    pub fn precision_limit_exceeded() -> JsError {
        JsError::error(format!("{DECIMAL_ERROR}Precision limit exceeded"))
    }

    /// decimal.js: `Error(decimalError + text)`.
    pub fn decimal(text: &str) -> JsError {
        JsError::error(format!("{DECIMAL_ERROR}{text}"))
    }

    /// The TypeError of reading the property `name` of `null`.
    pub fn null_property(name: &str) -> JsError {
        JsError::type_error(format!("Cannot read properties of null (reading '{name}')"))
    }

    /// The TypeError of converting a BigInt to a number (`Math.floor(1n)`, `+1n`).
    pub fn bigint_to_number() -> JsError {
        JsError::type_error("Cannot convert a BigInt value to a number")
    }

    /// The TypeError of arithmetic that mixes a BigInt with a number (`1n + 1`).
    pub fn mixed_bigint() -> JsError {
        JsError::type_error("Cannot mix BigInt and other types, use explicit conversions")
    }
}

impl fmt::Display for JsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.name, self.message)
    }
}

impl std::error::Error for JsError {}
