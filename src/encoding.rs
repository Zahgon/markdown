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

//! Text encodings of the command line: codec name lookup as Python's `codecs.lookup` does it, strict decoding
//! with Python's error messages, and encoding with the `xmlcharrefreplace` error handler.
//!
//! Standard input is decoded as UTF-8 with Python's `surrogateescape` handler: an undecodable byte `b`
//! becomes the lone surrogate `U+DC00 + b`. Rust strings cannot hold surrogates, so such a byte is carried as
//! the private use character `U+10FF00 + b` and written back out as the surrogate's character reference.

use crate::core::PyError;
use crate::encoding_tables::{ALIASES, CP1252};

/// The supported codecs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Codec {
    Utf8,
    Utf8Sig,
    Latin1,
    Ascii,
    Cp1252,
    Utf16,
    Utf16Le,
    Utf16Be,
    Utf32,
    Utf32Le,
    Utf32Be,
}

const ESCAPE_BASE: u32 = 0x10FF00;

fn module_codec(name: &str) -> Option<Codec> {
    Some(match name {
        "utf_8" => Codec::Utf8,
        "utf_8_sig" => Codec::Utf8Sig,
        "latin_1" => Codec::Latin1,
        "ascii" => Codec::Ascii,
        "cp1252" => Codec::Cp1252,
        "utf_16" => Codec::Utf16,
        "utf_16_le" => Codec::Utf16Le,
        "utf_16_be" => Codec::Utf16Be,
        "utf_32" => Codec::Utf32,
        "utf_32_le" => Codec::Utf32Le,
        "utf_32_be" => Codec::Utf32Be,
        _ => return None,
    })
}

/// CPython's `_Py_normalize_encoding`: runs of ASCII letters, digits and dots, lower-cased and joined by `_`;
/// every other character (any non-ASCII one included) separates them.
fn normalize_encoding(name: &str) -> String {
    let mut out = String::new();
    let mut punct = false;
    for c in name.chars() {
        if c.is_ascii_alphanumeric() || c == '.' {
            if punct && !out.is_empty() {
                out.push('_');
            }
            out.push(c.to_ascii_lowercase());
            punct = false;
        } else {
            punct = true;
        }
    }
    out
}

/// `codecs.lookup(name)`; the error is Python's `LookupError`.
pub fn lookup(name: &str) -> Result<Codec, PyError> {
    let norm = normalize_encoding(name);
    let alias = |key: &str| ALIASES.iter().find(|(k, _)| *k == key).map(|(_, v)| *v);
    let aliased = alias(&norm).or_else(|| alias(&norm.replace('.', "_")));
    let candidates: Vec<&str> = aliased
        .into_iter()
        .chain(std::iter::once(norm.as_str()))
        .collect();
    for module in candidates {
        if module.is_empty() || module.contains('.') {
            continue;
        }
        if let Some(codec) = module_codec(module) {
            return Ok(codec);
        }
    }
    Err(PyError::new(
        "LookupError",
        format!("unknown encoding: {name}"),
    ))
}

fn decode_error(codec: &str, data: &[u8], start: usize, end: usize, reason: &str) -> PyError {
    let message = if end == start + 1 {
        format!(
            "'{codec}' codec can't decode byte 0x{:02x} in position {start}: {reason}",
            data[start]
        )
    } else {
        format!(
            "'{codec}' codec can't decode bytes in position {start}-{}: {reason}",
            end - 1
        )
    };
    PyError::new("UnicodeDecodeError", message)
}

/// The kinds of UTF-8 errors CPython reports, with the length of the bad sequence.
enum Utf8Error {
    EndOfData,
    InvalidStart,
    InvalidContinuation(usize),
}

