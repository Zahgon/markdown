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

//! Python string semantics the Markdown pipeline depends on.
//!
//! Python-Markdown is written against Python's `str` methods: Unicode whitespace for `strip()` and `split()`,
//! full case mappings for `lower()`, column based `expandtabs()`, `int()` parsing and `str.format()`. Rust's
//! standard library differs in each of these, so this module reproduces the Python behaviour with the tables in
//! `unicode_tables`.

use crate::unicode_tables::{CAPITALIZE, DECIMAL_RUNS, DIGIT, LOWER, PRINTABLE, SPACE, WORD};

fn in_ranges(c: char, table: &[(u32, u32)]) -> bool {
    let cp = c as u32;
    table
        .binary_search_by(|&(lo, hi)| {
            if hi < cp {
                std::cmp::Ordering::Less
            } else if lo > cp {
                std::cmp::Ordering::Greater
            } else {
                std::cmp::Ordering::Equal
            }
        })
        .is_ok()
}

fn lookup<'a>(c: char, table: &'a [(u32, &'a str)]) -> Option<&'a str> {
    let cp = c as u32;
    table
        .binary_search_by_key(&cp, |&(k, _)| k)
        .ok()
        .map(|i| table[i].1)
}

/// `str.isspace()` for one character (also what `\s` matches in a `re` pattern).
pub fn is_space(c: char) -> bool {
    if c.is_ascii() {
        matches!(
            c,
            '\t' | '\n' | '\x0b' | '\x0c' | '\r' | '\x1c'..='\x1f' | ' '
        )
    } else {
        in_ranges(c, SPACE)
    }
}

/// What `\w` matches in a `re` pattern: `str.isalnum()` or `_`.
pub fn is_word(c: char) -> bool {
    if c.is_ascii() {
        c.is_ascii_alphanumeric() || c == '_'
    } else {
        in_ranges(c, WORD)
    }
}

/// What `\d` matches in a `re` pattern: `str.isdecimal()`.
pub fn is_digit(c: char) -> bool {
    if c.is_ascii() {
        c.is_ascii_digit()
    } else {
        in_ranges(c, DIGIT)
    }
}

/// `str.isprintable()` for one character.
pub fn is_printable(c: char) -> bool {
    in_ranges(c, PRINTABLE)
}

/// The value of a decimal digit (`unicodedata.decimal`).
pub fn decimal_value(c: char) -> Option<u32> {
    let cp = c as u32;
    let i = DECIMAL_RUNS.partition_point(|&(_, hi, _)| hi < cp);
    match DECIMAL_RUNS.get(i) {
        Some(&(lo, _, v)) if lo <= cp => Some(v + (cp - lo)),
        _ => None,
    }
}

/// `str.strip()`.
pub fn strip(s: &str) -> &str {
    s.trim_matches(is_space)
}

/// `str.lstrip()`.
pub fn lstrip(s: &str) -> &str {
    s.trim_start_matches(is_space)
}

/// `str.rstrip()`.
pub fn rstrip(s: &str) -> &str {
    s.trim_end_matches(is_space)
}

/// `str.strip(chars)`.
pub fn strip_chars<'a>(s: &'a str, chars: &str) -> &'a str {
    s.trim_matches(|c| chars.contains(c))
}

/// `str.lstrip(chars)`.
pub fn lstrip_chars<'a>(s: &'a str, chars: &str) -> &'a str {
    s.trim_start_matches(|c| chars.contains(c))
}

/// `str.rstrip(chars)`.
pub fn rstrip_chars<'a>(s: &'a str, chars: &str) -> &'a str {
    s.trim_end_matches(|c| chars.contains(c))
}

/// `str.lower()`, with Python's full (multi-character) mappings.
pub fn lower(s: &str) -> String {
    if s.is_ascii() {
        return s.to_ascii_lowercase();
    }
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if c.is_ascii() {
            out.push(c.to_ascii_lowercase());
        } else {
            match lookup(c, LOWER) {
                Some(m) => out.push_str(m),
                None => out.push(c),
            }
        }
    }
    out
}

