// Tables Extension for Python-Markdown
// ====================================
//
// Adds parsing of tables to Python-Markdown.
//
// See https://Python-Markdown.github.io/extensions/tables
// for documentation.
//
// Original code Copyright 2009 [Waylan Limberg](https://github.com/waylan)
//
// All changes Copyright 2008-2014 The Python Markdown Project
//
// License: [BSD](https://opensource.org/licenses/bsd-license.php)

//! Tables: pipe separated rows with a header separator line that sets the column alignment.

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::collections::BTreeSet;
use std::rc::Rc;

use super::Config;
use crate::core::{BlockProcessor, Extension, Markdown, PyResult};
use crate::etree::El;
use crate::pyre::Regex;
use crate::pystr;
use crate::value::Value;

pub struct TableExtension {
    config: Config,
}

impl TableExtension {
    pub fn create(configs: &[(String, Value)]) -> PyResult<Rc<dyn Extension>> {
        let mut config = Config::new(&[("use_align_attribute", Value::Bool(false))]);
        config.set_all(configs)?;
        Ok(Rc::new(TableExtension { config }))
    }
}

impl Extension for TableExtension {
    fn extend_markdown(&self, md: &mut Markdown, _this: Rc<dyn Extension>) -> PyResult<()> {
        if !md.escaped_chars.contains(&'|') {
            md.escaped_chars.push('|');
        }
        let processor = TableProcessor {
            use_align_attribute: self.config.get("use_align_attribute").truthy(),
            border: Cell::new(0),
            separator: RefCell::new(Vec::new()),
            code_pipes: Regex::new(r"(?:(\\\\)|(\\`+)|(`+)|(\\\|)|(\|))", 0),
            end_border: Regex::new(r"(?<!\\)(?:\\\\)*\|$", 0),
        };
        md.block_processors
            .register(Rc::new(processor), "table", 75.0);
        Ok(())
    }

    fn class_path(&self) -> &'static str {
        "markdown.extensions.tables.TableExtension"
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

const PIPE_LEFT: u8 = 1;
const PIPE_RIGHT: u8 = 2;

/// The table block processor.
pub struct TableProcessor {
    use_align_attribute: bool,
    border: Cell<u8>,
    separator: RefCell<Vec<String>>,
    code_pipes: Regex,
    end_border: Regex,
}

impl TableProcessor {
    fn split_row(&self, row: &str) -> Vec<String> {
        let mut row = row.to_string();
        if self.border.get() != 0 {
            if let Some(rest) = row.strip_prefix('|') {
                row = rest.to_string();
            }
            row = self.end_border.sub(&row, "");
        }
        self.split(&row)
    }

    /// Split a row at the pipes that are not escaped and not inside a code span.
    fn split(&self, row: &str) -> Vec<String> {
        let mut pipes = Vec::new();
        let mut tics: Vec<usize> = Vec::new();
        let mut tic_points: Vec<(usize, usize, usize)> = Vec::new();
        let mut tic_region: Vec<(usize, usize)> = Vec::new();
        for m in self.code_pipes.finditer(row, 0) {
            if let Some(g) = m.group(2) {
                tics.push(g.len() - 1);
                tic_points.push((m.start(2), m.end(2) - 1, 1));
            } else if let Some(g) = m.group(3) {
                tics.push(g.len());
                tic_points.push((m.start(3), m.end(3) - 1, 0));
            } else if m.group(5).is_some() {
                pipes.push(m.start(5));
            }
        }
        let mut pos = 0;
        while pos < tics.len() {
            let tic_size = tics[pos] as isize - tic_points[pos].2 as isize;
            let found = if tic_size == 0 {
                None
            } else {
                tics[pos + 1..]
                    .iter()
                    .position(|&t| t as isize == tic_size)
                    .map(|i| i + 1)
            };
            match found {
                Some(index) => {
                    tic_region.push((tic_points[pos].0, tic_points[pos + index].1));
                    pos += index + 1;
                }
                None => pos += 1,
            }
        }
        let mut good_pipes = Vec::new();
        for pipe in pipes {
            let mut throw_out = false;
            for region in &tic_region {
                if pipe < region.0 {
                    break;
                } else if region.0 <= pipe && pipe <= region.1 {
                    throw_out = true;
                    break;
                }
            }
            if !throw_out {
                good_pipes.push(pipe);
            }
        }
        let mut elements = Vec::new();
        let mut pos = 0;
        for pipe in good_pipes {
            elements.push(row[pos..pipe].to_string());
            pos = pipe + 1;
        }
        elements.push(row[pos..].to_string());
        elements
    }

