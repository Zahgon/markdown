// Abbreviation Extension for Python-Markdown
// ==========================================
//
// This extension adds abbreviation handling to Python-Markdown.
//
// See https://Python-Markdown.github.io/extensions/abbreviations
// for documentation.
//
// Original code Copyright 2007-2008 [Waylan Limberg](https://github.com/waylan)
// and [Seemant Kulleen](http://www.kulleen.org/)
//
// All changes Copyright 2008-2014 The Python Markdown Project
//
// License: [BSD](https://opensource.org/licenses/bsd-license.php)

//! Abbreviations: `*[HTML]: Hyper Text Markup Language` definitions wrap every use in `<abbr>`.

use std::any::Any;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::OnceLock;

use super::Config;
use crate::core::{BlockProcessor, Extension, Markdown, PyError, PyResult, Treeprocessor};
use crate::etree::{El, Text};
use crate::pyre::{Regex, M};
use crate::pystr;
use crate::value::Value;

/// The abbreviations of a document, in definition order.
type Abbrs = Rc<RefCell<Vec<(String, Value)>>>;

pub struct AbbrExtension {
    config: Config,
    abbrs: Abbrs,
    glossary: RefCell<Vec<(String, Value)>>,
}

impl AbbrExtension {
    pub fn create(configs: &[(String, Value)]) -> PyResult<Rc<dyn Extension>> {
        let mut config = Config::new(&[("glossary", Value::Dict(Vec::new()))]);
        config.set_all(configs)?;
        Ok(Rc::new(AbbrExtension {
            config,
            abbrs: Rc::default(),
            glossary: RefCell::default(),
        }))
    }

    /// `load_glossary`: definitions already in the glossary win over the new ones.
    fn load_glossary(&self, dictionary: &Value) -> PyResult<()> {
        if !dictionary.truthy() {
            return Ok(());
        }
        let Value::Dict(items) = dictionary else {
            return Err(PyError::new(
                "TypeError",
                format!("'{}' object is not a mapping", dictionary.type_name()),
            ));
        };
        let mut merged = items.clone();
        for (k, v) in self.glossary.borrow().iter() {
            dict_set(&mut merged, k, v.clone());
        }
        *self.glossary.borrow_mut() = merged;
        Ok(())
    }
}

fn dict_set(dict: &mut Vec<(String, Value)>, key: &str, value: Value) {
    match dict.iter_mut().find(|(k, _)| k == key) {
        Some(slot) => slot.1 = value,
        None => dict.push((key.to_string(), value)),
    }
}

impl Extension for AbbrExtension {
    fn extend_markdown(&self, md: &mut Markdown, this: Rc<dyn Extension>) -> PyResult<()> {
        let glossary = self.config.get("glossary").clone();
        if glossary.truthy() {
            self.load_glossary(&glossary)?;
        }
        for (k, v) in self.glossary.borrow().iter() {
            dict_set(&mut self.abbrs.borrow_mut(), k, v.clone());
        }
        md.register_extension(this);
        md.treeprocessors.register(
            Rc::new(AbbrTreeprocessor {
                abbrs: self.abbrs.clone(),
            }),
            "abbr",
            7.0,
        );
        md.block_processors.register(
            Rc::new(AbbrBlockprocessor {
                abbrs: self.abbrs.clone(),
            }),
            "abbr",
            16.0,
        );
        Ok(())
    }

    fn reset(&self, _md: &mut Markdown) {
        let mut abbrs = self.abbrs.borrow_mut();
        abbrs.clear();
        for (k, v) in self.glossary.borrow().iter() {
            dict_set(&mut abbrs, k, v.clone());
        }
    }

    fn class_path(&self) -> &'static str {
        "markdown.extensions.abbr.AbbrExtension"
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Wrap known abbreviations in text and tails with `<abbr title="...">`.
pub struct AbbrTreeprocessor {
    abbrs: Abbrs,
}

impl AbbrTreeprocessor {
    fn title(&self, key: &str) -> Value {
        self.abbrs
            .borrow()
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.clone())
            .unwrap_or(Value::None)
    }

    fn create_element(md: &mut Markdown, title: &Value, text: &str, tail: &str) -> El {
        let abbr = md
            .tree
            .element_with("abbr", vec![("title".into(), title.py_str())]);
        md.tree.set_text(abbr, Some(Text::atomic(text)));
        md.tree.set_tail_str(abbr, tail);
        abbr
    }