/// `str.capitalize()`: the first character in title case, the rest lower case.
pub fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(first) => {
            let mut out = match lookup(first, CAPITALIZE) {
                Some(m) => m.to_string(),
                None => first.to_string(),
            };
            out.push_str(&lower(chars.as_str()));
            out
        }
    }
}

/// `str.split()` without arguments: runs of whitespace separate the words, empty words are dropped.
pub fn split_whitespace(s: &str) -> Vec<&str> {
    s.split(is_space).filter(|w| !w.is_empty()).collect()
}

/// `str.expandtabs(tabsize)`: columns count characters and restart after `\n` and `\r`.
pub fn expandtabs(s: &str, tabsize: usize) -> String {
    let mut out = String::with_capacity(s.len());
    let mut column = 0usize;
    for c in s.chars() {
        match c {
            '\t' => {
                if tabsize > 0 {
                    let n = tabsize - column % tabsize;
                    out.extend(std::iter::repeat_n(' ', n));
                    column += n;
                }
            }
            '\n' | '\r' => {
                out.push(c);
                column = 0;
            }
            _ => {
                out.push(c);
                column += 1;
            }
        }
    }
    out
}

/// The number of characters (Python's `len()`) of a string.
pub fn char_len(s: &str) -> usize {
    s.chars().count()
}

/// The byte offset of the `n`th character of `s` (or `s.len()` past the end).
pub fn char_to_byte(s: &str, n: usize) -> usize {
    s.char_indices().nth(n).map(|(i, _)| i).unwrap_or(s.len())
}

/// Conversions between byte offsets and Python's character indices of one string.
pub struct CharIndex {
    starts: Vec<usize>,
}

impl CharIndex {
    pub fn new(s: &str) -> CharIndex {
        let mut starts: Vec<usize> = s.char_indices().map(|(i, _)| i).collect();
        starts.push(s.len());
        CharIndex { starts }
    }

    /// The number of characters.
    pub fn len(&self) -> usize {
        self.starts.len() - 1
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The character index of a byte offset (a character boundary).
    pub fn to_char(&self, byte: usize) -> usize {
        self.starts.partition_point(|&b| b < byte)
    }

    /// The byte offset of a character index, clamped to the end.
    pub fn to_byte(&self, index: usize) -> usize {
        self.starts[index.min(self.len())]
    }

    /// `s[a:b]` with character indices and Python's handling of negative and reversed bounds.
    pub fn slice<'a>(&self, s: &'a str, a: isize, b: isize) -> &'a str {
        let len = self.len() as isize;
        let norm = |x: isize| if x < 0 { (len + x).max(0) } else { x.min(len) } as usize;
        let (a, b) = (norm(a), norm(b));
        if a >= b {
            ""
        } else {
            &s[self.to_byte(a)..self.to_byte(b)]
        }
    }
}

/// `repr()` of a string.
pub fn repr_str(s: &str) -> String {
    let quote = if s.contains('\'') && !s.contains('"') {
        '"'
    } else {
        '\''
    };
    let mut out = String::with_capacity(s.len() + 2);
    out.push(quote);
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c == quote => {
                out.push('\\');
                out.push(c);
            }
            c if c == ' ' || (c as u32 >= 0x20 && is_printable(c)) => out.push(c),
            c => {
                let cp = c as u32;
                if cp < 0x100 {
                    out.push_str(&format!("\\x{cp:02x}"));
                } else if cp < 0x10000 {
                    out.push_str(&format!("\\u{cp:04x}"));
                } else {
                    out.push_str(&format!("\\U{cp:08x}"));
                }
            }
        }
    }
    out.push(quote);
    out
}

/// `int(text)` for a string: surrounding whitespace, an optional sign, Unicode decimal digits and single
/// underscores between digits. The error is the text of Python's `ValueError`.
pub fn parse_int(text: &str) -> Result<i64, String> {
    let err = || format!("invalid literal for int() with base 10: {}", repr_str(text));
    let s = strip(text);
    let (negative, digits) = match s.chars().next() {
        Some('-') => (true, &s[1..]),
        Some('+') => (false, &s[1..]),
        _ => (false, s),
    };
    if digits.is_empty() {
        return Err(err());
    }
    let mut value: i64 = 0;
    let mut previous_underscore = true;
    for c in digits.chars() {
        if c == '_' {
            if previous_underscore {
                return Err(err());
            }
            previous_underscore = true;
            continue;
        }
        let d = decimal_value(c).ok_or_else(err)?;
        value = value.saturating_mul(10).saturating_add(d as i64);
        previous_underscore = false;
    }
    if previous_underscore {
        return Err(err());
    }
    Ok(if negative { -value } else { value })
}

