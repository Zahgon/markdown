// Python Markdown
//
// A Python implementation of John Gruber's Markdown.
//
// Documentation: https://python-markdown.github.io/
// GitHub: https://github.com/Python-Markdown/markdown/
// PyPI: https://pypi.org/project/Markdown/
//
// Started by Manfred Stienstra (http://www.dwerg.net/).
// Maintained for a few years by Yuri Takhteyev (http://www.freewisdom.org).
// Currently maintained by Waylan Limberg (https://github.com/waylan),
// Dmitry Shachnev (https://github.com/mitya57) and Isaac Muse (https://github.com/facelessuser).
//
// Copyright 2007-2023 The Python Markdown Project (v. 1.7 and later)
// Copyright 2004, 2005, 2006 Yuri Takhteyev (v. 0.2-1.6b)
// Copyright 2004 Manfred Stienstra (the original version)
//
// License: BSD (see LICENSE.md for details).

//! Extension configuration values.
//!
//! The command line reads extension configurations from a JSON file the way Python's `json` module does, and
//! the extensions then treat the values with Python semantics: truthiness, `str()` when a value ends up in the
//! output, and `parseBoolValue` for boolean options. [`Value`] models the decoded JSON value.

use crate::pystr::repr_str;

/// A decoded JSON value, as Python's `json` module produces it.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    None,
    Bool(bool),
    Int(i128),
    Float(f64),
    Str(String),
    List(Vec<Value>),
    /// An object; keys keep their first position and the last value wins, as in a Python dict.
    Dict(Vec<(String, Value)>),
    /// A Python function: the default of options such as `slugify`, which JSON cannot express.
    Callable(&'static str),
}

impl Value {
    /// Python truthiness.
    pub fn truthy(&self) -> bool {
        match self {
            Value::None => false,
            Value::Bool(b) => *b,
            Value::Int(n) => *n != 0,
            Value::Float(f) => *f != 0.0,
            Value::Str(s) => !s.is_empty(),
            Value::List(l) => !l.is_empty(),
            Value::Dict(d) => !d.is_empty(),
            Value::Callable(_) => true,
        }
    }

    /// The Python type name, as error messages print it.
    pub fn type_name(&self) -> &'static str {
        match self {
            Value::None => "NoneType",
            Value::Bool(_) => "bool",
            Value::Int(_) => "int",
            Value::Float(_) => "float",
            Value::Str(_) => "str",
            Value::List(_) => "list",
            Value::Dict(_) => "dict",
            Value::Callable(_) => "function",
        }
    }

    /// `str(value)`.
    pub fn py_str(&self) -> String {
        match self {
            Value::Str(s) => s.clone(),
            other => other.py_repr(),
        }
    }

    /// `repr(value)`.
    pub fn py_repr(&self) -> String {
        match self {
            Value::None => "None".into(),
            Value::Bool(true) => "True".into(),
            Value::Bool(false) => "False".into(),
            Value::Int(n) => n.to_string(),
            Value::Float(f) => float_repr(*f),
            Value::Str(s) => repr_str(s),
            Value::List(items) => format!(
                "[{}]",
                items
                    .iter()
                    .map(Value::py_repr)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Value::Dict(items) => format!(
                "{{{}}}",
                items
                    .iter()
                    .map(|(k, v)| format!("{}: {}", repr_str(k), v.py_repr()))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Value::Callable(name) => format!("<function {name}>"),
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::Str(s) => Some(s),
            _ => None,
        }
    }

    /// `dict.get(key)` for a dict value.
    pub fn get(&self, key: &str) -> Option<&Value> {
        match self {
            Value::Dict(items) => items.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }
}

/// `repr()` of a float: the shortest round-tripping digits, in exponent form below 1e-4 and from 1e16 on.
pub fn float_repr(f: f64) -> String {
    if f.is_nan() {
        return "nan".into();
    }
    if f.is_infinite() {
        return if f > 0.0 { "inf".into() } else { "-inf".into() };
    }
    if f == 0.0 {
        return if f.is_sign_negative() {
            "-0.0".into()
        } else {
            "0.0".into()
        };
    }
    // Rust's `{:e}` gives the shortest round-tripping digits as `d.ddde<exp>`.
    let sci = format!("{:e}", f.abs());
    let (mantissa, exp) = sci.split_once('e').unwrap_or((&sci, "0"));
    let exp: i32 = exp.parse().unwrap_or(0);
    let digits: String = mantissa.chars().filter(|c| *c != '.').collect();
    let sign = if f < 0.0 { "-" } else { "" };
    if !(-4..16).contains(&exp) {
        let m = if digits.len() > 1 {
            format!("{}.{}", &digits[..1], &digits[1..])
        } else {
            digits.clone()
        };
        return format!(
            "{sign}{m}e{}{:02}",
            if exp < 0 { '-' } else { '+' },
            exp.abs()
        );
    }
    if exp < 0 {
        let zeros = "0".repeat((-exp - 1) as usize);
        return format!("{sign}0.{zeros}{digits}");
    }
    let point = exp as usize + 1;
    if digits.len() <= point {
        format!("{sign}{digits}{}.0", "0".repeat(point - digits.len()))
    } else {
        format!("{sign}{}.{}", &digits[..point], &digits[point..])
    }
}

/// `util.parseBoolValue(value, fail_on_errors, preserve_none)`. `Ok(None)` is Python's `None`; the error is
/// the `ValueError` message.
pub fn parse_bool_value(
    value: &Value,
    fail_on_errors: bool,
    preserve_none: bool,
) -> Result<Option<bool>, String> {
    let Value::Str(s) = value else {
        if preserve_none && *value == Value::None {
            return Ok(None);
        }
        return Ok(Some(value.truthy()));
    };
    let lower = crate::pystr::lower(s);
    if preserve_none && lower == "none" {
        Ok(None)
    } else if ["true", "yes", "y", "on", "1"].contains(&lower.as_str()) {
        Ok(Some(true))
    } else if ["false", "no", "n", "off", "0", "none"].contains(&lower.as_str()) {
        Ok(Some(false))
    } else if fail_on_errors {
        Err(format!("Cannot parse bool value: {}", repr_str(s)))
    } else {
        Ok(None)
    }
}

/// A document that `json.load` rejects.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvalidJson;

impl std::fmt::Display for InvalidJson {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("invalid JSON document")
    }
}

impl std::error::Error for InvalidJson {}

/// Parse a JSON document as `json.load` does (including `NaN`, `Infinity` and `-Infinity`).
pub fn parse_json(text: &str) -> Result<Value, InvalidJson> {
    let mut p = JsonParser {
        s: text.as_bytes(),
        text,
        i: 0,
    };
    p.ws();
    let v = p.value()?;
    p.ws();
    if p.i != p.s.len() {
        return Err(InvalidJson);
    }
    Ok(v)
}

struct JsonParser<'a> {
    s: &'a [u8],
    text: &'a str,
    i: usize,
}

