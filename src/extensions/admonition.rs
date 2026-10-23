// Admonition extension for Python-Markdown
// ========================================
//
// Adds rST-style admonitions. Inspired by [rST][] feature with the same name.
//
// [rST]: http://docutils.sourceforge.net/docs/ref/rst/directives.html#specific-admonitions
//
// See https://Python-Markdown.github.io/extensions/admonition
// for documentation.
//
// Original code Copyright [Tiago Serafim](https://www.tiagoserafim.com/).
//
// All changes Copyright The Python Markdown Project
//
// License: [BSD](https://opensource.org/licenses/bsd-license.php)

//! Admonitions: `!!! type "title"` blocks, rST style.

use std::any::Any;
use std::cell::Cell;
use std::rc::Rc;

use super::Config;
use crate::blockprocessors::detab;
use crate::core::{BlockProcessor, Extension, Markdown, PyResult};
use crate::etree::El;
use crate::pyre::Regex;
use crate::pystr;
use crate::value::Value;

pub struct AdmonitionExtension;

impl AdmonitionExtension {
    pub fn create(configs: &[(String, Value)]) -> PyResult<Rc<dyn Extension>> {
        Config::default().set_all(configs)?;
        Ok(Rc::new(AdmonitionExtension))
    }
}

impl Extension for AdmonitionExtension {
    fn extend_markdown(&self, md: &mut Markdown, this: Rc<dyn Extension>) -> PyResult<()> {
        md.register_extension(this);
        md.block_processors.register(
            Rc::new(AdmonitionProcessor::new(md.tab_length)),
            "admonition",
            105.0,
        );
        Ok(())
    }

    fn class_path(&self) -> &'static str {
        "markdown.extensions.admonition.AdmonitionExtension"
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

const CLASSNAME: &str = "admonition";
const CLASSNAME_TITLE: &str = "admonition-title";

/// The admonition block processor; continuation blocks indented under an admonition (also inside lists)
/// are added to it.
pub struct AdmonitionProcessor {
    tab_length: usize,
    re: Regex,
    re_spaces: Regex,
    current_sibling: Cell<Option<El>>,
    content_indent: Cell<usize>,
}

impl AdmonitionProcessor {
    fn new(tab_length: usize) -> AdmonitionProcessor {
        AdmonitionProcessor {
            tab_length,
            re: Regex::new(
                r#"(?:^|\n)!!! ?([\w\-]+(?: +[\w\-]+)*)(?: +"(.*?)")? *(?:\n|$)"#,
                0,
            ),
            re_spaces: Regex::new("  +", 0),
            current_sibling: Cell::new(None),
            content_indent: Cell::new(0),
        }
    }

    fn parse_content(
        &self,
        md: &Markdown,
        parent: El,
        block: &str,
    ) -> (Option<El>, String, String) {
        let old_block = block;
        if let Some(sibling) = self.current_sibling.get() {
            let (block, the_rest) = detab(block, self.content_indent.get());
            self.current_sibling.set(None);
            self.content_indent.set(0);
            return (Some(sibling), block, the_rest);
        }
        let mut block = block.to_string();
        let mut the_rest = String::new();
        let mut sibling = md.tree.last_child(parent);
        let is_admonition = |s: El| {
            md.tree.tag(s) == "div" && md.tree.get(s, "class").unwrap_or("").contains(CLASSNAME)
        };
        if !sibling.is_some_and(is_admonition) {
            sibling = None;
        } else {
            let mut last_child = sibling.and_then(|s| md.tree.last_child(s));
            let mut indent = 0;
            let double = " ".repeat(self.tab_length * 2);
            while let Some(lc) = last_child {
                if sibling.is_some()
                    && block.starts_with(&double)
                    && matches!(md.tree.tag(lc), "ul" | "ol" | "dl")
                {
                    sibling = md.tree.last_child(lc);
                    last_child = sibling.and_then(|s| md.tree.last_child(s));
                    block = block[self.tab_length..].to_string();
                    indent += self.tab_length;
                } else {
                    last_child = None;
                }
            }
            if !block.starts_with(&" ".repeat(self.tab_length)) {
                sibling = None;
            }
            if let Some(s) = sibling {
                indent += self.tab_length;
                let (b, rest) = detab(old_block, indent);
                block = b;
                the_rest = rest;
                self.current_sibling.set(Some(s));
                self.content_indent.set(indent);
            }
        }
        (sibling, block, the_rest)
    }

    fn get_class_and_title(&self, klass: &str, title: Option<&str>) -> (String, Option<String>) {
        let klass = self.re_spaces.sub(&pystr::lower(klass), " ");
        let title = match title {
            None => Some(pystr::capitalize(klass.split(' ').next().unwrap_or(""))),
            Some("") => None,
            Some(t) => Some(t.to_string()),
        };
        (klass, title)
    }
}

impl BlockProcessor for AdmonitionProcessor {
    fn test(&self, md: &mut Markdown, parent: El, block: &str) -> bool {
        self.re.is_match(block) || self.parse_content(md, parent, block).0.is_some()
    }

    fn run(&self, md: &mut Markdown, parent: El, blocks: &mut Vec<String>) -> PyResult<bool> {
        let block = blocks.remove(0);
        let found = self.re.search(&block, 0).map(|m| {
            (
                m.start(0),
                m.end(0),
                m.str(1).to_string(),
                m.group(2).map(str::to_string),
            )
        });
        let (div, block, the_rest) = match found {
            Some((start, end, klass, title)) => {
                if start > 0 {
                    md.parse_blocks(parent, vec![block[..start].to_string()])?;
                }
                let (block, the_rest) = detab(&block[end..], self.tab_length);
                let (klass, title) = self.get_class_and_title(&klass, title.as_deref());
                let div = md.tree.sub_element(parent, "div");
                md.tree.set(div, "class", format!("{CLASSNAME} {klass}"));
                if let Some(title) = title {
                    let p = md.tree.sub_element(div, "p");
                    md.tree.set_text_str(p, title);
                    md.tree.set(p, "class", CLASSNAME_TITLE);
                }
                (div, block, the_rest)
            }
            None => {
                let (sibling, block, the_rest) = self.parse_content(md, parent, &block);
                let sibling = sibling.expect("test() found the admonition to continue");
                if matches!(md.tree.tag(sibling), "li" | "dd") && md.tree.has_text(sibling) {
                    let text = md.tree.text(sibling).cloned();
                    md.tree.set_text_str(sibling, "");
                    let p = md.tree.sub_element(sibling, "p");
                    md.tree.set_text(p, text);
                }
                (sibling, block, the_rest)
            }
        };
        md.parse_chunk(div, &block)?;
        if !the_rest.is_empty() {
            blocks.insert(0, the_rest);
        }
        Ok(true)
    }
}
