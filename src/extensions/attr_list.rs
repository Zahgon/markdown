// Attribute List Extension for Python-Markdown
// ============================================
//
// Adds attribute list syntax. Inspired by
// [Maruku](http://maruku.rubyforge.org/proposal.html#attribute_lists)'s
// feature of the same name.
//
// See https://Python-Markdown.github.io/extensions/attr_list
// for documentation.
//
// Original code Copyright 2011 [Waylan Limberg](https://github.com/waylan).
//
// All changes Copyright 2011-2014 The Python Markdown Project
//
// License: [BSD](https://opensource.org/licenses/bsd-license.php)

//! Attribute lists: `{: #id .class key=value }` after a block, header or inline element sets its attributes.

use std::any::Any;
use std::rc::Rc;
use std::sync::OnceLock;

use super::Config;
use crate::core::{Extension, Markdown, PyResult, Treeprocessor};
use crate::etree::El;
use crate::pyre::Regex;
use crate::value::Value;

pub struct AttrListExtension;

impl AttrListExtension {
    pub fn create(configs: &[(String, Value)]) -> PyResult<Rc<dyn Extension>> {
        Config::default().set_all(configs)?;
        Ok(Rc::new(AttrListExtension))
    }
}

impl Extension for AttrListExtension {
    fn extend_markdown(&self, md: &mut Markdown, this: Rc<dyn Extension>) -> PyResult<()> {
        md.treeprocessors
            .register(Rc::new(AttrListTreeprocessor), "attr_list", 8.0);
        md.register_extension(this);
        Ok(())
    }

    fn class_path(&self) -> &'static str {
        "markdown.extensions.attr_list.AttrListExtension"
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

fn scanner() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r#"([^ =}]+=".*?")|([^ =}]+='.*?')|([^ =}]+=[^ =}]+)|([^ =}]+)|( )"#,
            0,
        )
    })
}

/// Scan an attribute list (the `re.Scanner` of Python-Markdown): the attributes, and the rest of the string
/// from the first `}` after the point where scanning stopped (empty if there is none).
pub fn get_attrs_and_remainder(attrs_string: &str) -> (Vec<(String, String)>, String) {
    let mut attrs = Vec::new();
    let mut i = 0;
    while let Some(m) = scanner().match_at(attrs_string, i) {
        let j = m.end(0);
        if j == i {
            break;
        }
        let token = m.str(0);
        if m.group(1).is_some() {
            let (k, v) = token.split_once('=').unwrap_or((token, ""));
            attrs.push((
                k.to_string(),
                crate::pystr::strip_chars(v, "\"").to_string(),
            ));
        } else if m.group(2).is_some() {
            let (k, v) = token.split_once('=').unwrap_or((token, ""));
            attrs.push((k.to_string(), crate::pystr::strip_chars(v, "'").to_string()));
        } else if m.group(3).is_some() {
            let (k, v) = token.split_once('=').unwrap_or((token, ""));
            attrs.push((k.to_string(), v.to_string()));
        } else if m.group(4).is_some() {
            attrs.push(if let Some(rest) = token.strip_prefix('.') {
                (".".to_string(), rest.to_string())
            } else if let Some(rest) = token.strip_prefix('#') {
                ("id".to_string(), rest.to_string())
            } else {
                (token.to_string(), token.to_string())
            });
        }
        i = j;
    }
    let remainder = &attrs_string[i..];
    let remainder = match remainder.find('}') {
        Some(index) => remainder[index..].to_string(),
        None => String::new(),
    };
    (attrs, remainder)
}

/// Replace characters that are not allowed in an XML attribute name with `_`.
pub fn sanitize_name(name: &str) -> String {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        Regex::new(
            concat!(
                r"[^A-Z_a-zÀ-ÖØ-öø-˿",
                r"Ͱ-ͽͿ-῿‌-‍",
                r"⁰-↏Ⰰ-⿯、-퟿",
                r"豈-﷏ﷰ-�",
                r"\:\-\.0-9·̀-ͯ‿-⁀]+"
            ),
            0,
        )
    });
    re.sub(name, "_")
}

fn is_header(tag: &str) -> bool {
    matches!(tag, "h1" | "h2" | "h3" | "h4" | "h5" | "h6")
}

struct AttrPatterns {
    header: Regex,
    block: Regex,
    inline: Regex,
}

