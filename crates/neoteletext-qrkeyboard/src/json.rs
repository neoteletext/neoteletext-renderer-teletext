//! JSON values for the relay's messages, read with the engine's bounded
//! reader (`neoteletext_web::json`), and written canonically as
//! qrkeyboard's `canonicalJson` writes them: object keys sorted at every
//! level, strings escaped as `JSON.stringify` escapes them.

use std::fmt::Write;

use neoteletext_web::json::{JsonReader, Token};

/// The most members and elements read from one document: far beyond any
/// message or configuration qrkeyboard sends, which the relay bounds to a
/// few kilobytes.
const MAX_ITEMS: usize = 4096;

/// A JSON value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Value {
    /// `null`
    Null,
    /// `true` or `false`
    Bool(bool),
    /// A number, as its validated text.
    Number(String),
    /// A string, decoded.
    String(String),
    /// An array.
    Array(Vec<Value>),
    /// An object, members in the order read.
    Object(Vec<(String, Value)>),
}

impl Value {
    /// A string value.
    #[must_use]
    pub fn string(text: &str) -> Self {
        Self::String(text.to_owned())
    }

    /// An integer value.
    #[must_use]
    pub fn integer(number: u64) -> Self {
        Self::Number(number.to_string())
    }

    /// An object from members.
    #[must_use]
    pub fn object(members: &[(&str, Value)]) -> Self {
        Self::Object(members.iter().map(|(key, value)| ((*key).to_owned(), value.clone())).collect())
    }

    /// A member of an object.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&Value> {
        match self {
            Self::Object(members) => members.iter().find(|(name, _)| name == key).map(|(_, value)| value),
            _ => None,
        }
    }

    /// The string, if this is one.
    #[must_use]
    pub fn as_str(&self) -> Option<&str> {
        if let Self::String(text) = self { Some(text) } else { None }
    }

    /// The boolean, if this is one.
    #[must_use]
    pub fn as_bool(&self) -> Option<bool> {
        if let Self::Bool(value) = self { Some(*value) } else { None }
    }

    /// The number, if this is a whole number that fits.
    #[must_use]
    pub fn as_u64(&self) -> Option<u64> {
        if let Self::Number(text) = self { text.parse().ok() } else { None }
    }

    /// Reads one document.
    ///
    /// # Errors
    /// [`JsonError::Malformed`] for malformed JSON or nesting past the
    /// reader's limit, [`JsonError::TooManyValues`] for more values than
    /// the item limit (`MAX_ITEMS`), [`JsonError::BadString`] for a string
    /// that would not decode.
    pub fn parse(bytes: &[u8]) -> Result<Self, JsonError> {
        let mut reader = JsonReader::new(bytes).map_err(|_| JsonError::Malformed)?;
        let mut items = 0_usize;
        let first = next(&mut reader)?;
        let value = read(&mut reader, first, &mut items)?;
        if reader.next_token().map_err(|_| JsonError::Malformed)?.is_some() {
            return Err(JsonError::Malformed);
        }
        Ok(value)
    }

    /// The canonical text: what qrkeyboard's `canonicalJson` writes for the
    /// same value.
    #[must_use]
    pub fn canonical(&self) -> String {
        let mut out = String::new();
        self.write(&mut out, true);
        out
    }

    /// The text with members in their own order.
    #[must_use]
    pub fn text(&self) -> String {
        let mut out = String::new();
        self.write(&mut out, false);
        out
    }

    fn write(&self, out: &mut String, sorted: bool) {
        match self {
            Self::Null => out.push_str("null"),
            Self::Bool(value) => out.push_str(if *value { "true" } else { "false" }),
            Self::Number(text) => out.push_str(text),
            Self::String(text) => quote(text, out),
            Self::Array(values) => {
                out.push('[');
                for (index, value) in values.iter().enumerate() {
                    if index > 0 {
                        out.push(',');
                    }
                    value.write(out, sorted);
                }
                out.push(']');
            }
            Self::Object(members) => {
                let mut order: Vec<&(String, Value)> = members.iter().collect();
                if sorted {
                    // JavaScript sorts keys by UTF-16 code units; for the
                    // keys qrkeyboard uses, which are ASCII, that is byte
                    // order.
                    order.sort_by(|left, right| left.0.encode_utf16().cmp(right.0.encode_utf16()));
                }
                out.push('{');
                for (index, (key, value)) in order.into_iter().enumerate() {
                    if index > 0 {
                        out.push(',');
                    }
                    quote(key, out);
                    out.push(':');
                    value.write(out, sorted);
                }
                out.push('}');
            }
        }
    }
}

