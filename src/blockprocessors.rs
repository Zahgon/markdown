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

//! Block processors: each one recognizes one kind of Markdown block (lists, code blocks, headers, quotes...)
//! and builds its elements. The block parser offers every block to the processors in priority order.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use crate::core::{BlockProcessor, Markdown, PyResult};
use crate::etree::{El, Text};
use crate::pyre::{Regex, M};
use crate::pystr;
use crate::util::code_escape;

/// Register the default block processors.
pub fn build_block_parser(md: &mut Markdown) {
    let tab = md.tab_length;
    md.block_processors
        .register(Rc::new(EmptyBlockProcessor), "empty", 100.0);
    md.block_processors.register(
        Rc::new(ListIndentProcessor::new(tab, false)),
        "indent",
        90.0,
    );
    md.block_processors.register(
        Rc::new(CodeBlockProcessor { tab_length: tab }),
        "code",
        80.0,
    );
    md.block_processors
        .register(Rc::new(HashHeaderProcessor::new()), "hashheader", 70.0);
    md.block_processors
        .register(Rc::new(SetextHeaderProcessor::new()), "setextheader", 60.0);
    md.block_processors
        .register(Rc::new(HrProcessor::new()), "hr", 50.0);
    md.block_processors
        .register(Rc::new(ListProcessor::olist(tab, false)), "olist", 40.0);
    md.block_processors
        .register(Rc::new(ListProcessor::ulist(tab, false)), "ulist", 30.0);
    md.block_processors
        .register(Rc::new(BlockQuoteProcessor::new()), "quote", 20.0);
    md.block_processors
        .register(Rc::new(ReferenceProcessor::new()), "reference", 15.0);
    md.block_processors
        .register(Rc::new(ParagraphProcessor), "paragraph", 10.0);
}

/// `BlockProcessor.detab`: remove one level of indent from the leading indented lines of a block; the rest
/// starts at the first line that is neither indented nor blank.
pub fn detab(text: &str, length: usize) -> (String, String) {
    let indent = " ".repeat(length);
    let lines: Vec<&str> = text.split('\n').collect();
    let mut newtext: Vec<&str> = Vec::new();
    for line in &lines {
        if let Some(rest) = line.strip_prefix(indent.as_str()) {
            newtext.push(rest);
        } else if pystr::strip(line).is_empty() {
            newtext.push("");
        } else {
            break;
        }
    }
    let n = newtext.len();
    (newtext.join("\n"), lines[n..].join("\n"))
}

/// `BlockProcessor.looseDetab`: remove `level` indents from every line that has them.
pub fn loose_detab(text: &str, tab_length: usize, level: usize) -> String {
    let indent = " ".repeat(tab_length * level);
    text.split('\n')
        .map(|line| line.strip_prefix(indent.as_str()).unwrap_or(line))
        .collect::<Vec<_>>()
        .join("\n")
}

/// `self.lastChild(parent)`.
fn last_child(md: &Markdown, parent: El) -> Option<El> {
    md.tree.last_child(parent)
}

/// Whether an element is a `pre` holding a `code` element.
fn is_code_block(md: &Markdown, el: El) -> bool {
    md.tree.tag(el) == "pre" && md.tree.len(el) > 0 && md.tree.tag(md.tree.child(el, 0)) == "code"
}

/// Move the text of an element into a new first `p` child.
fn text_into_first_p(md: &mut Markdown, el: El) {
    let p = md.tree.element("p");
    let text = md.tree.text(el).cloned();
    md.tree.set_text(p, text);
    md.tree.set_text_str(el, "");
    md.tree.insert(el, 0, p);
}

/// A block that is indented as part of a list item.
pub struct ListIndentProcessor {
    tab_length: usize,
    item_types: &'static [&'static str],
    list_types: &'static [&'static str],
    def_list: bool,
    indent_re: Regex,
}