/// One positional argument of [`format`].
pub enum FormatArg<'a> {
    Int(i64),
    Str(&'a str),
}

/// `template.format(*args)` for the field forms Python-Markdown's configuration strings use: automatic and
/// numbered fields, `!s`/`!r` conversions and the standard format specification for integers and strings.
/// The error is the text of the Python exception (with its type) when the template is not valid.
pub fn format(template: &str, args: &[FormatArg]) -> Result<String, String> {
    let mut out = String::new();
    let chars: Vec<char> = template.chars().collect();
    let mut i = 0;
    let mut auto_index: Option<usize> = None;
    let mut manual = false;
    while i < chars.len() {
        let c = chars[i];
        if c == '}' {
            if chars.get(i + 1) == Some(&'}') {
                out.push('}');
                i += 2;
                continue;
            }
            return Err("ValueError: Single '}' encountered in format string".into());
        }
        if c != '{' {
            out.push(c);
            i += 1;
            continue;
        }
        if chars.get(i + 1) == Some(&'{') {
            out.push('{');
            i += 2;
            continue;
        }
        // A replacement field: find its end, allowing one level of nested braces in the spec.
        let mut j = i + 1;
        let mut depth = 1;
        while j < chars.len() {
            match chars[j] {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
                _ => {}
            }
            j += 1;
        }
        if j >= chars.len() {
            return Err(if depth > 1 {
                "ValueError: expected '}' before end of string".into()
            } else {
                "ValueError: Single '{' encountered in format string".into()
            });
        }
        let field: String = chars[i + 1..j].iter().collect();
        i = j + 1;
        let (name_conv, spec) = match field.find(':') {
            Some(p) => (&field[..p], &field[p + 1..]),
            None => (&field[..], ""),
        };
        let (name, conversion) = match name_conv.find('!') {
            Some(p) => (&name_conv[..p], Some(&name_conv[p + 1..])),
            None => (name_conv, None),
        };
        let index = if name.is_empty() {
            if manual {
                return Err("ValueError: cannot switch from manual field specification to automatic field numbering"
                    .into());
            }
            let n = auto_index.map_or(0, |n| n + 1);
            auto_index = Some(n);
            n
        } else if name.chars().all(|c| c.is_ascii_digit()) {
            if auto_index.is_some() {
                return Err("ValueError: cannot switch from automatic field numbering to manual field specification"
                    .into());
            }
            manual = true;
            name.parse::<usize>().unwrap_or(usize::MAX)
        } else {
            return Err(format!("KeyError: {}", repr_str(name)));
        };
        let arg = args.get(index).ok_or_else(|| {
            format!("IndexError: Replacement index {index} out of range for positional args tuple")
        })?;
        let rendered = match conversion {
            None => format_value(arg, spec)?,
            Some("s") => format_str(&arg_to_string(arg), spec)?,
            Some("r") | Some("a") => format_str(&arg_repr(arg), spec)?,
            Some(other) => {
                return Err(match other.chars().next() {
                    None => {
                        "ValueError: end of string while looking for conversion specifier".into()
                    }
                    Some(c) if other.chars().count() == 1 => {
                        format!("ValueError: Unknown conversion specifier {c}")
                    }
                    Some(_) => "ValueError: expected ':' after conversion specifier".into(),
                });
            }
        };
        out.push_str(&rendered);
    }
    Ok(out)
}

fn arg_to_string(arg: &FormatArg) -> String {
    match arg {
        FormatArg::Int(n) => n.to_string(),
        FormatArg::Str(s) => s.to_string(),
    }
}

fn arg_repr(arg: &FormatArg) -> String {
    match arg {
        FormatArg::Int(n) => n.to_string(),
        FormatArg::Str(s) => repr_str(s),
    }
}

struct Spec {
    fill: char,
    align: Option<char>,
    sign: char,
    alternate: bool,
    zero: bool,
    width: usize,
    grouping: Option<char>,
    precision: Option<usize>,
    kind: Option<char>,
}

