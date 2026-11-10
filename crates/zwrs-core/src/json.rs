//! Minimal JSON reader/writer.
//!
//! Player profiles and sessions are stored as JSON because the project spec
//! requires a JSON format that humans can inspect and repair. Rather than
//! taking a serde dependency (which pulls in `syn`/`proc-macro2` and slows
//! cold builds on phones), ZWRS ships a small, strict, well-tested
//! implementation covering the subset it needs:
//!
//! * objects, arrays, strings (with escapes), numbers, booleans, null
//! * object keys preserved; lookups are by exact key
//! * output is pretty-printed with sorted insertion order preserved
//!
//! It is a parser/serializer for ZWRS-owned data, not a general-purpose
//! interchange library: duplicate keys are rejected, trailing content is
//! rejected, and numbers must fit in `f64`.

use std::collections::BTreeMap;
use std::fmt;

use crate::error::{Error, Result};

/// A JSON value.
#[derive(Debug, Clone, PartialEq)]
pub enum Json {
    /// `null`
    Null,
    /// `true` / `false`
    Bool(bool),
    /// A number (ZRS stores integers as `f64`; all stored values are exact).
    Number(f64),
    /// A string.
    String(String),
    /// An array.
    Array(Vec<Json>),
    /// An object with sorted keys.
    Object(BTreeMap<String, Json>),
}

impl Json {
    /// Convenience constructor for an object builder.
    #[must_use]
    pub fn object() -> BTreeMap<String, Json> {
        BTreeMap::new()
    }

    /// Parses a complete JSON document.
    ///
    /// # Errors
    /// Returns [`Error::Json`] with the byte offset of the first syntax error.
    pub fn parse(text: &str) -> Result<Self> {
        let mut p = Parser {
            bytes: text.as_bytes(),
            pos: 0,
        };
        p.skip_ws();
        let value = p.value()?;
        p.skip_ws();
        if p.pos != p.bytes.len() {
            return Err(p.err("trailing content after document"));
        }
        Ok(value)
    }

    /// Serialises to compact JSON.
    #[must_use]
    pub fn to_compact(&self) -> String {
        let mut out = String::new();
        self.write(&mut out, None, 0);
        out
    }

    /// Serialises to indented JSON (2 spaces) with a trailing newline.
    #[must_use]
    pub fn to_pretty(&self) -> String {
        let mut out = String::new();
        self.write(&mut out, Some(2), 0);
        out.push('\n');
        out
    }

    /// Object field lookup.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Self::Object(map) => map.get(key),
            _ => None,
        }
    }

    /// String view.
    #[must_use]
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::String(s) => Some(s),
            _ => None,
        }
    }

    /// Boolean view.
    #[must_use]
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Bool(b) => Some(*b),
            _ => None,
        }
    }

    /// Number view.
    #[must_use]
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Self::Number(n) => Some(*n),
            _ => None,
        }
    }

    /// Integer view; rejects values with a fractional part or out of `i64` range.
    #[must_use]
    pub fn as_i64(&self) -> Option<i64> {
        let n = self.as_f64()?;
        if !n.is_finite() || n.fract() != 0.0 {
            return None;
        }
        if n < -9.2e18 || n > 9.2e18 {
            return None;
        }
        #[allow(clippy::cast_possible_truncation)]
        Some(n as i64)
    }

    /// Unsigned integer view.
    #[must_use]
    pub fn as_u64(&self) -> Option<u64> {
        let n = self.as_i64()?;
        u64::try_from(n).ok()
    }

    /// Array view.
    #[must_use]
    pub fn as_array(&self) -> Option<&[Json]> {
        match self {
            Self::Array(v) => Some(v),
            _ => None,
        }
    }

    /// Object view.
    #[must_use]
    pub fn as_object(&self) -> Option<&BTreeMap<String, Json>> {
        match self {
            Self::Object(m) => Some(m),
            _ => None,
        }
    }

    fn write(&self, out: &mut String, indent: Option<usize>, depth: usize) {
        match self {
            Self::Null => out.push_str("null"),
            Self::Bool(true) => out.push_str("true"),
            Self::Bool(false) => out.push_str("false"),
            Self::Number(n) => out.push_str(&format_number(*n)),
            Self::String(s) => write_escaped(out, s),
            Self::Array(items) => {
                if items.is_empty() {
                    out.push_str("[]");
                    return;
                }
                out.push('[');
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    newline(out, indent, depth + 1);
                    item.write(out, indent, depth + 1);
                }
                newline(out, indent, depth);
                out.push(']');
            }
            Self::Object(map) => {
                if map.is_empty() {
                    out.push_str("{}");
                    return;
                }
                out.push('{');
                for (i, (k, v)) in map.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    newline(out, indent, depth + 1);
                    write_escaped(out, k);
                    out.push(':');
                    if indent.is_some() {
                        out.push(' ');
                    }
                    v.write(out, indent, depth + 1);
                }
                newline(out, indent, depth);
                out.push('}');
            }
        }
    }
}