    fn iter_element(&self, md: &mut Markdown, re: &Regex, el: El, parent: Option<El>) {
        for child in md.tree.children(el).to_vec().into_iter().rev() {
            self.iter_element(md, re, child, Some(el));
        }
        if let Some(text) = md.tree.text(el).filter(|t| !t.s.is_empty()).cloned() {
            if !text.atomic {
                let mut s = text.s.clone();
                let spans: Vec<(usize, usize)> = re
                    .finditer(&text.s, 0)
                    .map(|m| (m.start(0), m.end(0)))
                    .collect();
                for (start, end) in spans.into_iter().rev() {
                    let key = &text.s[start..end];
                    let title = self.title(key);
                    if title.truthy() {
                        let abbr = Self::create_element(md, &title, key, &s[end..]);
                        md.tree.insert(el, 0, abbr);
                        s.truncate(start);
                    }
                }
                md.tree.set_text_str(el, s);
            }
        }
        if let Some(parent) = parent {
            if let Some(tail) = md.tree.tail(el).filter(|t| !t.s.is_empty()).cloned() {
                let index = md.tree.index_of(parent, el).map_or(0, |i| i + 1);
                if !tail.atomic {
                    let mut s = tail.s.clone();
                    let spans: Vec<(usize, usize)> = re
                        .finditer(&tail.s, 0)
                        .map(|m| (m.start(0), m.end(0)))
                        .collect();
                    for (start, end) in spans.into_iter().rev() {
                        let key = &tail.s[start..end];
                        let title = self.title(key);
                        let abbr = Self::create_element(md, &title, key, &s[end..]);
                        md.tree.insert(parent, index, abbr);
                        s.truncate(start);
                    }
                    md.tree.set_tail_str(el, s);
                }
            }
        }
    }
}

impl Treeprocessor for AbbrTreeprocessor {
    fn run(&self, md: &mut Markdown, root: El) -> PyResult<()> {
        if self.abbrs.borrow().is_empty() {
            return Ok(());
        }
        let mut keys: Vec<String> = self.abbrs.borrow().iter().map(|(k, _)| k.clone()).collect();
        keys.sort_by_key(|k| std::cmp::Reverse(pystr::char_len(k)));
        let alternatives: Vec<String> = keys.iter().map(|k| crate::pyre::escape(k)).collect();
        let re = Regex::new(&format!(r"\b(?:{})\b", alternatives.join("|")), 0);
        self.iter_element(md, &re, root, None);
        Ok(())
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

fn block_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"^[*]\[(?P<abbr>[^\\]*?)\][ ]?:[ ]*\n?[ ]*(?P<title>.*)$",
            M,
        )
    })
}

/// Read abbreviation definitions.
pub struct AbbrBlockprocessor {
    abbrs: Abbrs,
}

impl BlockProcessor for AbbrBlockprocessor {
    fn test(&self, _md: &mut Markdown, _parent: El, _block: &str) -> bool {
        true
    }

    fn run(&self, _md: &mut Markdown, _parent: El, blocks: &mut Vec<String>) -> PyResult<bool> {
        let block = blocks.remove(0);
        if let Some(m) = block_re().search(&block, 0) {
            let abbr = pystr::strip(m.name("abbr").unwrap_or("")).to_string();
            let title = pystr::strip(m.name("title").unwrap_or("")).to_string();
            if !title.is_empty() && !abbr.is_empty() {
                if title == "''" || title == "\"\"" {
                    self.abbrs.borrow_mut().retain(|(k, _)| *k != abbr);
                } else {
                    dict_set(&mut self.abbrs.borrow_mut(), &abbr, Value::Str(title));
                }
                let (start, end) = (m.start(0), m.end(0));
                if !pystr::strip(&block[end..]).is_empty() {
                    blocks.insert(0, pystr::lstrip_chars(&block[end..], "\n").to_string());
                }
                if !pystr::strip(&block[..start]).is_empty() {
                    blocks.insert(0, pystr::rstrip_chars(&block[..start], "\n").to_string());
                }
                return Ok(true);
            }
        }
        blocks.insert(0, block);
        Ok(false)
    }
}
