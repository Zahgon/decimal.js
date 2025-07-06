//! `JSON.parse`: the strict JSON grammar of ECMA-404, with JavaScript's object semantics (a repeated
//! key keeps its first position and takes its last value; `Object.keys` lists array-index keys
//! first, in ascending numeric order, then the other keys in insertion order).

/// A parsed JSON value.
#[derive(Clone, Debug, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    Number(f64),
    String(String),
    Array(Vec<Json>),
    Object(JsonObject),
}

/// A JSON object, its entries in `Object.keys` order.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct JsonObject {
    entries: Vec<(String, Json)>,
}

impl JsonObject {
    /// The entries in `Object.keys` order.
    pub fn entries(&self) -> &[(String, Json)] {
        &self.entries
    }

    /// The keys in `Object.keys` order.
    pub fn keys(&self) -> impl Iterator<Item = &str> {
        self.entries.iter().map(|(k, _)| k.as_str())
    }

    /// The value of an own property.
    pub fn get(&self, key: &str) -> Option<&Json> {
        self.entries.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }

    /// `Object.prototype.hasOwnProperty`.
    pub fn has(&self, key: &str) -> bool {
        self.get(key).is_some()
    }

    /// The number of own properties.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the object has no own properties.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    fn insert(&mut self, key: String, value: Json) {
        if let Some(slot) = self.entries.iter_mut().find(|(k, _)| *k == key) {
            slot.1 = value;
            return;
        }
        match array_index(&key) {
            Some(index) => {
                let at = self
                    .entries
                    .iter()
                    .position(|(k, _)| array_index(k).is_none_or(|other| other > index))
                    .unwrap_or(self.entries.len());
                self.entries.insert(at, (key, value));
            }
            None => self.entries.push((key, value)),
        }
    }
}

/// The value of a key that is an array index (a canonical decimal integer below 2^32 - 1).
fn array_index(key: &str) -> Option<u32> {
    if key.is_empty() || (key.len() > 1 && key.starts_with('0')) {
        return None;
    }
    if !key.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    key.parse::<u32>().ok().filter(|&n| n != u32::MAX)
}

/// Why a text is not JSON.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SyntaxError;

/// `JSON.parse(text)`.
pub fn parse(text: &str) -> Result<Json, SyntaxError> {
    let mut parser = Parser {
        chars: text.chars().collect(),
        pos: 0,
    };
    parser.skip_space();
    let value = parser.value(0)?;
    parser.skip_space();
    if parser.pos != parser.chars.len() {
        return Err(SyntaxError);
    }
    Ok(value)
}

/// Nesting deeper than this is refused rather than risking the native stack.
const MAX_DEPTH: usize = 4000;

struct Parser {
    chars: Vec<char>,
    pos: usize,
}

impl Parser {
    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn bump(&mut self) -> Option<char> {
        let c = self.peek()?;
        self.pos += 1;
        Some(c)
    }

    fn expect_char(&mut self, c: char) -> Result<(), SyntaxError> {
        if self.bump() == Some(c) {
            Ok(())
        } else {
            Err(SyntaxError)
        }
    }

    fn skip_space(&mut self) {
        while matches!(self.peek(), Some(' ' | '\t' | '\n' | '\r')) {
            self.pos += 1;
        }
    }

    fn value(&mut self, depth: usize) -> Result<Json, SyntaxError> {
        if depth > MAX_DEPTH {
            return Err(SyntaxError);
        }
        match self.peek().ok_or(SyntaxError)? {
            '{' => self.object(depth),
            '[' => self.array(depth),
            '"' => Ok(Json::String(self.string()?)),
            't' => self.literal("true", Json::Bool(true)),
            'f' => self.literal("false", Json::Bool(false)),
            'n' => self.literal("null", Json::Null),
            '-' | '0'..='9' => self.number(),
            _ => Err(SyntaxError),
        }
    }

    fn literal(&mut self, word: &str, value: Json) -> Result<Json, SyntaxError> {
        for c in word.chars() {
            self.expect_char(c)?;
        }
        Ok(value)
    }

    fn object(&mut self, depth: usize) -> Result<Json, SyntaxError> {
        self.expect_char('{')?;
        let mut object = JsonObject::default();
        self.skip_space();
        if self.peek() == Some('}') {
            self.pos += 1;
            return Ok(Json::Object(object));
        }
        loop {
            self.skip_space();
            if self.peek() != Some('"') {
                return Err(SyntaxError);
            }
            let key = self.string()?;
            self.skip_space();
            self.expect_char(':')?;
            self.skip_space();
            let value = self.value(depth + 1)?;
            object.insert(key, value);
            self.skip_space();
            match self.bump() {
                Some(',') => continue,
                Some('}') => return Ok(Json::Object(object)),
                _ => return Err(SyntaxError),
            }
        }
    }

    fn array(&mut self, depth: usize) -> Result<Json, SyntaxError> {
        self.expect_char('[')?;
        let mut items = Vec::new();
        self.skip_space();
        if self.peek() == Some(']') {
            self.pos += 1;
            return Ok(Json::Array(items));
        }
        loop {
            self.skip_space();
            items.push(self.value(depth + 1)?);
            self.skip_space();
            match self.bump() {
                Some(',') => continue,
                Some(']') => return Ok(Json::Array(items)),
                _ => return Err(SyntaxError),
            }
        }
    }