fn parse_spec(spec: &str) -> Result<Spec, String> {
    let chars: Vec<char> = spec.chars().collect();
    let mut i = 0;
    let mut out = Spec {
        fill: ' ',
        align: None,
        sign: '-',
        alternate: false,
        zero: false,
        width: 0,
        grouping: None,
        precision: None,
        kind: None,
    };
    let is_align = |c: char| matches!(c, '<' | '>' | '^' | '=');
    if chars.len() >= 2 && is_align(chars[1]) {
        out.fill = chars[0];
        out.align = Some(chars[1]);
        i = 2;
    } else if !chars.is_empty() && is_align(chars[0]) {
        out.align = Some(chars[0]);
        i = 1;
    }
    if i < chars.len() && matches!(chars[i], '+' | '-' | ' ') {
        out.sign = chars[i];
        i += 1;
    }
    if i < chars.len() && chars[i] == '#' {
        out.alternate = true;
        i += 1;
    }
    if i < chars.len() && chars[i] == '0' {
        out.zero = true;
        i += 1;
    }
    let start = i;
    while i < chars.len() && chars[i].is_ascii_digit() {
        i += 1;
    }
    if i > start {
        out.width = chars[start..i]
            .iter()
            .collect::<String>()
            .parse()
            .unwrap_or(usize::MAX);
    }
    if i < chars.len() && matches!(chars[i], ',' | '_') {
        out.grouping = Some(chars[i]);
        i += 1;
    }
    if i < chars.len() && chars[i] == '.' {
        i += 1;
        let start = i;
        while i < chars.len() && chars[i].is_ascii_digit() {
            i += 1;
        }
        if i == start {
            return Err("ValueError: Format specifier missing precision".into());
        }
        out.precision = Some(
            chars[start..i]
                .iter()
                .collect::<String>()
                .parse()
                .unwrap_or(usize::MAX),
        );
    }
    if i < chars.len() {
        out.kind = Some(chars[i]);
        i += 1;
    }
    if i < chars.len() {
        return Err("ValueError: Invalid format specifier".into());
    }
    Ok(out)
}

fn pad(body: &str, sign: &str, spec: &Spec, default_align: char) -> String {
    let len = body.chars().count() + sign.chars().count();
    let align = spec
        .align
        .unwrap_or(if spec.zero { '=' } else { default_align });
    let fill = if spec.align.is_none() && spec.zero {
        '0'
    } else {
        spec.fill
    };
    if len >= spec.width {
        return format!("{sign}{body}");
    }
    let n = spec.width - len;
    let fills = |k: usize| std::iter::repeat_n(fill, k).collect::<String>();
    match align {
        '<' => format!("{sign}{body}{}", fills(n)),
        '^' => format!("{}{sign}{body}{}", fills(n / 2), fills(n - n / 2)),
        '=' => format!("{sign}{}{body}", fills(n)),
        _ => format!("{}{sign}{body}", fills(n)),
    }
}

fn group_digits(digits: &str, sep: char, every: usize) -> String {
    let chars: Vec<char> = digits.chars().collect();
    let mut out = String::new();
    for (k, c) in chars.iter().enumerate() {
        if k > 0 && (chars.len() - k) % every == 0 {
            out.push(sep);
        }
        out.push(*c);
    }
    out
}

