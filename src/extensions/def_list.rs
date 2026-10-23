// Definition List Extension for Python-Markdown
// =============================================
//
// Adds parsing of Definition Lists to Python-Markdown.
//
// See https://Python-Markdown.github.io/extensions/definition_lists
// for documentation.
//
// Original code Copyright 2008 [Waylan Limberg](https://github.com/waylan)
//
// All changes Copyright 2008-2014 The Python Markdown Project
//
// License: [BSD](https://opensource.org/licenses/bsd-license.php)

//! Definition lists: terms followed by `: definition` lines.

use std::any::Any;
use std::rc::Rc;

use super::Config;
use crate::blockprocessors::{detab, ListIndentProcessor};
use crate::core::{BlockProcessor, Extension, Markdown, PyResult};
use crate::etree::El;
use crate::pyre::Regex;
use crate::pystr;
use crate::value::Value;

pub struct DefListExtension;

impl DefListExtension {
    pub fn create(configs: &[(String, Value)]) -> PyResult<Rc<dyn Extension>> {
        Config::default().set_all(configs)?;
        Ok(Rc::new(DefListExtension))
    }
}

impl Extension for DefListExtension {
    fn extend_markdown(&self, md: &mut Markdown, _this: Rc<dyn Extension>) -> PyResult<()> {
        md.block_processors.register(
            Rc::new(ListIndentProcessor::new(md.tab_length, true)),
            "defindent",
            85.0,
        );
        md.block_processors.register(
            Rc::new(DefListProcessor::new(md.tab_length)),
            "deflist",
            25.0,
        );
        Ok(())
    }

    fn class_path(&self) -> &'static str {
        "markdown.extensions.def_list.DefListExtension"
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// A definition: the terms before it, or the paragraph just before it, become the `dt` elements.
pub struct DefListProcessor {
    tab_length: usize,
    re: Regex,
    no_indent_re: Regex,
}

impl DefListProcessor {
    fn new(tab_length: usize) -> DefListProcessor {
        DefListProcessor {
            tab_length,
            re: Regex::new(r"(^|\n)[ ]{0,3}:[ ]{1,3}(.*?)(\n|$)", 0),
            no_indent_re: Regex::new(r"^[ ]{0,3}[^ :]", 0),
        }
    }
}

impl BlockProcessor for DefListProcessor {
    fn test(&self, _md: &mut Markdown, _parent: El, block: &str) -> bool {
        self.re.is_match(block)
    }

    fn run(&self, md: &mut Markdown, parent: El, blocks: &mut Vec<String>) -> PyResult<bool> {
        let raw_block = blocks.remove(0);
        let m = self
            .re
            .search(&raw_block, 0)
            .expect("test() found a definition");
        let mut terms: Vec<String> = raw_block[..m.start(0)]
            .split('\n')
            .map(pystr::strip)
            .filter(|t| !t.is_empty())
            .map(str::to_string)
            .collect();
        let block = &raw_block[m.end(0)..];
        let (d, the_rest) = if self.no_indent_re.match_at(block, 0).is_some() {
            (block.to_string(), String::new())
        } else {
            detab(block, self.tab_length)
        };
        let d = if d.is_empty() {
            m.str(2).to_string()
        } else {
            format!("{}\n{d}", m.str(2))
        };
        let mut sibling = md.tree.last_child(parent);
        if terms.is_empty() && sibling.is_none() {
            blocks.insert(0, raw_block);
            return Ok(false);
        }
        let mut state = "list";
        if terms.is_empty() && sibling.is_some_and(|s| md.tree.tag(s) == "p") {
            let s = sibling.expect("a paragraph");
            state = "looselist";
            terms = md
                .tree
                .text_str(s)
                .unwrap_or("")
                .split('\n')
                .map(str::to_string)
                .collect();
            md.tree.remove(parent, s);
            sibling = md.tree.last_child(parent);
        }
        let dl = match sibling {
            Some(s) if md.tree.tag(s) == "dl" => {
                if terms.is_empty() {
                    if let Some(last) = md.tree.last_child(s) {
                        if md.tree.tag(last) == "dd" && md.tree.len(last) > 0 {
                            state = "looselist";
                        }
                    }
                }
                s
            }
            _ => md.tree.sub_element(parent, "dl"),
        };
        for term in terms {
            let dt = md.tree.sub_element(dl, "dt");
            md.tree.set_text_str(dt, term);
        }
        md.state_set(state);
        let dd = md.tree.sub_element(dl, "dd");
        md.parse_blocks(dd, vec![d])?;
        md.state_reset();
        if !the_rest.is_empty() {
            blocks.insert(0, the_rest);
        }
        Ok(true)
    }
}