impl ListIndentProcessor {
    /// The core processor, or with `def_list` the definition list variant that also continues `dd` items.
    pub fn new(tab_length: usize, def_list: bool) -> ListIndentProcessor {
        let (item_types, list_types): (&'static [&'static str], &'static [&'static str]) =
            if def_list {
                (&["dd", "li"], &["dl", "ol", "ul"])
            } else {
                (&["li"], &["ul", "ol"])
            };
        ListIndentProcessor {
            tab_length,
            item_types,
            list_types,
            def_list,
            indent_re: Regex::new(&format!(r"^(([ ]{{{tab_length}}})+)"), 0),
        }
    }

    fn create_item(&self, md: &mut Markdown, parent: El, block: String) -> PyResult<()> {
        let item = md
            .tree
            .sub_element(parent, if self.def_list { "dd" } else { "li" });
        md.parse_blocks(item, vec![block])
    }

    fn get_level(&self, md: &Markdown, parent: El, block: &str) -> (usize, El) {
        let indent_level = match self.indent_re.match_at(block, 0) {
            Some(m) => m.str(1).len() as f64 / self.tab_length as f64,
            None => 0.0,
        };
        let mut level = if md.is_state("list") { 1 } else { 0 };
        let mut parent = parent;
        while indent_level > level as f64 {
            match last_child(md, parent) {
                Some(child)
                    if self.list_types.contains(&md.tree.tag(child))
                        || self.item_types.contains(&md.tree.tag(child)) =>
                {
                    if self.list_types.contains(&md.tree.tag(child)) {
                        level += 1;
                    }
                    parent = child;
                }
                _ => break,
            }
        }
        (level, parent)
    }
}

impl BlockProcessor for ListIndentProcessor {
    fn test(&self, md: &mut Markdown, parent: El, block: &str) -> bool {
        block.starts_with(&" ".repeat(self.tab_length))
            && !md.is_state("detabbed")
            && (self.item_types.contains(&md.tree.tag(parent))
                || last_child(md, parent)
                    .is_some_and(|c| self.list_types.contains(&md.tree.tag(c))))
    }

    fn run(&self, md: &mut Markdown, parent: El, blocks: &mut Vec<String>) -> PyResult<bool> {
        let block = blocks.remove(0);
        let (level, sibling) = self.get_level(md, parent, &block);
        let block = loose_detab(&block, self.tab_length, level);
        md.state_set("detabbed");
        if self.item_types.contains(&md.tree.tag(parent)) {
            match last_child(md, parent) {
                Some(last) if self.list_types.contains(&md.tree.tag(last)) => {
                    md.parse_blocks(last, vec![block])?
                }
                _ => md.parse_blocks(parent, vec![block])?,
            }
        } else if self.item_types.contains(&md.tree.tag(sibling)) {
            md.parse_blocks(sibling, vec![block])?;
        } else if let Some(last) =
            last_child(md, sibling).filter(|&l| self.item_types.contains(&md.tree.tag(l)))
        {
            if md.tree.has_text(last) {
                text_into_first_p(md, last);
            }
            md.parse_chunk(last, &block)?;
        } else {
            self.create_item(md, sibling, block)?;
        }
        md.state_reset();
        Ok(true)
    }
}

/// An indented code block.
pub struct CodeBlockProcessor {
    pub tab_length: usize,
}

impl BlockProcessor for CodeBlockProcessor {
    fn test(&self, _md: &mut Markdown, _parent: El, block: &str) -> bool {
        block.starts_with(&" ".repeat(self.tab_length))
    }

    fn run(&self, md: &mut Markdown, parent: El, blocks: &mut Vec<String>) -> PyResult<bool> {
        let sibling = last_child(md, parent);
        let block = blocks.remove(0);
        let (block, the_rest) = detab(&block, self.tab_length);
        match sibling.filter(|&s| is_code_block(md, s)) {
            Some(pre) => {
                // Blank lines do not start a new code block: continue the previous one.
                let code = md.tree.child(pre, 0);
                let old = md.tree.text_str(code).unwrap_or("").to_string();
                let text = format!("{old}\n{}\n", code_escape(pystr::rstrip(&block)));
                md.tree.set_text(code, Some(Text::atomic(text)));
            }
            None => {
                let pre = md.tree.sub_element(parent, "pre");
                let code = md.tree.sub_element(pre, "code");
                let text = format!("{}\n", code_escape(pystr::rstrip(&block)));
                md.tree.set_text(code, Some(Text::atomic(text)));
            }
        }
        if !the_rest.is_empty() {
            blocks.insert(0, the_rest);
        }
        Ok(true)
    }
}