fn format_value(arg: &FormatArg, spec: &str) -> Result<String, String> {
    match arg {
        FormatArg::Str(s) => format_str(s, spec),
        FormatArg::Int(n) => {
            let sp = parse_spec(spec)?;
            if sp.precision.is_some()
                && !matches!(sp.kind, Some('e' | 'E' | 'f' | 'F' | 'g' | 'G' | '%'))
            {
                return Err("ValueError: Precision not allowed in integer format specifier".into());
            }
            let magnitude = n.unsigned_abs();
            let (digits, prefix) = match sp.kind {
                None | Some('d') | Some('n') => (magnitude.to_string(), ""),
                Some('x') => (format!("{magnitude:x}"), "0x"),
                Some('X') => (format!("{magnitude:X}"), "0X"),
                Some('o') => (format!("{magnitude:o}"), "0o"),
                Some('b') => (format!("{magnitude:b}"), "0b"),
                Some('c') => {
                    let c = char::from_u32(*n as u32)
                        .ok_or("OverflowError: %c arg not in range(0x110000)")?;
                    return Ok(pad(&c.to_string(), "", &sp, '<'));
                }
                Some(k) => {
                    return Err(format!(
                        "ValueError: Unknown format code '{k}' for object of type 'int'"
                    ))
                }
            };
            let digits = match sp.grouping {
                Some(g) if matches!(sp.kind, None | Some('d')) => group_digits(&digits, g, 3),
                Some('_') => group_digits(&digits, '_', 4),
                Some(g) => {
                    return Err(format!(
                        "ValueError: Cannot specify '{g}' with '{}'.",
                        sp.kind.unwrap_or('d')
                    ))
                }
                None => digits,
            };
            let mut sign = String::new();
            if *n < 0 {
                sign.push('-');
            } else if sp.sign == '+' {
                sign.push('+');
            } else if sp.sign == ' ' {
                sign.push(' ');
            }
            if sp.alternate {
                sign.push_str(prefix);
            }
            Ok(pad(&digits, &sign, &sp, '>'))
        }
    }
}

fn format_str(s: &str, spec: &str) -> Result<String, String> {
    let sp = parse_spec(spec)?;
    if let Some(k) = sp.kind {
        if k != 's' {
            return Err(format!(
                "ValueError: Unknown format code '{k}' for object of type 'str'"
            ));
        }
    }
    if sp.sign != '-' {
        return Err("ValueError: Sign not allowed in string format specifier".into());
    }
    if sp.alternate {
        return Err("ValueError: Alternate form (#) not allowed in string format specifier".into());
    }
    if sp.align == Some('=') {
        return Err("ValueError: '=' alignment not allowed in string format specifier".into());
    }
    if sp.grouping.is_some() {
        return Err("ValueError: Cannot specify ',' with 's'.".into());
    }
    let body: String = match sp.precision {
        Some(p) => s.chars().take(p).collect(),
        None => s.to_string(),
    };
    let sp = Spec { zero: false, ..sp };
    Ok(pad(&body, "", &sp, '<'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn whitespace_matches_python() {
        assert!(is_space('\x1c'));
        assert!(is_space('\u{3000}'));
        assert!(!is_space('\u{200b}'));
        assert_eq!(strip("\u{a0} a \x1f"), "a");
        assert_eq!(split_whitespace(" a\u{2003}b  c "), vec!["a", "b", "c"]);
    }

    #[test]
    fn lower_uses_full_mappings() {
        assert_eq!(lower("ABC"), "abc");
        assert_eq!(lower("\u{130}"), "i\u{307}");
        assert_eq!(capitalize("note here"), "Note here");
    }

    #[test]
    fn expandtabs_counts_columns() {
        assert_eq!(expandtabs("a\tb", 4), "a   b");
        assert_eq!(expandtabs("ab\n\tc", 4), "ab\n    c");
    }

    #[test]
    fn parse_int_follows_python() {
        assert_eq!(parse_int(" 12 "), Ok(12));
        assert_eq!(parse_int("1_000"), Ok(1000));
        assert_eq!(parse_int("\u{663}"), Ok(3));
        assert_eq!(
            parse_int("1__0"),
            Err("invalid literal for int() with base 10: '1__0'".into())
        );
    }

    #[test]
    fn format_fields() {
        assert_eq!(format("{}", &[FormatArg::Int(3)]).unwrap(), "3");
        assert_eq!(format("[{0:>3}]", &[FormatArg::Int(3)]).unwrap(), "[  3]");
        assert_eq!(format("{{{}}}", &[FormatArg::Int(3)]).unwrap(), "{3}");
        assert_eq!(format("{:03d}", &[FormatArg::Int(7)]).unwrap(), "007");
        assert!(format("{1}", &[FormatArg::Int(3)])
            .unwrap_err()
            .starts_with("IndexError"));
    }

    #[test]
    fn repr_quotes() {
        assert_eq!(repr_str("a'b"), "\"a'b\"");
        assert_eq!(repr_str("a\nb"), "'a\\nb'");
    }
}