fn patterns() -> &'static AttrPatterns {
    static P: OnceLock<AttrPatterns> = OnceLock::new();
    P.get_or_init(|| {
        let base = r"\{\:?[ ]*([^\}\n ][^\n]*)[ ]*\}";
        AttrPatterns {
            header: Regex::new(&format!(r"[ ]+{base}[ ]*$"), 0),
            block: Regex::new(&format!(r"\n[ ]*{base}[ ]*$"), 0),
            inline: Regex::new(&format!(r"^{base}"), 0),
        }
    })
}

/// Apply attribute lists to the elements they follow.
pub struct AttrListTreeprocessor;

impl AttrListTreeprocessor {
    /// Set the attributes on `elem`; with `strict`, nothing is set when text follows the closing brace.
    /// Returns the unscanned remainder.
    fn assign_attrs(md: &mut Markdown, elem: El, attrs_string: &str, strict: bool) -> String {
        let (attrs, remainder) = get_attrs_and_remainder(attrs_string);
        if strict && !remainder.is_empty() {
            return remainder;
        }
        for (k, v) in attrs {
            if k == "." {
                match md
                    .tree
                    .get(elem, "class")
                    .filter(|c| !c.is_empty())
                    .map(str::to_string)
                {
                    Some(cls) => md.tree.set(elem, "class", format!("{cls} {v}")),
                    None => md.tree.set(elem, "class", v),
                }
            } else {
                md.tree.set(elem, &sanitize_name(&k), v);
            }
        }
        remainder
    }

    /// Look for a strict attribute list at the end of the text (`in_tail` false) or of a child's tail.
    fn apply_end(md: &mut Markdown, re: &Regex, elem: El, target: El, in_tail: bool, header: bool) {
        let s = if in_tail {
            md.tree.tail_str(target)
        } else {
            md.tree.text_str(target)
        }
        .unwrap_or("")
        .to_string();
        let Some(m) = re.search(&s, 0) else { return };
        if Self::assign_attrs(md, elem, m.str(1), true).is_empty() {
            let mut kept = s[..m.start(0)].to_string();
            if header {
                kept = crate::pystr::rstrip(crate::pystr::rstrip_chars(&kept, "#")).to_string();
            }
            if in_tail {
                md.tree.set_tail_str(target, kept);
            } else {
                md.tree.set_text_str(target, kept);
            }
        }
    }
}

impl Treeprocessor for AttrListTreeprocessor {
    fn run(&self, md: &mut Markdown, doc: El) -> PyResult<()> {
        let p = patterns();
        for elem in md.tree.iter(doc) {
            let tag = md.tree.tag(elem).to_string();
            if md.is_block_level(&tag) {
                let re = if is_header(&tag) || matches!(tag.as_str(), "dt" | "td" | "th") {
                    &p.header
                } else {
                    &p.block
                };
                let n = md.tree.len(elem);
                if n > 0 && tag == "li" {
                    let pos = md
                        .tree
                        .children(elem)
                        .iter()
                        .position(|&c| matches!(md.tree.tag(c), "ul" | "ol"));
                    let last = md.tree.child(elem, n - 1);
                    match pos {
                        None if md.tree.has_tail(last) => {
                            Self::apply_end(md, re, elem, last, true, false)
                        }
                        Some(pos) if pos > 0 && md.tree.has_tail(md.tree.child(elem, pos - 1)) => {
                            let prev = md.tree.child(elem, pos - 1);
                            Self::apply_end(md, re, elem, prev, true, false);
                        }
                        _ => {
                            if md.tree.has_text(elem) {
                                Self::apply_end(md, re, elem, elem, false, false);
                            }
                        }
                    }
                } else if n > 0 && md.tree.has_tail(md.tree.child(elem, n - 1)) {
                    let last = md.tree.child(elem, n - 1);
                    Self::apply_end(md, re, elem, last, true, is_header(&tag));
                } else if md.tree.has_text(elem) {
                    Self::apply_end(md, re, elem, elem, false, is_header(&tag));
                }
            } else if md.tree.has_tail(elem) {
                let tail = md.tree.tail_str(elem).unwrap_or("").to_string();
                if let Some(m) = p.inline.match_at(&tail, 0) {
                    let remainder = Self::assign_attrs(md, elem, m.str(1), false);
                    md.tree
                        .set_tail_str(elem, format!("{}{remainder}", &tail[m.end(0)..]));
                }
            }
        }
        Ok(())
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}