    fn number(&mut self) -> Result<Json, SyntaxError> {
        let start = self.pos;
        if self.peek() == Some('-') {
            self.pos += 1;
        }
        match self.bump() {
            Some('0') => {}
            Some('1'..='9') => self.digits(),
            _ => return Err(SyntaxError),
        }
        if self.peek() == Some('.') {
            self.pos += 1;
            if !matches!(self.peek(), Some('0'..='9')) {
                return Err(SyntaxError);
            }
            self.digits();
        }
        if matches!(self.peek(), Some('e' | 'E')) {
            self.pos += 1;
            if matches!(self.peek(), Some('+' | '-')) {
                self.pos += 1;
            }
            if !matches!(self.peek(), Some('0'..='9')) {
                return Err(SyntaxError);
            }
            self.digits();
        }
        let text: String = self.chars[start..self.pos].iter().collect();
        text.parse::<f64>()
            .map(Json::Number)
            .map_err(|_| SyntaxError)
    }

    fn digits(&mut self) {
        while matches!(self.peek(), Some('0'..='9')) {
            self.pos += 1;
        }
    }

    fn hex4(&mut self) -> Result<u32, SyntaxError> {
        let mut n = 0;
        for _ in 0..4 {
            let d = self
                .bump()
                .and_then(|c| c.to_digit(16))
                .ok_or(SyntaxError)?;
            n = n * 16 + d;
        }
        Ok(n)
    }

    /// A string literal. JavaScript strings are UTF-16; an escaped lone surrogate, which no Rust
    /// string can hold, becomes U+FFFD, the character that printing such a string produces.
    fn string(&mut self) -> Result<String, SyntaxError> {
        self.expect_char('"')?;
        let mut out = String::new();
        loop {
            let c = self.bump().ok_or(SyntaxError)?;
            match c {
                '"' => return Ok(out),
                '\\' => {
                    let escaped = self.bump().ok_or(SyntaxError)?;
                    match escaped {
                        '"' => out.push('"'),
                        '\\' => out.push('\\'),
                        '/' => out.push('/'),
                        'b' => out.push('\u{8}'),
                        'f' => out.push('\u{c}'),
                        'n' => out.push('\n'),
                        'r' => out.push('\r'),
                        't' => out.push('\t'),
                        'u' => {
                            let unit = self.hex4()?;
                            out.push(self.code_unit(unit)?);
                        }
                        _ => return Err(SyntaxError),
                    }
                }
                c if (c as u32) < 0x20 => return Err(SyntaxError),
                c => out.push(c),
            }
        }
    }

    /// The character of a `\uXXXX` escape, combined with a following `\uXXXX` low surrogate when
    /// it is a high surrogate.
    fn code_unit(&mut self, unit: u32) -> Result<char, SyntaxError> {
        if (0xD800..0xDC00).contains(&unit) {
            let save = self.pos;
            if self.peek() == Some('\\') && self.chars.get(self.pos + 1) == Some(&'u') {
                self.pos += 2;
                let low = self.hex4()?;
                if (0xDC00..0xE000).contains(&low) {
                    let code = 0x10000 + ((unit - 0xD800) << 10) + (low - 0xDC00);
                    return Ok(char::from_u32(code).unwrap_or('\u{FFFD}'));
                }
                self.pos = save;
            }
            return Ok('\u{FFFD}');
        }
        if (0xDC00..0xE000).contains(&unit) {
            return Ok('\u{FFFD}');
        }
        Ok(char::from_u32(unit).unwrap_or('\u{FFFD}'))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn obj(text: &str) -> JsonObject {
        match parse(text).unwrap() {
            Json::Object(o) => o,
            other => panic!("not an object: {other:?}"),
        }
    }

    #[test]
    fn rejects_what_json_parse_rejects() {
        for bad in [
            "{\"x\":\"1\",}",
            "{'x':'1'}",
            "{\"x\":NaN}",
            "{\"x\":01}",
            "{\"x\":1.}",
            "{\"x\":.5}",
            "{\"x\":+1}",
            "{x:\"1\"}",
            "{\"x\":\"1\"",
            "\u{feff}{}",
            "{\"x\":\"\t\"}",
            "{\"x\":\"\\x41\"}",
            "{\"x\":\"\\u12\"}",
            "[1,]",
            "{\"x\":1} {}",
            "",
        ] {
            assert!(parse(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn accepts_json() {
        assert_eq!(parse(" 5 ").unwrap(), Json::Number(5.0));
        assert_eq!(parse("-0").unwrap(), Json::Number(-0.0));
        assert_eq!(parse("1E+2").unwrap(), Json::Number(100.0));
        assert_eq!(parse("1e400").unwrap(), Json::Number(f64::INFINITY));
        assert_eq!(
            parse("\"\\ud83d\\ude00\\u0041\"").unwrap(),
            Json::String("😀A".into())
        );
        assert_eq!(
            parse("\"\\ud83d\"").unwrap(),
            Json::String("\u{FFFD}".into())
        );
        assert_eq!(
            parse("\"\\uDE00\\uD83D\"").unwrap(),
            Json::String("\u{FFFD}\u{FFFD}".into())
        );
    }

    #[test]
    fn object_key_order_and_duplicates() {
        let o = obj("{\"b\":1,\"2\":2,\"a\":3,\"1\":4,\"b\":5,\"01\":6,\"4294967295\":7}");
        let keys: Vec<&str> = o.keys().collect();
        assert_eq!(keys, ["1", "2", "b", "a", "01", "4294967295"]);
        assert_eq!(o.get("b"), Some(&Json::Number(5.0)));
    }
}