/// Why [`Value::parse`] refused a document.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JsonError {
    /// Not JSON: a broken token, a document that ends early or has more
    /// after its value, or nesting deeper than the engine's reader allows.
    Malformed,
    /// More members and elements than `MAX_ITEMS`, 4,096, in the document.
    TooManyValues,
    /// A string or a key that would not decode. The engine's reader checks
    /// each string's escapes and UTF-8 as it reads it, so a document does
    /// not reach this; a decode that failed is refused all the same rather
    /// than trusted.
    BadString,
}

impl core::fmt::Display for JsonError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter.write_str(match self {
            Self::Malformed => "JSON",
            Self::TooManyValues => "too many values",
            Self::BadString => "JSON string",
        })
    }
}

impl std::error::Error for JsonError {}

/// The next token, which must exist: the document has not ended.
fn next<'a>(reader: &mut JsonReader<'a>) -> Result<Token<'a>, JsonError> {
    reader.next_token().map_err(|_| JsonError::Malformed)?.ok_or(JsonError::Malformed)
}

fn read(reader: &mut JsonReader<'_>, token: Token<'_>, items: &mut usize) -> Result<Value, JsonError> {
    *items = items.saturating_add(1);
    if *items > MAX_ITEMS {
        return Err(JsonError::TooManyValues);
    }
    Ok(match token {
        Token::Null => Value::Null,
        Token::Bool(value) => Value::Bool(value),
        Token::Number(text) => Value::Number(String::from_utf8_lossy(text).into_owned()),
        Token::Str(text) => Value::String(decode(text)?),
        Token::BeginArray => {
            let mut values = Vec::new();
            loop {
                match next(reader)? {
                    Token::EndArray => break,
                    token => values.push(read(reader, token, items)?),
                }
            }
            Value::Array(values)
        }
        Token::BeginObject => {
            let mut members = Vec::new();
            loop {
                match next(reader)? {
                    Token::EndObject => break,
                    Token::Key(key) => {
                        let key = decode(key)?;
                        let token = next(reader)?;
                        members.push((key, read(reader, token, items)?));
                    }
                    _ => return Err(JsonError::Malformed),
                }
            }
            Value::Object(members)
        }
        Token::EndArray | Token::EndObject | Token::Key(_) => return Err(JsonError::Malformed),
    })
}

fn decode(text: neoteletext_web::json::JsonStr<'_>) -> Result<String, JsonError> {
    // Decoding never makes a string longer than its escaped form.
    let mut buffer = vec![0_u8; text.raw_len()];
    let length = text.decode_into(&mut buffer).map_err(|_| JsonError::BadString)?;
    buffer.truncate(length);
    String::from_utf8(buffer).map_err(|_| JsonError::BadString)
}

/// A string as `JSON.stringify` writes it (ECMA-262 `QuoteJSONString`):
/// the five short escapes, other controls as `\u00xx` in lower case,
/// everything else as it is.
fn quote(text: &str, out: &mut String) {
    out.push('"');
    for character in text.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            control if u32::from(control) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", u32::from(control));
            }
            other => out.push(other),
        }
    }
    out.push('"');
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    #[test]
    fn values_round_trip_and_sort_canonically() {
        let text = r#"{"b":[1,true,null,"x\n\u0001\"é"],"a":{"d":2,"c":-3.5}}"#.as_bytes();
        let value = Value::parse(text).unwrap();
        assert_eq!(value.canonical(), r#"{"a":{"c":-3.5,"d":2},"b":[1,true,null,"x\n\u0001\"é"]}"#);
        assert_eq!(value.text().as_bytes(), text);
        assert_eq!(value.get("a").and_then(|a| a.get("d")).and_then(Value::as_u64), Some(2));
    }

    /// REQ-155: each refusal is its own variant of an error enumeration.
    #[test]
    fn malformed_documents_are_refused() {
        for text in [b"{".as_slice(), b"{\"a\":1}x", b"[1,]", b"\"\xff\""] {
            assert_eq!(Value::parse(text), Err(JsonError::Malformed), "{text:?}");
        }
        let deep = "[".repeat(100) + &"]".repeat(100);
        assert_eq!(Value::parse(deep.as_bytes()), Err(JsonError::Malformed), "nesting is bounded");
        let many = format!("[{}0]", "0,".repeat(MAX_ITEMS));
        assert_eq!(Value::parse(many.as_bytes()), Err(JsonError::TooManyValues));
        assert_eq!(JsonError::TooManyValues.to_string(), "too many values");
    }
}
