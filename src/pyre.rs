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

//! Python `re` semantics on top of `fancy_regex`.
//!
//! Python-Markdown's behaviour is defined by its regular expressions, which use Python's dialect: Unicode
//! `\w`, `\s`, `\d` and `\b` as `str.isalnum()`/`str.isspace()` define them, `$` matching before a final
//! newline, verbose patterns, look-behind, back-references and possessive quantifiers. [`Regex::new`] translates
//! such a pattern into `fancy_regex` syntax with the same meaning, and [`Regex`] offers the `match`, `search`,
//! `finditer`, `sub` and `split` operations with Python's rules for positions and empty matches.

use std::sync::{Arc, OnceLock};

use crate::unicode_tables::{DIGIT, SPACE, WORD};

/// `re.IGNORECASE`.
pub const I: u32 = 1;
/// `re.MULTILINE`.
pub const M: u32 = 2;
/// `re.DOTALL`.
pub const S: u32 = 4;
/// `re.VERBOSE`.
pub const X: u32 = 8;

fn class_body(table: &[(u32, u32)]) -> String {
    let mut out = String::new();
    for &(lo, hi) in table {
        if lo == hi {
            out.push_str(&format!("\\x{{{lo:X}}}"));
        } else {
            out.push_str(&format!("\\x{{{lo:X}}}-\\x{{{hi:X}}}"));
        }
    }
    out
}

fn word_body() -> &'static str {
    static BODY: OnceLock<String> = OnceLock::new();
    BODY.get_or_init(|| class_body(WORD))
}

fn space_body() -> &'static str {
    static BODY: OnceLock<String> = OnceLock::new();
    BODY.get_or_init(|| class_body(SPACE))
}

fn digit_body() -> &'static str {
    static BODY: OnceLock<String> = OnceLock::new();
    BODY.get_or_init(|| class_body(DIGIT))
}

fn class_for(e: char) -> Option<(bool, &'static str)> {
    match e {
        'w' => Some((false, word_body())),
        'W' => Some((true, word_body())),
        's' => Some((false, space_body())),
        'S' => Some((true, space_body())),
        'd' => Some((false, digit_body())),
        'D' => Some((true, digit_body())),
        _ => None,
    }
}

/// Translate a Python pattern (with `re` flags) into an equivalent `fancy_regex` pattern.
pub fn translate(pattern: &str, flags: u32) -> String {
    let (lead, body) = translate_parts(pattern, flags);
    format!("{lead}{body}")
}

/// [`translate`], split into the inline-flag group (possibly empty) and the pattern body.
fn translate_parts(pattern: &str, mut flags: u32) -> (String, String) {
    let mut chars: Vec<char> = pattern.chars().collect();
    // Global inline flags at the start of the pattern, e.g. `(?x)`.
    loop {
        if chars.len() >= 3 && chars[0] == '(' && chars[1] == '?' {
            let mut j = 2;
            let mut f = 0;
            while j < chars.len() && "aiLmsux".contains(chars[j]) {
                f |= match chars[j] {
                    'i' => I,
                    'm' => M,
                    's' => S,
                    'x' => X,
                    _ => 0,
                };
                j += 1;
            }
            if j > 2 && j < chars.len() && chars[j] == ')' {
                flags |= f;
                chars.drain(..=j);
                continue;
            }
        }
        break;
    }
    let verbose = flags & X != 0;
    let multiline = flags & M != 0;
    let mut out = String::new();
    let mut prefix = String::new();
    if flags & I != 0 {
        prefix.push('i');
    }
    if multiline {
        prefix.push('m');
    }
    if flags & S != 0 {
        prefix.push('s');
    }
    let lead = if prefix.is_empty() {
        String::new()
    } else {
        format!("(?{prefix})")
    };
    let n = chars.len();
    let mut i = 0;
    while i < n {
        let c = chars[i];
        match c {
            '\\' if i + 1 < n => {
                let e = chars[i + 1];
                i += 2;
                if let Some((negated, body)) = class_for(e) {
                    out.push_str(if negated { "[^" } else { "[" });
                    out.push_str(body);
                    out.push(']');
                } else {
                    match e {
                        'b' => out.push_str(&format!(
                            "(?:(?<=[{w}])(?![{w}])|(?<![{w}])(?=[{w}]))",
                            w = word_body()
                        )),
                        'B' => out.push_str(&format!(
                            "(?:(?<=[{w}])(?=[{w}])|(?<![{w}])(?![{w}]))",
                            w = word_body()
                        )),
                        'Z' => out.push_str("\\z"),
                        _ => push_escape(&mut out, e),
                    }
                }
            }
            '[' => {
                i = translate_class(&chars, i, &mut out);
            }
            '$' => {
                out.push_str(if multiline { "$" } else { "\\Z" });
                i += 1;
            }
            '#' if verbose => {
                while i < n && chars[i] != '\n' {
                    i += 1;
                }
            }
            ' ' | '\t' | '\n' | '\r' | '\x0b' | '\x0c' if verbose => {
                i += 1;
            }
            '(' if chars.get(i + 1) == Some(&'?') && chars.get(i + 2) == Some(&'#') => {
                while i < n && chars[i] != ')' {
                    i += 1;
                }
                i += 1;
            }
            _ => {
                out.push(c);
                i += 1;
            }
        }
    }
    (lead, out)
}