fn newline(out: &mut String, indent: Option<usize>, depth: usize) {
    if let Some(width) = indent {
        out.push('\n');
        out.push_str(&" ".repeat(width * depth));
    }
}

fn format_number(n: f64) -> String {
    if n.fract() == 0.0 && n.abs() < 9.0e15 {
        #[allow(clippy::cast_possible_truncation)]
        let i = n as i64;
        return i.to_string();
    }
    let s = format!("{n}");
    // Guard against `inf`/`NaN` leaking into JSON; ZWRS never intends to store them.
    if s.contains("inf") || s.contains("NaN") {
        "null".to_string()
    } else {
        s
    }
}

fn write_escaped(out: &mut String, s: &str) {
    out.push('"');
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

struct Parser<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl Parser<'_> {
    fn err(&self, msg: &str) -> Error {
        Error::Json(format!("{msg} at byte {}", self.pos))
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    fn skip_ws(&mut self) {
        while let Some(b) = self.peek() {
            if matches!(b, b' ' | b'\t' | b'\n' | b'\r') {
                self.pos += 1;
            } else {
                break;
            }
        }
    }

    fn expect(&mut self, byte: u8) -> Result<()> {
        if self.peek() == Some(byte) {
            self.pos += 1;
            Ok(())
        } else {
            Err(self.err(&format!("expected '{}'", byte as char)))
        }
    }

    fn value(&mut self) -> Result<Json> {
        match self.peek() {
            None => Err(self.err("unexpected end of input")),
            Some(b'{') => self.object(),
            Some(b'[') => self.array(),
            Some(b'"') => self.string().map(Json::String),
            Some(b't') => self.literal("true", Json::Bool(true)),
            Some(b'f') => self.literal("false", Json::Bool(false)),
            Some(b'n') => self.literal("null", Json::Null),
            Some(b'-' | b'0'..=b'9') => self.number(),
            Some(other) => Err(self.err(&format!("unexpected byte 0x{other:02x}"))),
        }
    }

    fn literal(&mut self, word: &str, value: Json) -> Result<Json> {
        if self.bytes[self.pos..].starts_with(word.as_bytes()) {
            self.pos += word.len();
            Ok(value)
        } else {
            Err(self.err(&format!("expected {word}")))
        }
    }

    fn object(&mut self) -> Result<Json> {
        self.expect(b'{')?;
        let mut map = BTreeMap::new();
        self.skip_ws();
        if self.peek() == Some(b'}') {
            self.pos += 1;
            return Ok(Json::Object(map));
        }
        loop {
            self.skip_ws();
            let key = self.string()?;
            self.skip_ws();
            self.expect(b':')?;
            self.skip_ws();
            let value = self.value()?;
            if map.insert(key.clone(), value).is_some() {
                return Err(self.err(&format!("duplicate object key {key:?}")));
            }
            self.skip_ws();
            match self.peek() {
                Some(b',') => {
                    self.pos += 1;
                }
                Some(b'}') => {
                    self.pos += 1;
                    return Ok(Json::Object(map));
                }
                _ => return Err(self.err("expected ',' or '}' in object")),
            }
        }
    }

    fn array(&mut self) -> Result<Json> {
        self.expect(b'[')?;
        let mut items = Vec::new();
        self.skip_ws();
        if self.peek() == Some(b']') {
            self.pos += 1;
            return Ok(Json::Array(items));
        }
        loop {
            self.skip_ws();
            items.push(self.value()?);
            self.skip_ws();
            match self.peek() {
                Some(b',') => {
                    self.pos += 1;
                }
                Some(b']') => {
                    self.pos += 1;
                    return Ok(Json::Array(items));
                }
                _ => return Err(self.err("expected ',' or ']' in array")),
            }
        }
    }

    fn string(&mut self) -> Result<String> {
        self.expect(b'"')?;
        let mut out = String::new();
        loop {
            let Some(b) = self.peek() else {
                return Err(self.err("unterminated string"));
            };
            self.pos += 1;
            match b {
                b'"' => return Ok(out),
                b'\\' => {
                    let Some(esc) = self.peek() else {
                        return Err(self.err("unterminated escape"));
                    };
                    self.pos += 1;
                    match esc {
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'/' => out.push('/'),
                        b'b' => out.push('\u{08}'),
                        b'f' => out.push('\u{0c}'),
                        b'n' => out.push('\n'),
                        b'r' => out.push('\r'),
                        b't' => out.push('\t'),
                        b'u' => {
                            let hi = self.hex4()?;
                            let ch = if (0xD800..0xDC00).contains(&hi) {
                                // UTF-16 surrogate pair.
                                if self.bytes.get(self.pos) == Some(&b'\\')
                                    && self.bytes.get(self.pos + 1) == Some(&b'u')
                                {
                                    self.pos += 2;
                                    let lo = self.hex4()?;
                                    if !(0xDC00..0xE000).contains(&lo) {
                                        return Err(self.err("invalid low surrogate"));
                                    }
                                    let combined =
                                        0x1_0000 + ((hi - 0xD800) << 10) + (lo - 0xDC00);
                                    char::from_u32(combined)
                                        .ok_or_else(|| self.err("invalid surrogate pair"))?
                                } else {
                                    return Err(self.err("lone high surrogate"));
                                }
                            } else {
                                char::from_u32(hi)
                                    .ok_or_else(|| self.err("invalid unicode escape"))?
                            };
                            out.push(ch);
                        }
                        other => {
                            return Err(self.err(&format!(
                                "invalid escape '\\{}'",
                                other as char
                            )));
                        }
                    }
                }
                b if b < 0x20 => return Err(self.err("control character in string")),
                b if b < 0x80 => out.push(b as char),
                _ => {
                    // Multi-byte UTF-8: rewind and take the char from the slice.
                    self.pos -= 1;
                    let rest = std::str::from_utf8(&self.bytes[self.pos..])
                        .map_err(|_| self.err("invalid utf-8"))?;
                    let ch = rest.chars().next().ok_or_else(|| self.err("invalid utf-8"))?;
                    self.pos += ch.len_utf8();
                    out.push(ch);
                }
            }
        }
    }

    fn hex4(&mut self) -> Result<u32> {
        let slice = self
            .bytes
            .get(self.pos..self.pos + 4)
            .ok_or_else(|| self.err("truncated \\u escape"))?;
        let text = std::str::from_utf8(slice).map_err(|_| self.err("invalid \\u escape"))?;
        let value = u32::from_str_radix(text, 16).map_err(|_| self.err("invalid \\u escape"))?;
        self.pos += 4;
        Ok(value)
    }

    fn number(&mut self) -> Result<Json> {
        let start = self.pos;
        if self.peek() == Some(b'-') {
            self.pos += 1;
        }
        let mut digits = 0;
        if self.peek() == Some(b'0') {
            // JSON forbids leading zeros such as `01`.
            self.pos += 1;
            digits = 1;
            if matches!(self.peek(), Some(b'0'..=b'9')) {
                return Err(self.err("numbers must not have leading zeros"));
            }
        }
        while matches!(self.peek(), Some(b'0'..=b'9')) {
            self.pos += 1;
            digits += 1;
        }
        if digits == 0 {
            return Err(self.err("number requires at least one digit"));
        }
        if self.peek() == Some(b'.') {
            self.pos += 1;
            let mut frac = 0;
            while matches!(self.peek(), Some(b'0'..=b'9')) {
                self.pos += 1;
                frac += 1;
            }
            if frac == 0 {
                return Err(self.err("expected digits after decimal point"));
            }
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.pos += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.pos += 1;
            }
            let mut exp = 0;
            while matches!(self.peek(), Some(b'0'..=b'9')) {
                self.pos += 1;
                exp += 1;
            }
            if exp == 0 {
                return Err(self.err("expected exponent digits"));
            }
        }
        let text = std::str::from_utf8(&self.bytes[start..self.pos])
            .map_err(|_| self.err("invalid number"))?;
        text.parse::<f64>()
            .map(Json::Number)
            .map_err(|_| self.err("number out of range"))
    }
}