/// A block quote.
pub struct BlockQuoteProcessor {
    re: Regex,
}

impl BlockQuoteProcessor {
    pub fn new() -> BlockQuoteProcessor {
        BlockQuoteProcessor {
            re: Regex::new(r"(^|\n)[ ]{0,3}>[ ]?(.*)", 0),
        }
    }

    fn clean(&self, line: &str) -> String {
        if pystr::strip(line) == ">" {
            return String::new();
        }
        match self.re.match_at(line, 0) {
            Some(m) => m.str(2).to_string(),
            None => line.to_string(),
        }
    }
}

impl Default for BlockQuoteProcessor {
    fn default() -> Self {
        Self::new()
    }
}

impl BlockProcessor for BlockQuoteProcessor {
    fn test(&self, _md: &mut Markdown, _parent: El, block: &str) -> bool {
        self.re.is_match(block)
    }

    fn run(&self, md: &mut Markdown, parent: El, blocks: &mut Vec<String>) -> PyResult<bool> {
        let block = blocks.remove(0);
        let start = self.re.search(&block, 0).map(|m| m.start(0));
        let block = match start {
            Some(start) => {
                // The lines before the quote are parsed first.
                md.parse_blocks(parent, vec![block[..start].to_string()])?;
                block[start..]
                    .split('\n')
                    .map(|line| self.clean(line))
                    .collect::<Vec<_>>()
                    .join("\n")
            }
            None => block,
        };
        let quote = match last_child(md, parent) {
            Some(sibling) if md.tree.tag(sibling) == "blockquote" => sibling,
            _ => md.tree.sub_element(parent, "blockquote"),
        };
        md.state_set("blockquote");
        md.parse_chunk(quote, &block)?;
        md.state_reset();
        Ok(true)
    }
}

/// An ordered or unordered list; `sane_lists` makes the two kinds separate and honours the start number.
pub struct ListProcessor {
    tag: &'static str,
    lazy_ol: bool,
    sibling_tags: &'static [&'static str],
    tab_length: usize,
    re: Regex,
    child_re: Regex,
    indent_re: Regex,
    startswith: RefCell<String>,
}

impl ListProcessor {
    pub fn olist(tab_length: usize, sane: bool) -> ListProcessor {
        let t = tab_length - 1;
        ListProcessor {
            tag: "ol",
            lazy_ol: !sane,
            sibling_tags: if sane { &["ol"] } else { &["ol", "ul"] },
            tab_length,
            re: Regex::new(&format!(r"^[ ]{{0,{t}}}\d+\.[ ]+(.*)"), 0),
            child_re: if sane {
                Regex::new(&format!(r"^[ ]{{0,{t}}}((\d+\.))[ ]+(.*)"), 0)
            } else {
                Regex::new(&format!(r"^[ ]{{0,{t}}}((\d+\.)|[*+-])[ ]+(.*)"), 0)
            },
            indent_re: Regex::new(
                &format!(
                    r"^[ ]{{{tab_length},{}}}((\d+\.)|[*+-])[ ]+.*",
                    tab_length * 2 - 1
                ),
                0,
            ),
            startswith: RefCell::new("1".into()),
        }
    }

    pub fn ulist(tab_length: usize, sane: bool) -> ListProcessor {
        let t = tab_length - 1;
        ListProcessor {
            tag: "ul",
            lazy_ol: true,
            sibling_tags: if sane { &["ul"] } else { &["ol", "ul"] },
            tab_length,
            re: Regex::new(&format!(r"^[ ]{{0,{t}}}[*+-][ ]+(.*)"), 0),
            child_re: if sane {
                Regex::new(&format!(r"^[ ]{{0,{t}}}(([*+-]))[ ]+(.*)"), 0)
            } else {
                Regex::new(&format!(r"^[ ]{{0,{t}}}((\d+\.)|[*+-])[ ]+(.*)"), 0)
            },
            indent_re: Regex::new(
                &format!(
                    r"^[ ]{{{tab_length},{}}}((\d+\.)|[*+-])[ ]+.*",
                    tab_length * 2 - 1
                ),
                0,
            ),
            startswith: RefCell::new("1".into()),
        }
    }