    fn build_row(&self, md: &mut Markdown, row: &str, parent: El, align: &[Option<&str>]) {
        let tr = md.tree.sub_element(parent, "tr");
        let tag = if md.tree.tag(parent) == "thead" {
            "th"
        } else {
            "td"
        };
        let cells = self.split_row(row);
        for (i, a) in align.iter().enumerate() {
            let c = md.tree.sub_element(tr, tag);
            let text = cells
                .get(i)
                .map(|c| pystr::strip_chars(c, " ").to_string())
                .unwrap_or_default();
            md.tree.set_text_str(c, text);
            if let Some(a) = a {
                if self.use_align_attribute {
                    md.tree.set(c, "align", *a);
                } else {
                    md.tree.set(c, "style", format!("text-align: {a};"));
                }
            }
        }
    }
}

impl BlockProcessor for TableProcessor {
    fn test(&self, _md: &mut Markdown, _parent: El, block: &str) -> bool {
        let rows: Vec<&str> = block
            .split('\n')
            .map(|r| pystr::strip_chars(r, " "))
            .collect();
        if rows.len() <= 1 {
            return false;
        }
        let header0 = rows[0];
        let mut border = 0;
        if header0.starts_with('|') {
            border |= PIPE_LEFT;
        }
        if self.end_border.is_match(header0) {
            border |= PIPE_RIGHT;
        }
        self.border.set(border);
        let row = self.split_row(header0);
        let row0_len = row.len();
        let mut is_table = row0_len > 1;
        if !is_table && row0_len == 1 && border != 0 {
            for r in &rows[1..] {
                is_table = r.starts_with('|') || self.end_border.is_match(r);
                if !is_table {
                    break;
                }
            }
        }
        if is_table {
            let row = self.split_row(rows[1]);
            let chars: BTreeSet<char> = row.concat().chars().collect();
            is_table = row.len() == row0_len && chars.iter().all(|c| "|:- ".contains(*c));
            if is_table {
                *self.separator.borrow_mut() = row;
            }
        }
        is_table
    }

    fn run(&self, md: &mut Markdown, parent: El, blocks: &mut Vec<String>) -> PyResult<bool> {
        let block_text = blocks.remove(0);
        let block: Vec<&str> = block_text.split('\n').collect();
        let header = pystr::strip_chars(block[0], " ");
        let rows: &[&str] = if block.len() < 3 { &[] } else { &block[2..] };
        let separator = self.separator.borrow().clone();
        let align: Vec<Option<&str>> = separator
            .iter()
            .map(|c| {
                let c = pystr::strip_chars(c, " ");
                if c.starts_with(':') && c.ends_with(':') {
                    Some("center")
                } else if c.starts_with(':') {
                    Some("left")
                } else if c.ends_with(':') {
                    Some("right")
                } else {
                    None
                }
            })
            .collect();
        let table = md.tree.sub_element(parent, "table");
        let thead = md.tree.sub_element(table, "thead");
        self.build_row(md, header, thead, &align);
        let tbody = md.tree.sub_element(table, "tbody");
        if rows.is_empty() {
            let tr = md.tree.sub_element(tbody, "tr");
            for _ in 0..align.len() {
                md.tree.sub_element(tr, "td");
            }
        } else {
            for row in rows {
                self.build_row(md, pystr::strip_chars(row, " "), tbody, &align);
            }
        }
        Ok(true)
    }
}