/// Decode one UTF-8 sequence at `s`: the character and its length, or CPython's error classification.
fn utf8_char(data: &[u8], s: usize) -> Result<(char, usize), Utf8Error> {
    let cont = |b: u8| (0x80..0xC0).contains(&b);
    let ch = data[s];
    let rest = data.len() - s;
    if ch < 0x80 {
        return Ok((ch as char, 1));
    }
    if ch < 0xE0 {
        if ch < 0xC2 {
            return Err(Utf8Error::InvalidStart);
        }
        if rest < 2 {
            return Err(Utf8Error::EndOfData);
        }
        if !cont(data[s + 1]) {
            return Err(Utf8Error::InvalidContinuation(1));
        }
        let cp = ((ch as u32 & 0x1F) << 6) | (data[s + 1] as u32 & 0x3F);
        return Ok((char::from_u32(cp).expect("valid"), 2));
    }
    if ch < 0xF0 {
        let bad2 = |c2: u8| !cont(c2) || (if c2 < 0xA0 { ch == 0xE0 } else { ch == 0xED });
        if rest < 3 {
            if rest < 2 {
                return Err(Utf8Error::EndOfData);
            }
            if bad2(data[s + 1]) {
                return Err(Utf8Error::InvalidContinuation(1));
            }
            return Err(Utf8Error::EndOfData);
        }
        if bad2(data[s + 1]) {
            return Err(Utf8Error::InvalidContinuation(1));
        }
        if !cont(data[s + 2]) {
            return Err(Utf8Error::InvalidContinuation(2));
        }
        let cp = ((ch as u32 & 0x0F) << 12)
            | ((data[s + 1] as u32 & 0x3F) << 6)
            | (data[s + 2] as u32 & 0x3F);
        return Ok((char::from_u32(cp).expect("valid"), 3));
    }
    if ch < 0xF5 {
        let bad2 = |c2: u8| !cont(c2) || (if c2 < 0x90 { ch == 0xF0 } else { ch == 0xF4 });
        if rest < 4 {
            if rest < 2 {
                return Err(Utf8Error::EndOfData);
            }
            if bad2(data[s + 1]) {
                return Err(Utf8Error::InvalidContinuation(1));
            }
            if rest < 3 {
                return Err(Utf8Error::EndOfData);
            }
            if !cont(data[s + 2]) {
                return Err(Utf8Error::InvalidContinuation(2));
            }
            return Err(Utf8Error::EndOfData);
        }
        if bad2(data[s + 1]) {
            return Err(Utf8Error::InvalidContinuation(1));
        }
        if !cont(data[s + 2]) {
            return Err(Utf8Error::InvalidContinuation(2));
        }
        if !cont(data[s + 3]) {
            return Err(Utf8Error::InvalidContinuation(3));
        }
        let cp = ((ch as u32 & 0x07) << 18)
            | ((data[s + 1] as u32 & 0x3F) << 12)
            | ((data[s + 2] as u32 & 0x3F) << 6)
            | (data[s + 3] as u32 & 0x3F);
        return Ok((char::from_u32(cp).expect("valid"), 4));
    }
    Err(Utf8Error::InvalidStart)
}

/// Decode UTF-8. With `escape`, bad bytes become escape characters (`surrogateescape`); otherwise the first
/// error is returned.
fn decode_utf8(data: &[u8], escape: bool) -> Result<String, PyError> {
    let mut out = String::with_capacity(data.len());
    let mut s = 0;
    while s < data.len() {
        match utf8_char(data, s) {
            Ok((c, n)) => {
                out.push(c);
                s += n;
            }
            Err(e) => {
                let (end, reason) = match e {
                    Utf8Error::EndOfData => (data.len(), "unexpected end of data"),
                    Utf8Error::InvalidStart => (s + 1, "invalid start byte"),
                    Utf8Error::InvalidContinuation(n) => (s + n, "invalid continuation byte"),
                };
                if !escape {
                    return Err(decode_error("utf-8", data, s, end, reason));
                }
                for &b in &data[s..end] {
                    out.push(
                        char::from_u32(ESCAPE_BASE + b as u32).expect("a private use character"),
                    );
                }
                s = end;
            }
        }
    }
    Ok(out)
}

/// Decode standard input: UTF-8 with `surrogateescape`.
pub fn decode_stdin(data: &[u8]) -> String {
    decode_utf8(data, true).unwrap_or_default()
}