    fn get_items(&self, block: &str) -> Vec<String> {
        let indent = " ".repeat(self.tab_length);
        let mut items: Vec<String> = Vec::new();
        for line in block.split('\n') {
            if let Some(m) = self.child_re.match_at(line, 0) {
                if items.is_empty() && self.tag == "ol" {
                    let digits: String = m
                        .str(1)
                        .chars()
                        .take_while(|&c| pystr::is_digit(c))
                        .collect();
                    *self.startswith.borrow_mut() = digits;
                }
                items.push(m.str(3).to_string());
            } else if self.indent_re.match_at(line, 0).is_some() {
                match items.last_mut() {
                    Some(last) if last.starts_with(&indent) => {
                        last.push('\n');
                        last.push_str(line);
                    }
                    _ => items.push(line.to_string()),
                }
            } else if let Some(last) = items.last_mut() {
                last.push('\n');
                last.push_str(line);
            }
        }
        items
    }
}

impl BlockProcessor for ListProcessor {
    fn test(&self, _md: &mut Markdown, _parent: El, block: &str) -> bool {
        self.re.match_at(block, 0).is_some()
    }

    fn run(&self, md: &mut Markdown, parent: El, blocks: &mut Vec<String>) -> PyResult<bool> {
        let mut items = self.get_items(&blocks.remove(0));
        let sibling = last_child(md, parent);
        let indent = " ".repeat(self.tab_length);
        let lst = match sibling {
            Some(lst) if self.sibling_tags.contains(&md.tree.tag(lst)) => {
                // The previous block was a list item: this item belongs to the same (now loose) list.
                let last_item = md.tree.last_child(lst).expect("a list has items");
                if md.tree.has_text(last_item) {
                    text_into_first_p(md, last_item);
                }
                if let Some(lch) = md.tree.last_child(last_item) {
                    if md.tree.has_tail(lch) {
                        let p = md.tree.sub_element(last_item, "p");
                        let tail = pystr::lstrip(md.tree.tail_str(lch).unwrap_or("")).to_string();
                        md.tree.set_text_str(p, tail);
                        md.tree.set_tail_str(lch, "");
                    }
                }
                let li = md.tree.sub_element(lst, "li");
                md.state_set("looselist");
                let first = items.remove(0);
                md.parse_blocks(li, vec![first])?;
                md.state_reset();
                lst
            }
            _ if matches!(md.tree.tag(parent), "ol" | "ul") => parent,
            _ => {
                let lst = md.tree.sub_element(parent, self.tag);
                let start = self.startswith.borrow().clone();
                if !self.lazy_ol && start != "1" {
                    md.tree.set(lst, "start", start);
                }
                lst
            }
        };
        md.state_set("list");
        for item in items {
            if item.starts_with(&indent) {
                let last = md.tree.last_child(lst).expect("a list has items");
                md.parse_blocks(last, vec![item])?;
            } else {
                let li = md.tree.sub_element(lst, "li");
                md.parse_blocks(li, vec![item])?;
            }
        }
        md.state_reset();
        Ok(true)
    }
}

/// A `#` header.
pub struct HashHeaderProcessor {
    re: Regex,
}

impl HashHeaderProcessor {
    pub fn new() -> HashHeaderProcessor {
        HashHeaderProcessor {
            re: Regex::new(
                r"(?:^|\n)(?P<level>#{1,6})(?P<header>(?:\\.|[^\\])*?)#*(?:\n|$)",
                0,
            ),
        }
    }
}

impl Default for HashHeaderProcessor {
    fn default() -> Self {
        Self::new()
    }
}

impl BlockProcessor for HashHeaderProcessor {
    fn test(&self, _md: &mut Markdown, _parent: El, block: &str) -> bool {
        self.re.is_match(block)
    }