/// An escaped character other than a class shorthand. Python treats an escaped ASCII punctuation character
/// (or space) as that literal character, while `fancy_regex` gives some of them a meaning (`\<`, `\>`), so
/// those are written as hexadecimal escapes.
fn push_escape(out: &mut String, e: char) {
    if e.is_ascii() && !e.is_ascii_alphanumeric() {
        out.push_str(&format!("\\x{{{:X}}}", e as u32));
    } else {
        out.push('\\');
        out.push(e);
    }
}

/// Copy the character class starting at `chars[start] == '['` into `out`; returns the index after it.
fn translate_class(chars: &[char], start: usize, out: &mut String) -> usize {
    let n = chars.len();
    let mut i = start + 1;
    out.push('[');
    if i < n && chars[i] == '^' {
        out.push('^');
        i += 1;
    }
    // A `]` right after the opening (or `^`) is a literal in Python.
    if i < n && chars[i] == ']' {
        out.push_str("\\]");
        i += 1;
    }
    let mut negated_parts: Vec<&'static str> = Vec::new();
    while i < n && chars[i] != ']' {
        let c = chars[i];
        if c == '\\' && i + 1 < n {
            let e = chars[i + 1];
            i += 2;
            match class_for(e) {
                Some((false, body)) => out.push_str(body),
                Some((true, body)) => negated_parts.push(body),
                None => match e {
                    'b' => out.push_str("\\x08"),
                    _ => push_escape(out, e),
                },
            }
            continue;
        }
        match c {
            '[' | '&' | '~' => {
                out.push('\\');
                out.push(c);
            }
            _ => out.push(c),
        }
        i += 1;
    }
    for body in negated_parts {
        out.push_str("[^");
        out.push_str(body);
        out.push(']');
    }
    out.push(']');
    i + 1
}

/// A compiled Python regular expression.
pub struct Regex {
    search: fancy_regex::Regex,
    anchored: fancy_regex::Regex,
    names: Arc<[Option<String>]>,
}

fn build(pattern: &str) -> fancy_regex::Regex {
    fancy_regex::RegexBuilder::new(pattern)
        .backtrack_limit(1_000_000_000)
        .build()
        .unwrap_or_else(|e| panic!("invalid translated pattern {pattern:?}: {e}"))
}

impl Regex {
    /// Compile a Python pattern with the given `re` flags.
    pub fn new(pattern: &str, flags: u32) -> Regex {
        // Inline flags must stay at the very start of the anchored form.
        let (lead, body) = translate_parts(pattern, flags);
        let search = build(&format!("{lead}{body}"));
        let anchored = build(&format!("{lead}\\G(?:{body})"));
        let names: Arc<[Option<String>]> = search
            .capture_names()
            .map(|n| n.map(str::to_string))
            .collect();
        Regex {
            search,
            anchored,
            names,
        }
    }

    /// `pattern.search(text, pos)`.
    pub fn search<'t>(&self, text: &'t str, pos: usize) -> Option<Match<'t>> {
        let caps = self.search.captures_from_pos(text, pos).ok().flatten()?;
        Some(Match {
            caps,
            names: self.names.clone(),
        })
    }

    /// `pattern.match(text, pos)`: a match that starts exactly at `pos`.
    pub fn match_at<'t>(&self, text: &'t str, pos: usize) -> Option<Match<'t>> {
        let caps = self.anchored.captures_from_pos(text, pos).ok().flatten()?;
        Some(Match {
            caps,
            names: self.names.clone(),
        })
    }

    /// `bool(pattern.search(text))`.
    pub fn is_match(&self, text: &str) -> bool {
        self.search.is_match(text).unwrap_or(false)
    }

    /// `pattern.finditer(text, pos)`, with Python's handling of empty matches.
    pub fn finditer<'r, 't>(&'r self, text: &'t str, pos: usize) -> FindIter<'r, 't> {
        FindIter {
            re: self,
            text,
            at: pos,
            last_empty_at: None,
        }
    }

    /// `pattern.sub(repl, text)` with a function as the replacement.
    pub fn sub_fn(&self, text: &str, mut repl: impl FnMut(&Match) -> String) -> String {
        let mut out = String::with_capacity(text.len());
        let mut last = 0;
        for m in self.finditer(text, 0) {
            out.push_str(&text[last..m.start(0)]);
            out.push_str(&repl(&m));
            last = m.end(0);
        }
        out.push_str(&text[last..]);
        out
    }

    /// `pattern.sub(repl, text)` with a literal replacement string (no group references).
    pub fn sub(&self, text: &str, repl: &str) -> String {
        self.sub_fn(text, |_| repl.to_string())
    }

    /// `pattern.split(text, maxsplit)`: captured groups are included; `None` for groups that did not take part.
    pub fn split<'t>(&self, text: &'t str, maxsplit: usize) -> Vec<Option<&'t str>> {
        let mut out = Vec::new();
        let mut last = 0;
        for (count, m) in self.finditer(text, 0).enumerate() {
            if maxsplit != 0 && count >= maxsplit {
                break;
            }
            out.push(Some(&text[last..m.start(0)]));
            for g in 1..m.len() {
                out.push(m.group(g));
            }
            last = m.end(0);
        }
        out.push(Some(&text[last..]));
        out
    }
}