fn decode_utf16(
    data: &[u8],
    big_endian: Option<bool>,
    name_le: &str,
    name_be: &str,
) -> Result<String, PyError> {
    let mut start = 0;
    let be = match big_endian {
        Some(be) => be,
        None => {
            if data.starts_with(&[0xFF, 0xFE]) {
                start = 2;
                false
            } else if data.starts_with(&[0xFE, 0xFF]) {
                start = 2;
                true
            } else {
                false
            }
        }
    };
    let codec = if be { name_be } else { name_le };
    let unit = |i: usize| -> u32 {
        if be {
            (data[i] as u32) << 8 | data[i + 1] as u32
        } else {
            (data[i + 1] as u32) << 8 | data[i] as u32
        }
    };
    let mut out = String::new();
    let mut q = start;
    while q < data.len() {
        if q + 1 >= data.len() {
            return Err(decode_error(codec, data, q, data.len(), "truncated data"));
        }
        let u = unit(q);
        q += 2;
        if !(0xD800..0xE000).contains(&u) {
            out.push(char::from_u32(u).expect("not a surrogate"));
            continue;
        }
        if u >= 0xDC00 {
            return Err(decode_error(codec, data, q - 2, q, "illegal encoding"));
        }
        if q + 1 >= data.len() {
            return Err(decode_error(
                codec,
                data,
                q - 2,
                data.len(),
                "unexpected end of data",
            ));
        }
        let low = unit(q);
        if !(0xDC00..0xE000).contains(&low) {
            return Err(decode_error(
                codec,
                data,
                q - 2,
                q,
                "illegal UTF-16 surrogate",
            ));
        }
        q += 2;
        out.push(
            char::from_u32(0x10000 + ((u - 0xD800) << 10) + (low - 0xDC00))
                .expect("a supplementary character"),
        );
    }
    Ok(out)
}

fn decode_utf32(data: &[u8], big_endian: Option<bool>) -> Result<String, PyError> {
    let mut start = 0;
    let be = match big_endian {
        Some(be) => be,
        None => {
            if data.starts_with(&[0xFF, 0xFE, 0, 0]) {
                start = 4;
                false
            } else if data.starts_with(&[0, 0, 0xFE, 0xFF]) {
                start = 4;
                true
            } else {
                false
            }
        }
    };
    let codec = if be { "utf-32-be" } else { "utf-32-le" };
    let mut out = String::new();
    let mut q = start;
    while q < data.len() {
        if q + 3 >= data.len() {
            return Err(decode_error(codec, data, q, data.len(), "truncated data"));
        }
        let b = &data[q..q + 4];
        let cp = if be {
            u32::from_be_bytes([b[0], b[1], b[2], b[3]])
        } else {
            u32::from_le_bytes([b[0], b[1], b[2], b[3]])
        };
        if cp > 0x10FFFF {
            return Err(decode_error(
                codec,
                data,
                q,
                q + 4,
                "code point not in range(0x110000)",
            ));
        }
        if (0xD800..0xE000).contains(&cp) {
            return Err(decode_error(
                codec,
                data,
                q,
                q + 4,
                "code point in surrogate code point range(0xd800, 0xe000)",
            ));
        }
        out.push(char::from_u32(cp).expect("a valid code point"));
        q += 4;
    }
    Ok(out)
}

/// Decode a file's bytes strictly.
pub fn decode(data: &[u8], codec: Codec) -> Result<String, PyError> {
    match codec {
        Codec::Utf8 => decode_utf8(data, false),
        Codec::Utf8Sig => {
            let data = data.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(data);
            decode_utf8(data, false)
        }
        Codec::Latin1 => Ok(data.iter().map(|&b| b as char).collect()),
        Codec::Ascii => {
            let mut out = String::with_capacity(data.len());
            for (i, &b) in data.iter().enumerate() {
                if b >= 0x80 {
                    return Err(decode_error(
                        "ascii",
                        data,
                        i,
                        i + 1,
                        "ordinal not in range(128)",
                    ));
                }
                out.push(b as char);
            }
            Ok(out)
        }
        Codec::Cp1252 => {
            let mut out = String::with_capacity(data.len());
            for (i, &b) in data.iter().enumerate() {
                match CP1252[b as usize] {
                    Some(c) => out.push(c),
                    None => {
                        return Err(decode_error(
                            "charmap",
                            data,
                            i,
                            i + 1,
                            "character maps to <undefined>",
                        ))
                    }
                }
            }
            Ok(out)
        }
        Codec::Utf16 => decode_utf16(data, None, "utf-16-le", "utf-16-be"),
        Codec::Utf16Le => decode_utf16(data, Some(false), "utf-16-le", "utf-16-be"),
        Codec::Utf16Be => decode_utf16(data, Some(true), "utf-16-le", "utf-16-be"),
        Codec::Utf32 => decode_utf32(data, None),
        Codec::Utf32Le => decode_utf32(data, Some(false)),
        Codec::Utf32Be => decode_utf32(data, Some(true)),
    }
}

