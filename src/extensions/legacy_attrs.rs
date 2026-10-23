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

//! Legacy attributes: `{@key=value}` in text sets an attribute on the enclosing element.

use std::any::Any;
use std::rc::Rc;
use std::sync::OnceLock;

use super::attr_list::sanitize_name;
use super::Config;
use crate::core::{Extension, Markdown, PyResult, Treeprocessor};
use crate::etree::El;
use crate::pyre::Regex;
use crate::value::Value;

pub struct LegacyAttrExtension;

impl LegacyAttrExtension {
    pub fn create(configs: &[(String, Value)]) -> PyResult<Rc<dyn Extension>> {
        Config::default().set_all(configs)?;
        Ok(Rc::new(LegacyAttrExtension))
    }
}

impl Extension for LegacyAttrExtension {
    fn extend_markdown(&self, md: &mut Markdown, _this: Rc<dyn Extension>) -> PyResult<()> {
        md.treeprocessors
            .register(Rc::new(LegacyAttrs), "legacyattrs", 15.0);
        Ok(())
    }

    fn class_path(&self) -> &'static str {
        "markdown.extensions.legacy_attrs.LegacyAttrExtension"
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

fn attr_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\{@([^\}]*)=([^\}]*)}", 0))
}

/// Move `{@key=value}` attributes from text, tails and `alt` values onto their element.
pub struct LegacyAttrs;

impl LegacyAttrs {
    fn handle_attributes(md: &mut Markdown, el: El, text: &str) -> String {
        let mut found: Vec<(String, String)> = Vec::new();
        let out = attr_re().sub_fn(text, |m| {
            found.push((sanitize_name(m.str(1)), m.str(2).replace('\n', " ")));
            String::new()
        });
        for (k, v) in found {
            md.tree.set(el, &k, v);
        }
        out
    }
}

impl Treeprocessor for LegacyAttrs {
    fn run(&self, md: &mut Markdown, root: El) -> PyResult<()> {
        for el in md.tree.iter(root) {
            if let Some(alt) = md.tree.get(el, "alt").map(str::to_string) {
                let new = Self::handle_attributes(md, el, &alt);
                md.tree.set(el, "alt", new);
            }
            if let Some(text) = md
                .tree
                .text(el)
                .filter(|t| !t.s.is_empty() && !t.atomic)
                .cloned()
            {
                let new = Self::handle_attributes(md, el, &text.s);
                md.tree.set_text_str(el, new);
            }
            if let Some(tail) = md
                .tree
                .tail(el)
                .filter(|t| !t.s.is_empty() && !t.atomic)
                .cloned()
            {
                let new = Self::handle_attributes(md, el, &tail.s);
                md.tree.set_tail_str(el, new);
            }
        }
        Ok(())
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}