/// The iterator of [`Regex::finditer`].
pub struct FindIter<'r, 't> {
    re: &'r Regex,
    text: &'t str,
    at: usize,
    last_empty_at: Option<usize>,
}

impl<'t> Iterator for FindIter<'_, 't> {
    type Item = Match<'t>;

    fn next(&mut self) -> Option<Match<'t>> {
        while self.at <= self.text.len() {
            let m = self.re.search(self.text, self.at)?;
            let (s, e) = (m.start(0), m.end(0));
            if s == e && self.last_empty_at == Some(s) {
                // An empty match where the previous empty one was: Python moves on by one character.
                let c = self.text[s..].chars().next()?;
                self.at = s + c.len_utf8();
                continue;
            }
            self.last_empty_at = if s == e { Some(e) } else { None };
            self.at = e;
            return Some(m);
        }
        None
    }
}

/// A match object.
pub struct Match<'t> {
    caps: fancy_regex::Captures<'t, str>,
    names: Arc<[Option<String>]>,
}

impl<'t> Match<'t> {
    /// `m.group(i)`.
    pub fn group(&self, i: usize) -> Option<&'t str> {
        self.caps.get(i).map(|m| m.as_str())
    }

    /// `m.group(name)`.
    pub fn name(&self, name: &str) -> Option<&'t str> {
        self.caps.name(name).map(|m| m.as_str())
    }

    /// `m.group(i)`, or the empty string when the group did not take part (`m.group(i) or ''`).
    pub fn str(&self, i: usize) -> &'t str {
        self.group(i).unwrap_or("")
    }

    /// `m.start(i)`; panics for a group that did not take part.
    pub fn start(&self, i: usize) -> usize {
        self.caps
            .get(i)
            .map(|m| m.start())
            .expect("group did not participate")
    }

    /// `m.end(i)`; panics for a group that did not take part.
    pub fn end(&self, i: usize) -> usize {
        self.caps
            .get(i)
            .map(|m| m.end())
            .expect("group did not participate")
    }

    /// The span of a named group, if it took part.
    pub fn name_span(&self, name: &str) -> Option<(usize, usize)> {
        self.caps.name(name).map(|m| (m.start(), m.end()))
    }

    /// The number of groups plus one (`len(m.groups()) + 1`).
    pub fn len(&self) -> usize {
        self.caps.len()
    }

    /// Whether the match has no groups beyond the whole match.
    pub fn is_empty(&self) -> bool {
        self.caps.len() <= 1
    }

    /// `m.lastgroup`: the name of the last group that took part.
    pub fn lastgroup(&self) -> Option<&str> {
        (1..self.caps.len())
            .rev()
            .find(|&i| self.caps.get(i).is_some() && self.names.get(i).is_some_and(|n| n.is_some()))
            .and_then(|i| self.names[i].as_deref())
    }
}

/// `re.escape(text)`.
pub fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if c.is_ascii() && !c.is_ascii_alphanumeric() && c != '_' {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dollar_matches_before_final_newline() {
        let re = Regex::new(r"a$", 0);
        assert!(re.is_match("a\n"));
        assert!(!re.is_match("a\nb"));
    }

    #[test]
    fn word_class_is_python_isalnum() {
        let re = Regex::new(r"^\w+$", 0);
        assert!(re.is_match("a²_"));
        assert!(!re.is_match("e\u{301}"));
    }

    #[test]
    fn space_class_includes_separators() {
        assert!(Regex::new(r"\s", 0).is_match("\x1c"));
        assert!(Regex::new(r"[^\s]", 0).search("\x1c x", 0).is_some());
    }

    #[test]
    fn match_is_anchored_at_pos() {
        let re = Regex::new(r"b+", 0);
        assert!(re.match_at("abb", 0).is_none());
        assert_eq!(re.match_at("abb", 1).map(|m| m.end(0)), Some(3));
    }

    #[test]
    fn verbose_patterns() {
        let re = Regex::new("(?x) a [ ]b # comment\n c", 0);
        assert!(re.is_match("a bc"));
    }

    #[test]
    fn split_with_groups() {
        let re = Regex::new(r"(-)", 0);
        assert_eq!(re.split("a-b", 0), vec![Some("a"), Some("-"), Some("b")]);
    }
}