/// The code point Python would see for a character: escape characters stand for lone surrogates.
fn python_code_point(c: char) -> u32 {
    let cp = c as u32;
    if (ESCAPE_BASE + 0x80..=ESCAPE_BASE + 0xFF).contains(&cp) {
        0xDC00 + (cp - ESCAPE_BASE)
    } else {
        cp
    }
}

/// Encode with the `xmlcharrefreplace` error handler.
pub fn encode(text: &str, codec: Codec) -> Vec<u8> {
    let mut out = Vec::with_capacity(text.len());
    match codec {
        Codec::Utf8Sig => out.extend_from_slice(&[0xEF, 0xBB, 0xBF]),
        Codec::Utf16 => out.extend_from_slice(&[0xFF, 0xFE]),
        Codec::Utf32 => out.extend_from_slice(&[0xFF, 0xFE, 0, 0]),
        _ => {}
    }
    let cp1252_byte = |c: char| CP1252.iter().position(|&m| m == Some(c)).map(|b| b as u8);
    for c in text.chars() {
        let cp = python_code_point(c);
        let surrogate = (0xD800..0xE000).contains(&cp);
        let encoded: Option<Vec<u8>> = match codec {
            Codec::Utf8 | Codec::Utf8Sig if !surrogate => Some(c.to_string().into_bytes()),
            Codec::Latin1 if cp < 0x100 => Some(vec![cp as u8]),
            Codec::Ascii if cp < 0x80 => Some(vec![cp as u8]),
            Codec::Cp1252 if !surrogate => cp1252_byte(c).map(|b| vec![b]),
            Codec::Utf16 | Codec::Utf16Le | Codec::Utf16Be if !surrogate => {
                let mut units = [0u16; 2];
                let be = codec == Codec::Utf16Be;
                Some(
                    c.encode_utf16(&mut units)
                        .iter()
                        .flat_map(|u| if be { u.to_be_bytes() } else { u.to_le_bytes() })
                        .collect(),
                )
            }
            Codec::Utf32 | Codec::Utf32Le | Codec::Utf32Be if !surrogate => {
                Some(if codec == Codec::Utf32Be {
                    cp.to_be_bytes().to_vec()
                } else {
                    cp.to_le_bytes().to_vec()
                })
            }
            _ => None,
        };
        match encoded {
            Some(bytes) => out.extend(bytes),
            None => {
                // The reference is ASCII text, encoded with the same codec.
                for r in format!("&#{cp};").chars() {
                    out.extend(encode_ascii_char(r, codec));
                }
            }
        }
    }
    out
}

fn encode_ascii_char(c: char, codec: Codec) -> Vec<u8> {
    let b = c as u8;
    match codec {
        Codec::Utf16 | Codec::Utf16Le => vec![b, 0],
        Codec::Utf16Be => vec![0, b],
        Codec::Utf32 | Codec::Utf32Le => vec![b, 0, 0, 0],
        Codec::Utf32Be => vec![0, 0, 0, b],
        _ => vec![b],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_resolve_like_python() {
        assert_eq!(lookup("UTF-8"), Ok(Codec::Utf8));
        assert_eq!(lookup("utf 8"), Ok(Codec::Utf8));
        assert_eq!(lookup("Latin1"), Ok(Codec::Latin1));
        assert_eq!(lookup("windows-1252"), Ok(Codec::Cp1252));
        assert_eq!(lookup("utf-\u{ff18}"), Ok(Codec::Utf8));
        assert!(lookup("\u{ff55}tf-8").is_err());
        assert!(lookup("cp-1252").is_err());
        assert!(lookup("utf.8").is_err());
    }

    #[test]
    fn utf8_errors_match_cpython() {
        let e = decode(b"a\xe2\x82", Codec::Utf8).unwrap_err();
        assert_eq!(
            e.message,
            "'utf-8' codec can't decode bytes in position 1-2: unexpected end of data"
        );
        let e = decode(b"\xed\xa0\x80", Codec::Utf8).unwrap_err();
        assert_eq!(
            e.message,
            "'utf-8' codec can't decode byte 0xed in position 0: invalid continuation byte"
        );
    }

    #[test]
    fn surrogate_escapes_round_trip() {
        let s = decode_stdin(b"a\xffb");
        assert_eq!(encode(&s, Codec::Utf8), b"a&#56575;b".to_vec());
    }
}