    fn run(&self, md: &mut Markdown, parent: El, blocks: &mut Vec<String>) -> PyResult<bool> {
        let block = blocks.remove(0);
        let Some(m) = self.re.search(&block, 0) else {
            return Ok(true);
        };
        let before = &block[..m.start(0)];
        let after = &block[m.end(0)..];
        if !before.is_empty() {
            md.parse_blocks(parent, vec![before.to_string()])?;
        }
        let level = m.name("level").unwrap_or("#").len();
        let h = md.tree.sub_element(parent, &format!("h{level}"));
        md.tree
            .set_text_str(h, pystr::strip(m.name("header").unwrap_or("")));
        if !after.is_empty() {
            let after = if md.is_state("looselist") {
                loose_detab(after, md.tab_length, 1)
            } else {
                after.to_string()
            };
            blocks.insert(0, after);
        }
        Ok(true)
    }
}

/// A setext header: a line underlined with `=` or `-`.
pub struct SetextHeaderProcessor {
    re: Regex,
}

impl SetextHeaderProcessor {
    pub fn new() -> SetextHeaderProcessor {
        SetextHeaderProcessor {
            re: Regex::new(r"^.*?\n(?:=+|-+)[ ]*(\n|$)", M),
        }
    }
}

impl Default for SetextHeaderProcessor {
    fn default() -> Self {
        Self::new()
    }
}

impl BlockProcessor for SetextHeaderProcessor {
    fn test(&self, _md: &mut Markdown, _parent: El, block: &str) -> bool {
        self.re.match_at(block, 0).is_some()
    }

    fn run(&self, md: &mut Markdown, parent: El, blocks: &mut Vec<String>) -> PyResult<bool> {
        let block = blocks.remove(0);
        let lines: Vec<&str> = block.split('\n').collect();
        let level = if lines[1].starts_with('=') { 1 } else { 2 };
        let h = md.tree.sub_element(parent, &format!("h{level}"));
        md.tree.set_text_str(h, pystr::strip(lines[0]));
        if lines.len() > 2 {
            blocks.insert(0, lines[2..].join("\n"));
        }
        Ok(true)
    }
}

/// A horizontal rule.
pub struct HrProcessor {
    search_re: Regex,
    found: Cell<(usize, usize)>,
}

impl HrProcessor {
    pub fn new() -> HrProcessor {
        HrProcessor {
            search_re: Regex::new(
                r"^[ ]{0,3}(?=(?P<atomicgroup>(-+[ ]{0,2}){3,}|(_+[ ]{0,2}){3,}|(\*+[ ]{0,2}){3,}))(?P=atomicgroup)[ ]*$",
                M,
            ),
            found: Cell::new((0, 0)),
        }
    }
}

impl Default for HrProcessor {
    fn default() -> Self {
        Self::new()
    }
}

impl BlockProcessor for HrProcessor {
    fn test(&self, _md: &mut Markdown, _parent: El, block: &str) -> bool {
        match self.search_re.search(block, 0) {
            Some(m) => {
                self.found.set((m.start(0), m.end(0)));
                true
            }
            None => false,
        }
    }

    fn run(&self, md: &mut Markdown, parent: El, blocks: &mut Vec<String>) -> PyResult<bool> {
        let block = blocks.remove(0);
        let (start, end) = self.found.get();
        let prelines = pystr::rstrip_chars(&block[..start], "\n");
        if !prelines.is_empty() {
            md.parse_blocks(parent, vec![prelines.to_string()])?;
        }
        md.tree.sub_element(parent, "hr");
        let postlines = pystr::lstrip_chars(&block[end..], "\n");
        if !postlines.is_empty() {
            blocks.insert(0, postlines.to_string());
        }
        Ok(true)
    }
}

/// An empty block, or a block that starts with a blank line.
pub struct EmptyBlockProcessor;

impl BlockProcessor for EmptyBlockProcessor {
    fn test(&self, _md: &mut Markdown, _parent: El, block: &str) -> bool {
        block.is_empty() || block.starts_with('\n')
    }

