// Smarty extension for Python-Markdown
// ====================================
//
// Adds conversion of ASCII dashes, quotes and ellipses to their HTML
// entity equivalents.
//
// See https://Python-Markdown.github.io/extensions/smarty
// for documentation.
//
// Author: 2013, Dmitry Shachnev <mitya57@gmail.com>
//
// All changes Copyright 2013-2014 The Python Markdown Project
//
// License: [BSD](https://opensource.org/licenses/bsd-license.php)
//
// SmartyPants license:
//
//    Copyright (c) 2003 John Gruber <https://daringfireball.net/>
//    All rights reserved.
//
//    Redistribution and use in source and binary forms, with or without
//    modification, are permitted provided that the following conditions are
//    met:
//
//    *  Redistributions of source code must retain the above copyright
//       notice, this list of conditions and the following disclaimer.
//
//    *  Redistributions in binary form must reproduce the above copyright
//       notice, this list of conditions and the following disclaimer in
//       the documentation and/or other materials provided with the
//       distribution.
//
//    *  Neither the name "SmartyPants" nor the names of its contributors
//       may be used to endorse or promote products derived from this
//       software without specific prior written permission.
//
//    This software is provided by the copyright holders and contributors "as
//    is" and any express or implied warranties, including, but not limited
//    to, the implied warranties of merchantability and fitness for a
//    particular purpose are disclaimed. In no event shall the copyright
//    owner or contributors be liable for any direct, indirect, incidental,
//    special, exemplary, or consequential damages (including, but not
//    limited to, procurement of substitute goods or services; loss of use,
//    data, or profits; or business interruption) however caused and on any
//    theory of liability, whether in contract, strict liability, or tort
//    (including negligence or otherwise) arising in any way out of the use
//    of this software, even if advised of the possibility of such damage.
//
//
// `smartypants.py` license:
//
//    `smartypants.py` is a derivative work of SmartyPants.
//    Copyright (c) 2004, 2007 Chad Miller <http://web.chad.org/>
//
//    Redistribution and use in source and binary forms, with or without
//    modification, are permitted provided that the following conditions are
//    met:
//
//    *  Redistributions of source code must retain the above copyright
//       notice, this list of conditions and the following disclaimer.
//
//    *  Redistributions in binary form must reproduce the above copyright
//       notice, this list of conditions and the following disclaimer in
//       the documentation and/or other materials provided with the
//       distribution.
//
//    This software is provided by the copyright holders and contributors "as
//    is" and any express or implied warranties, including, but not limited
//    to, the implied warranties of merchantability and fitness for a
//    particular purpose are disclaimed. In no event shall the copyright
//    owner or contributors be liable for any direct, indirect, incidental,
//    special, exemplary, or consequential damages (including, but not
//    limited to, procurement of substitute goods or services; loss of use,
//    data, or profits; or business interruption) however caused and on any
//    theory of liability, whether in contract, strict liability, or tort
//    (including negligence or otherwise) arising in any way out of the use
//    of this software, even if advised of the possibility of such damage.

//! Smarty: ASCII dashes, quotes and ellipses become their typographic HTML entities.

use std::any::Any;
use std::rc::Rc;

use super::Config;
use crate::core::{Extension, InlinePattern, InlineResult, Markdown, PyError, PyResult};
use crate::inlinepatterns::{HtmlInlineProcessor, InlineNode, HTML_RE};
use crate::pyre::{Match, Regex, S};
use crate::treeprocessors::InlineProcessor;
use crate::util::Registry;
use crate::value::Value;

const PUNCT_CLASS: &str = r##"[!"#\$\%'()*+,-.\/:;<=>?\@\[\\\]\^_`{|}~]"##;
const CLOSE_CLASS: &str = r"[^\ \t\r\n\[\{\(\-\u0002\u0003]";
const OPENING_QUOTES_BASE: &str = concat!(
    r"(\s",               // a  whitespace char
    r"|&nbsp;",           // or a non-breaking space entity
    r"|--",               // or dashes
    "|\u{2013}|\u{2014}", // or Unicode
    r"|&[mn]dash;",       // or named dash entities
    r"|&#8211;|&#8212;",  // or decimal entities
    r")"
);

/// The default substitutions, by name.
const SUBSTITUTIONS: [(&str, &str); 9] = [
    ("mdash", "&mdash;"),
    ("ndash", "&ndash;"),
    ("ellipsis", "&hellip;"),
    ("left-angle-quote", "&laquo;"),
    ("right-angle-quote", "&raquo;"),
    ("left-single-quote", "&lsquo;"),
    ("right-single-quote", "&rsquo;"),
    ("left-double-quote", "&ldquo;"),
    ("right-double-quote", "&rdquo;"),
];