impl JsonParser<'_> {
    fn ws(&mut self) {
        while self.i < self.s.len() && matches!(self.s[self.i], b' ' | b'\t' | b'\n' | b'\r') {
            self.i += 1;
        }
    }

    fn eat(&mut self, lit: &str) -> bool {
        if self.text[self.i..].starts_with(lit) {
            self.i += lit.len();
            true
        } else {
            false
        }
    }

    fn value(&mut self) -> Result<Value, InvalidJson> {
        match self.s.get(self.i) {
            Some(b'{') => self.object(),
            Some(b'[') => self.array(),
            Some(b'"') => self.string().map(Value::Str),
            Some(b'n') if self.eat("null") => Ok(Value::None),
            Some(b't') if self.eat("true") => Ok(Value::Bool(true)),
            Some(b'f') if self.eat("false") => Ok(Value::Bool(false)),
            Some(b'N') if self.eat("NaN") => Ok(Value::Float(f64::NAN)),
            Some(b'I') if self.eat("Infinity") => Ok(Value::Float(f64::INFINITY)),
            Some(b'-') if self.text[self.i..].starts_with("-Infinity") => {
                self.i += "-Infinity".len();
                Ok(Value::Float(f64::NEG_INFINITY))
            }
            Some(b'-' | b'0'..=b'9') => self.number(),
            _ => Err(InvalidJson),
        }
    }

    fn object(&mut self) -> Result<Value, InvalidJson> {
        self.i += 1;
        let mut items: Vec<(String, Value)> = Vec::new();
        self.ws();
        if self.s.get(self.i) == Some(&b'}') {
            self.i += 1;
            return Ok(Value::Dict(items));
        }
        loop {
            self.ws();
            if self.s.get(self.i) != Some(&b'"') {
                return Err(InvalidJson);
            }
            let key = self.string()?;
            self.ws();
            if self.s.get(self.i) != Some(&b':') {
                return Err(InvalidJson);
            }
            self.i += 1;
            self.ws();
            let value = self.value()?;
            match items.iter_mut().find(|(k, _)| *k == key) {
                Some(slot) => slot.1 = value,
                None => items.push((key, value)),
            }
            self.ws();
            match self.s.get(self.i) {
                Some(b',') => self.i += 1,
                Some(b'}') => {
                    self.i += 1;
                    return Ok(Value::Dict(items));
                }
                _ => return Err(InvalidJson),
            }
        }
    }

    fn array(&mut self) -> Result<Value, InvalidJson> {
        self.i += 1;
        let mut items = Vec::new();
        self.ws();
        if self.s.get(self.i) == Some(&b']') {
            self.i += 1;
            return Ok(Value::List(items));
        }
        loop {
            self.ws();
            items.push(self.value()?);
            self.ws();
            match self.s.get(self.i) {
                Some(b',') => self.i += 1,
                Some(b']') => {
                    self.i += 1;
                    return Ok(Value::List(items));
                }
                _ => return Err(InvalidJson),
            }
        }
    }

    fn hex4(&mut self) -> Result<u32, InvalidJson> {
        let h = self.text.get(self.i..self.i + 4).ok_or(InvalidJson)?;
        if !h.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(InvalidJson);
        }
        self.i += 4;
        u32::from_str_radix(h, 16).map_err(|_| InvalidJson)
    }

    fn string(&mut self) -> Result<String, InvalidJson> {
        self.i += 1;
        let mut out = String::new();
        loop {
            let c = self.text[self.i..].chars().next().ok_or(InvalidJson)?;
            match c {
                '"' => {
                    self.i += 1;
                    return Ok(out);
                }
                '\\' => {
                    self.i += 1;
                    let e = *self.s.get(self.i).ok_or(InvalidJson)?;
                    self.i += 1;
                    match e {
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'/' => out.push('/'),
                        b'b' => out.push('\x08'),
                        b'f' => out.push('\x0c'),
                        b'n' => out.push('\n'),
                        b'r' => out.push('\r'),
                        b't' => out.push('\t'),
                        b'u' => {
                            let mut cp = self.hex4()?;
                            if (0xD800..0xDC00).contains(&cp)
                                && self.text[self.i..].starts_with("\\u")
                            {
                                let save = self.i;
                                self.i += 2;
                                let low = self.hex4()?;
                                if (0xDC00..0xE000).contains(&low) {
                                    cp = 0x10000 + ((cp - 0xD800) << 10) + (low - 0xDC00);
                                } else {
                                    self.i = save;
                                }
                            }
                            // A lone surrogate has no Rust representation; it stands for itself as U+FFFD.
                            out.push(char::from_u32(cp).unwrap_or('\u{fffd}'));
                        }
                        _ => return Err(InvalidJson),
                    }
                }
                c if (c as u32) < 0x20 => return Err(InvalidJson),
                c => {
                    out.push(c);
                    self.i += c.len_utf8();
                }
            }
        }
    }

    fn number(&mut self) -> Result<Value, InvalidJson> {
        let start = self.i;
        if self.s.get(self.i) == Some(&b'-') {
            self.i += 1;
        }
        match self.s.get(self.i) {
            Some(b'0') => self.i += 1,
            Some(b'1'..=b'9') => {
                while matches!(self.s.get(self.i), Some(b'0'..=b'9')) {
                    self.i += 1;
                }
            }
            _ => return Err(InvalidJson),
        }
        let mut is_float = false;
        if self.s.get(self.i) == Some(&b'.') && matches!(self.s.get(self.i + 1), Some(b'0'..=b'9'))
        {
            is_float = true;
            self.i += 1;
            while matches!(self.s.get(self.i), Some(b'0'..=b'9')) {
                self.i += 1;
            }
        }
        if matches!(self.s.get(self.i), Some(b'e' | b'E')) {
            let save = self.i;
            self.i += 1;
            if matches!(self.s.get(self.i), Some(b'+' | b'-')) {
                self.i += 1;
            }
            if matches!(self.s.get(self.i), Some(b'0'..=b'9')) {
                is_float = true;
                while matches!(self.s.get(self.i), Some(b'0'..=b'9')) {
                    self.i += 1;
                }
            } else {
                self.i = save;
            }
        }
        let lit = &self.text[start..self.i];
        if is_float {
            lit.parse::<f64>()
                .map(Value::Float)
                .map_err(|_| InvalidJson)
        } else {
            Ok(lit
                .parse::<i128>()
                .map(Value::Int)
                .unwrap_or(Value::Float(f64::INFINITY)))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn floats_print_like_python() {
        assert_eq!(float_repr(1e16), "1e+16");
        assert_eq!(float_repr(1e15), "1000000000000000.0");
        assert_eq!(float_repr(1e-5), "1e-05");
        assert_eq!(float_repr(0.0001), "0.0001");
        assert_eq!(float_repr(2.5), "2.5");
        assert_eq!(float_repr(1.2345678901234568e17), "1.2345678901234568e+17");
    }

    #[test]
    fn json_objects_keep_first_position() {
        let v = parse_json(r#"{"a": 1, "b": 2, "a": 3}"#).unwrap();
        assert_eq!(
            v,
            Value::Dict(vec![
                ("a".into(), Value::Int(3)),
                ("b".into(), Value::Int(2))
            ])
        );
        assert!(parse_json("{'a': 1}").is_err());
        assert_eq!(parse_json(" NaN ").map(|v| v.py_repr()), Ok("nan".into()));
    }

    #[test]
    fn bool_values() {
        assert_eq!(
            parse_bool_value(&Value::Str("Yes".into()), true, false),
            Ok(Some(true))
        );
        assert_eq!(
            parse_bool_value(&Value::Int(0), true, false),
            Ok(Some(false))
        );
        assert!(parse_bool_value(&Value::Str("maybe".into()), true, false).is_err());
        assert_eq!(
            parse_bool_value(&Value::Str("maybe".into()), false, false),
            Ok(None)
        );
    }
}