impl fmt::Display for Json {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_compact())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roundtrip(v: Json) {
        let text = v.to_pretty();
        let parsed = Json::parse(&text).expect("re-parse pretty output");
        assert_eq!(parsed, v);
        let compact = v.to_compact();
        assert_eq!(Json::parse(&compact).expect("re-parse compact"), v);
    }

    #[test]
    fn scalars_roundtrip() {
        for v in [
            Json::Null,
            Json::Bool(true),
            Json::Bool(false),
            Json::Number(0.0),
            Json::Number(-42.0),
            Json::Number(1.5),
            Json::String(String::new()),
            Json::String("hello".into()),
            Json::String("quote \" backslash \\ newline \n".into()),
            Json::String("unicode ✓ ✓".into()),
        ] {
            roundtrip(v);
        }
    }

    #[test]
    fn nested_roundtrip() {
        let text = r#"{"a":[1,2,{"b":null}],"c":{"d":true,"e":"x"}}"#;
        let v = Json::parse(text).expect("parse");
        roundtrip(v);
    }

    #[test]
    fn object_keys_are_sorted() {
        let v = Json::parse(r#"{"z":1,"a":2}"#).unwrap();
        assert_eq!(v.to_compact(), r#"{"a":2,"z":1}"#);
    }

    #[test]
    fn escapes_parse() {
        let v = Json::parse(r#""\u00e9\u2713\n\t\"\\""#).unwrap();
        assert_eq!(v.as_str(), Some("é✓\n\t\"\\"));
    }

    #[test]
    fn surrogate_pair_parses() {
        let v = Json::parse(r#""\ud83d\ude00""#).unwrap();
        assert_eq!(v.as_str(), Some("😀"));
    }

    #[test]
    fn rejects_malformed() {
        for bad in [
            "",
            "{",
            "}",
            "[1,]",
            "{\"a\":1,}",
            "{\"a\" 1}",
            "{a:1}",
            "01",
            "1.",
            "\"unterminated",
            "nul",
            "tru",
            "{} extra",
            "{\"a\":1,\"a\":2}",
        ] {
            assert!(Json::parse(bad).is_err(), "{bad:?} should be rejected");
        }
    }

    #[test]
    fn integer_views() {
        let v = Json::parse(r#"{"i":7,"f":7.5,"n":-3}"#).unwrap();
        assert_eq!(v.get("i").and_then(Json::as_i64), Some(7));
        assert_eq!(v.get("i").and_then(Json::as_u64), Some(7));
        assert_eq!(v.get("f").and_then(Json::as_i64), None);
        assert_eq!(v.get("n").and_then(Json::as_u64), None);
        assert_eq!(v.get("missing").and_then(Json::as_i64), None);
    }

    #[test]
    fn control_characters_rejected() {
        assert!(Json::parse("\"a\u{1}b\"").is_err());
    }

    #[test]
    fn floats_render_without_scientific_noise() {
        assert_eq!(Json::Number(3.0).to_compact(), "3");
        assert_eq!(Json::Number(3.25).to_compact(), "3.25");
    }
}