    fn run(&self, md: &mut Markdown, parent: El, blocks: &mut Vec<String>) -> PyResult<bool> {
        let block = blocks.remove(0);
        let mut filler = "\n\n";
        if !block.is_empty() {
            // Only one leading blank line is consumed; the rest is kept for later.
            filler = "\n";
            let the_rest = &block[1..];
            if !the_rest.is_empty() {
                blocks.insert(0, the_rest.to_string());
            }
        }
        if let Some(pre) = last_child(md, parent).filter(|&s| is_code_block(md, s)) {
            let code = md.tree.child(pre, 0);
            let text = format!("{}{filler}", md.tree.text_str(code).unwrap_or(""));
            md.tree.set_text(code, Some(Text::atomic(text)));
        }
        Ok(true)
    }
}

/// A link reference definition.
pub struct ReferenceProcessor {
    re: Regex,
}

impl ReferenceProcessor {
    pub fn new() -> ReferenceProcessor {
        ReferenceProcessor {
            re: Regex::new(
                r#"^[ ]{0,3}\[([^\[\]]*)\]:[ ]*(?:\n[ ]*)?([^\s]+)[ ]*(?:\n[ ]*)?((["'])(.*)\4[ ]*|\((.*)\)[ ]*)?$"#,
                M,
            ),
        }
    }
}

impl Default for ReferenceProcessor {
    fn default() -> Self {
        Self::new()
    }
}

impl BlockProcessor for ReferenceProcessor {
    fn test(&self, _md: &mut Markdown, _parent: El, _block: &str) -> bool {
        true
    }

    fn run(&self, md: &mut Markdown, _parent: El, blocks: &mut Vec<String>) -> PyResult<bool> {
        let block = blocks.remove(0);
        if let Some(m) = self.re.search(&block, 0) {
            let id = pystr::lower(pystr::strip(m.str(1)));
            let link = pystr::rstrip_chars(pystr::lstrip_chars(m.str(2), "<"), ">").to_string();
            let title = match m.group(5) {
                Some(t) if !t.is_empty() => Some(t.to_string()),
                _ => m.group(6).filter(|t| !t.is_empty()).map(str::to_string),
            };
            md.references.insert(id, (link, title));
            let (start, end) = (m.start(0), m.end(0));
            if !pystr::strip(&block[end..]).is_empty() {
                blocks.insert(0, pystr::lstrip_chars(&block[end..], "\n").to_string());
            }
            if !pystr::strip(&block[..start]).is_empty() {
                blocks.insert(0, pystr::rstrip_chars(&block[..start], "\n").to_string());
            }
            return Ok(true);
        }
        blocks.insert(0, block);
        Ok(false)
    }
}

/// A paragraph: everything no other processor claims.
pub struct ParagraphProcessor;

impl BlockProcessor for ParagraphProcessor {
    fn test(&self, _md: &mut Markdown, _parent: El, _block: &str) -> bool {
        true
    }

    fn run(&self, md: &mut Markdown, parent: El, blocks: &mut Vec<String>) -> PyResult<bool> {
        let block = blocks.remove(0);
        if pystr::strip(&block).is_empty() {
            return Ok(true);
        }
        if md.is_state("list") {
            // In a tight list the text joins the item instead of becoming a paragraph.
            match last_child(md, parent) {
                Some(sibling) => {
                    let tail = match md.tree.tail_str(sibling).filter(|t| !t.is_empty()) {
                        Some(t) => format!("{t}\n{block}"),
                        None => format!("\n{block}"),
                    };
                    md.tree.set_tail_str(sibling, tail);
                }
                None => {
                    let text = match md.tree.text_str(parent).filter(|t| !t.is_empty()) {
                        Some(t) => format!("{t}\n{block}"),
                        None => pystr::lstrip(&block).to_string(),
                    };
                    md.tree.set_text_str(parent, text);
                }
            }
        } else {
            let p = md.tree.sub_element(parent, "p");
            md.tree.set_text_str(p, pystr::lstrip(&block));
        }
        Ok(true)
    }
}