pub struct SmartyExtension {
    config: Config,
    substitutions: Vec<(String, Value)>,
}

impl SmartyExtension {
    pub fn create(configs: &[(String, Value)]) -> PyResult<Rc<dyn Extension>> {
        let mut config = Config::new(&[
            ("smart_quotes", Value::Bool(true)),
            ("smart_angled_quotes", Value::Bool(false)),
            ("smart_dashes", Value::Bool(true)),
            ("smart_ellipses", Value::Bool(true)),
            ("substitutions", Value::Dict(Vec::new())),
        ]);
        config.set_all(configs)?;
        let mut substitutions: Vec<(String, Value)> = SUBSTITUTIONS
            .iter()
            .map(|(k, v)| (k.to_string(), Value::Str(v.to_string())))
            .collect();
        // `dict.update(substitutions)`.
        let updates: Vec<(String, Value)> = match config.get("substitutions") {
            Value::Dict(items) => items.clone(),
            Value::List(items) => {
                let mut out = Vec::new();
                for (i, item) in items.iter().enumerate() {
                    match item {
                        Value::List(pair) if pair.len() == 2 => out.push((pair[0].py_str(), pair[1].clone())),
                        Value::List(pair) => {
                            return Err(PyError::new(
                                "ValueError",
                                format!("dictionary update sequence element #{i} has length {}; 2 is required", pair.len()),
                            ))
                        }
                        _ => {
                            return Err(PyError::new(
                                "TypeError",
                                format!("cannot convert dictionary update sequence element #{i} to a sequence"),
                            ))
                        }
                    }
                }
                out
            }
            Value::Str(s) if s.is_empty() => Vec::new(),
            Value::Str(_) => {
                return Err(PyError::new(
                    "ValueError",
                    "dictionary update sequence element #0 has length 1; 2 is required",
                ))
            }
            other => {
                return Err(PyError::new(
                    "TypeError",
                    format!("'{}' object is not iterable", other.type_name()),
                ))
            }
        };
        for (k, v) in updates {
            match substitutions.iter_mut().find(|(key, _)| *key == k) {
                Some(slot) => slot.1 = v,
                None => substitutions.push((k, v)),
            }
        }
        Ok(Rc::new(SmartyExtension {
            config,
            substitutions,
        }))
    }

    fn sub(&self, name: &str) -> PyResult<Value> {
        self.substitutions
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.clone())
            .ok_or_else(|| PyError::key_error(name))
    }
}

/// One replacement part: a group of the match, or text to stash as raw HTML.
#[derive(Clone)]
enum Part {
    Group(usize),
    Html(Value),
}

/// Replace a match with the given parts.
struct SubstituteTextPattern {
    re: Rc<Regex>,
    replace: Vec<Part>,
}

impl SubstituteTextPattern {
    fn new(pattern: &str, replace: Vec<Part>) -> SubstituteTextPattern {
        SubstituteTextPattern {
            re: Rc::new(Regex::new(pattern, S)),
            replace,
        }
    }
}

impl InlinePattern for SubstituteTextPattern {
    fn regex(&self) -> Rc<Regex> {
        self.re.clone()
    }

    fn handle_match(&self, md: &mut Markdown, m: &Match, _data: &str) -> PyResult<InlineResult> {
        let mut result = String::new();
        for part in &self.replace {
            match part {
                Part::Group(g) => result.push_str(m.str(*g)),
                Part::Html(v) => result.push_str(&md.html_stash.store_str(v.py_str())),
            }
        }
        Ok((
            Some(InlineNode::Str(result)),
            Some(m.start(0)),
            Some(m.end(0)),
        ))
    }
}

