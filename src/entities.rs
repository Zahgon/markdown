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

//! HTML character references: Python's `html.unescape` and the HTML 4 entity names.

use std::sync::OnceLock;

use crate::entities_table::{CODEPOINT2NAME, HTML5, INVALID_CHARREFS, INVALID_CODEPOINTS};
use crate::pyre::Regex;

/// `html.entities.codepoint2name.get(code)`.
pub fn codepoint2name(code: u32) -> Option<&'static str> {
    CODEPOINT2NAME
        .binary_search_by_key(&code, |&(k, _)| k)
        .ok()
        .map(|i| CODEPOINT2NAME[i].1)
}

fn html5(name: &str) -> Option<&'static str> {
    HTML5
        .binary_search_by(|&(k, _)| k.cmp(name))
        .ok()
        .map(|i| HTML5[i].1)
}

fn replace_charref(s: &str) -> String {
    if let Some(num) = s.strip_prefix('#') {
        let parsed = match num.strip_prefix(['x', 'X']) {
            Some(hex) => u64::from_str_radix(hex.trim_end_matches(';'), 16),
            None => num.trim_end_matches(';').parse::<u64>(),
        };
        // Digits beyond u64 are past U+10FFFF as well.
        let num = parsed.unwrap_or(u64::MAX);
        if let Ok(i) = INVALID_CHARREFS.binary_search_by_key(&num, |&(k, _)| k as u64) {
            return INVALID_CHARREFS[i].1.to_string();
        }
        if (0xD800..=0xDFFF).contains(&num) || num > 0x10FFFF {
            return "\u{fffd}".into();
        }
        if INVALID_CODEPOINTS.contains(&(num as u32)) {
            return String::new();
        }
        return char::from_u32(num as u32)
            .map(String::from)
            .unwrap_or_default();
    }
    if let Some(v) = html5(s) {
        return v.to_string();
    }
    // The longest name that is a prefix of the reference, as the HTML standard defines it.
    let boundaries: Vec<usize> = s.char_indices().map(|(i, _)| i).collect();
    for x in (2..boundaries.len()).rev() {
        let (head, rest) = s.split_at(boundaries[x]);
        if let Some(v) = html5(head) {
            return format!("{v}{rest}");
        }
    }
    format!("&{s}")
}

/// `html.unescape(s)`: replace named and numeric character references.
pub fn unescape(s: &str) -> String {
    static CHARREF: OnceLock<Regex> = OnceLock::new();
    if !s.contains('&') {
        return s.to_string();
    }
    let re = CHARREF.get_or_init(|| {
        Regex::new(
            r"&(#[0-9]+;?|#[xX][0-9a-fA-F]+;?|[^\t\n\f <&#;]{1,32};?)",
            0,
        )
    });
    re.sub_fn(s, |m| replace_charref(m.str(1)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn named_and_numeric() {
        assert_eq!(unescape("&amp;&lt;&#65;&#x42;&copy"), "&<AB\u{a9}");
        assert_eq!(unescape("&notit;"), "\u{ac}it;");
        assert_eq!(unescape("&#128;&#0;&#x110000;"), "\u{20ac}\u{fffd}\u{fffd}");
        assert_eq!(unescape("&bogus;"), "&bogus;");
        assert_eq!(codepoint2name(0x22), Some("quot"));
    }
}