impl Extension for SmartyExtension {
    fn extend_markdown(&self, md: &mut Markdown, _this: Rc<dyn Extension>) -> PyResult<()> {
        let mut patterns: Registry<Rc<dyn InlinePattern>> = Registry::new();
        let html = |v: Value| Part::Html(v);
        if self.config.get("smart_ellipses").truthy() {
            let p = SubstituteTextPattern::new(
                r"(?<!\.)\.{3}(?!\.)",
                vec![html(self.sub("ellipsis")?)],
            );
            patterns.register(Rc::new(p), "smarty-ellipses", 10.0);
        }
        if self.config.get("smart_quotes").truthy() {
            let lsquo = self.sub("left-single-quote")?;
            let rsquo = self.sub("right-single-quote")?;
            let ldquo = self.sub("left-double-quote")?;
            let rdquo = self.sub("right-double-quote")?;
            let concat = |a: &Value, b: &Value| -> PyResult<Value> {
                match (a, b) {
                    (Value::Str(a), Value::Str(b)) => Ok(Value::Str(format!("{a}{b}"))),
                    (Value::Str(_), other) | (other, _) => Err(PyError::new(
                        "TypeError",
                        format!(
                            "can only concatenate str (not \"{}\") to str",
                            other.type_name()
                        ),
                    )),
                }
            };
            let quote_patterns: Vec<(String, Vec<Part>)> = vec![
                (format!(r"^'(?={PUNCT_CLASS}\B)"), vec![html(rsquo.clone())]),
                (
                    format!(r#"^"(?={PUNCT_CLASS}\B)"#),
                    vec![html(rdquo.clone())],
                ),
                (
                    r#""'(?=\w)"#.to_string(),
                    vec![html(concat(&ldquo, &lsquo)?)],
                ),
                (
                    r#"'"(?=\w)"#.to_string(),
                    vec![html(concat(&lsquo, &ldquo)?)],
                ),
                (
                    format!(r#"(?<={CLOSE_CLASS})\'""#),
                    vec![html(concat(&rsquo, &rdquo)?)],
                ),
                (
                    format!(r#"(?<={CLOSE_CLASS})\"'"#),
                    vec![html(concat(&rdquo, &rsquo)?)],
                ),
                (r"(?<!\w)'(?=\d{2}s)".to_string(), vec![html(rsquo.clone())]),
                (
                    format!(r"{OPENING_QUOTES_BASE}'(?=\w)"),
                    vec![Part::Group(1), html(lsquo.clone())],
                ),
                (
                    format!(r"(?<={CLOSE_CLASS})'(?!\s|s\b|\d)"),
                    vec![html(rsquo.clone())],
                ),
                (
                    r"'(\s|s\b)".to_string(),
                    vec![html(rsquo.clone()), Part::Group(1)],
                ),
                ("'".to_string(), vec![html(lsquo.clone())]),
                (
                    format!(r#"{OPENING_QUOTES_BASE}"(?=\w)"#),
                    vec![Part::Group(1), html(ldquo.clone())],
                ),
                (r#""(?=\s)"#.to_string(), vec![html(rdquo.clone())]),
                (format!(r#"(?<={CLOSE_CLASS})""#), vec![html(rdquo.clone())]),
                ("\"".to_string(), vec![html(ldquo.clone())]),
            ];
            for (ind, (pattern, replace)) in quote_patterns.into_iter().enumerate() {
                let p = SubstituteTextPattern::new(&pattern, replace);
                patterns.register(
                    Rc::new(p),
                    &format!("smarty-quotes-{ind}"),
                    30.0 - ind as f64,
                );
            }
        }
        if self.config.get("smart_angled_quotes").truthy() {
            let left =
                SubstituteTextPattern::new(r"\<\<", vec![html(self.sub("left-angle-quote")?)]);
            let right =
                SubstituteTextPattern::new(r"\>\>", vec![html(self.sub("right-angle-quote")?)]);
            patterns.register(Rc::new(left), "smarty-left-angle-quotes", 40.0);
            patterns.register(Rc::new(right), "smarty-right-angle-quotes", 35.0);
            md.inline_patterns.register(
                Rc::new(HtmlInlineProcessor::new(&format!(r"{HTML_RE}(?!\>)"))),
                "html",
                90.0,
            );
        }
        if self.config.get("smart_dashes").truthy() {
            let em = SubstituteTextPattern::new(r"(?<!-)---(?!-)", vec![html(self.sub("mdash")?)]);
            let en = SubstituteTextPattern::new(r"(?<!-)--(?!-)", vec![html(self.sub("ndash")?)]);
            patterns.register(Rc::new(em), "smarty-em-dashes", 50.0);
            patterns.register(Rc::new(en), "smarty-en-dashes", 45.0);
        }
        md.treeprocessors
            .register(Rc::new(InlineProcessor::new(Some(patterns))), "smarty", 6.0);
        md.escaped_chars.extend(['"', '\'']);
        Ok(())
    }

    fn class_path(&self) -> &'static str {
        "markdown.extensions.smarty.SmartyExtension"
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}
