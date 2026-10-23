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

//! Tree processors run on the element tree after block parsing: inline patterns, prettifying and unescaping.

use std::any::Any;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::OnceLock;

use crate::core::{InlinePattern, Markdown, PyResult, Treeprocessor};
use crate::etree::{El, Text};
use crate::inlinepatterns::InlineNode;
use crate::pyre::Regex;
use crate::pystr;
use crate::util::{Registry, INLINE_PLACEHOLDER_PREFIX};

/// Register the default tree processors.
pub fn build_treeprocessors(md: &mut Markdown) {
    md.treeprocessors
        .register(Rc::new(InlineProcessor::new(None)), "inline", 20.0);
    md.treeprocessors
        .register(Rc::new(PrettifyTreeprocessor), "prettify", 10.0);
    md.treeprocessors
        .register(Rc::new(UnescapeTreeprocessor), "unescape", 0.0);
}

/// A node kept in the inline stash: an element or a string.
#[derive(Clone, Debug)]
pub enum StashNode {
    El(El),
    Str(String),
}

fn placeholder_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(crate::util::INLINE_PLACEHOLDER_RE, 0))
}

/// Apply inline patterns to the text of every element. Matched text is replaced by placeholders for stashed
/// nodes, which are then expanded back into the tree.
pub struct InlineProcessor {
    /// Its own patterns (smarty), or `None` for the converter's inline patterns.
    patterns: Option<Registry<Rc<dyn InlinePattern>>>,
    stashed_nodes: RefCell<HashMap<String, StashNode>>,
}

impl InlineProcessor {
    pub fn new(patterns: Option<Registry<Rc<dyn InlinePattern>>>) -> InlineProcessor {
        InlineProcessor {
            patterns,
            stashed_nodes: RefCell::new(HashMap::new()),
        }
    }

    /// The stashed node of an id.
    pub fn stashed(&self, id: &str) -> Option<StashNode> {
        self.stashed_nodes.borrow().get(id).cloned()
    }

    fn patterns(&self, md: &Markdown) -> Vec<Rc<dyn InlinePattern>> {
        match &self.patterns {
            Some(p) => p.items(),
            None => md.inline_patterns.items(),
        }
    }

    fn stash_node(&self, node: StashNode) -> String {
        let mut stash = self.stashed_nodes.borrow_mut();
        let id = format!("{:04}", stash.len());
        let placeholder = crate::util::inline_placeholder(&id);
        stash.insert(id, node);
        placeholder
    }

    /// `__handleInline`: run the patterns from `pattern_index` on, unless the text is atomic.
    fn handle_inline(&self, md: &mut Markdown, data: Text, pattern_index: usize) -> PyResult<Text> {
        if data.atomic {
            return Ok(data);
        }
        let patterns = self.patterns(md);
        let mut data = data;
        let mut start_index = 0;
        let mut index = pattern_index;
        while index < patterns.len() {
            let (new_data, matched, new_start) =
                self.apply_pattern(md, &patterns[index], data, index, start_index)?;
            data = new_data;
            start_index = new_start;
            if !matched {
                index += 1;
            }
        }
        Ok(data)
    }

    /// `__applyPattern`: apply one pattern at the first match from `start_index` it accepts.
    fn apply_pattern(
        &self,
        md: &mut Markdown,
        pattern: &Rc<dyn InlinePattern>,
        data: Text,
        pattern_index: usize,
        start_index: usize,
    ) -> PyResult<(Text, bool, usize)> {
        let re = pattern.regex();
        let mut found = None;
        for m in re.finditer(&data.s, start_index) {
            let (node, start, end) = pattern.handle_match(md, &m, &data.s)?;
            if let (Some(start), Some(end)) = (start, end) {
                found = Some((node, start, end));
                break;
            }
        }
        let Some((node, start, end)) = found else {
            return Ok((data, false, 0));
        };
        let Some(node) = node else {
            return Ok((data, true, end));
        };
        let stash = match node {
            InlineNode::Str(s) => StashNode::Str(s),
            InlineNode::El(el) => {
                if !md.tree.text(el).is_some_and(|t| t.atomic) {
                    let mut family = vec![el];
                    family.extend_from_slice(md.tree.children(el));
                    for child in family {
                        if let Some(text) = md.tree.text(child).filter(|t| !t.s.is_empty()).cloned()
                        {
                            let new = self.handle_inline(md, text, pattern_index + 1)?;
                            md.tree.set_text(child, Some(new));
                        }
                        if let Some(tail) = md.tree.tail(child).filter(|t| !t.s.is_empty()).cloned()
                        {
                            let new = self.handle_inline(md, tail, pattern_index)?;
                            md.tree.set_tail(child, Some(new));
                        }
                    }
                }
                StashNode::El(el)
            }
        };
        let placeholder = self.stash_node(stash);
        let new = format!("{}{}{}", &data.s[..start], placeholder, &data.s[end..]);
        Ok((Text::plain(new), true, start + placeholder.len()))
    }

    /// `__processElementText`: expand the placeholders of an element's text (or tail) in place.
    fn process_element_text(&self, md: &mut Markdown, node: El, subnode: El, is_text: bool) {
        let text = if is_text {
            md.tree.node_mut(subnode).text.take()
        } else {
            md.tree.node_mut(subnode).tail.take()
        };
        let mut child_result = self.process_placeholders(md, text, subnode, is_text);
        let pos = if !is_text && node != subnode {
            md.tree.index_of(node, subnode).map_or(0, |i| i + 1)
        } else {
            0
        };
        child_result.reverse();
        for new_child in child_result {
            md.tree.insert(node, pos, new_child);
        }
    }

    /// `__processPlaceholders`: split text at placeholders, linking plain text to the parent (or to the
    /// previous node's tail) and returning the stashed elements in order.
    fn process_placeholders(
        &self,
        md: &mut Markdown,
        data: Option<Text>,
        parent: El,
        is_text: bool,
    ) -> Vec<El> {
        let mut result: Vec<El> = Vec::new();
        let Some(data) = data.filter(|d| !d.s.is_empty()) else {
            return result;
        };
        let link_text = |md: &mut Markdown, result: &[El], text: Text| {
            if text.s.is_empty() {
                return;
            }
            let (target, is_tail) = match result.last() {
                Some(&last) => (last, true),
                None => (parent, !is_text),
            };
            let current = if is_tail {
                md.tree.tail(target)
            } else {
                md.tree.text(target)
            };
            let new = match current.filter(|t| !t.s.is_empty()) {
                Some(cur) => Text::plain(format!("{}{}", cur.s, text.s)),
                None => text,
            };
            if is_tail {
                md.tree.set_tail(target, Some(new));
            } else {
                md.tree.set_text(target, Some(new));
            }
        };
        let s = data.s.as_str();
        let mut start_index = 0;
        loop {
            match s[start_index..]
                .find(INLINE_PLACEHOLDER_PREFIX)
                .map(|i| i + start_index)
            {
                Some(index) => {
                    let (id, ph_end) = match placeholder_re().search(s, index) {
                        Some(m) => (Some(m.str(1).to_string()), m.end(0)),
                        None => (None, index + 1),
                    };
                    let node = id.as_deref().and_then(|id| self.stashed(id));
                    match node {
                        Some(node) => {
                            if index > 0 {
                                link_text(md, &result, Text::plain(&s[start_index..index]));
                            }
                            match node {
                                StashNode::El(el) => {
                                    let mut family = vec![el];
                                    family.extend_from_slice(md.tree.children(el));
                                    for child in family {
                                        if md
                                            .tree
                                            .tail_str(child)
                                            .is_some_and(|t| !pystr::strip(t).is_empty())
                                        {
                                            self.process_element_text(md, el, child, false);
                                        }
                                        if md
                                            .tree
                                            .text_str(child)
                                            .is_some_and(|t| !pystr::strip(t).is_empty())
                                        {
                                            self.process_element_text(md, child, child, true);
                                        }
                                    }
                                    start_index = ph_end;
                                    result.push(el);
                                }
                                StashNode::Str(text) => {
                                    link_text(md, &result, Text::plain(text));
                                    start_index = ph_end;
                                }
                            }
                        }
                        None => {
                            let end = index + INLINE_PLACEHOLDER_PREFIX.len();
                            link_text(md, &result, Text::plain(&s[start_index..end]));
                            start_index = end;
                        }
                    }
                }
                None => {
                    let text = Text {
                        s: s[start_index..].to_string(),
                        atomic: data.atomic,
                    };
                    link_text(md, &result, text);
                    break;
                }
            }
        }
        result
    }
}

impl Treeprocessor for InlineProcessor {
    fn run(&self, md: &mut Markdown, root: El) -> PyResult<()> {
        self.stashed_nodes.borrow_mut().clear();
        let mut stack: std::collections::VecDeque<El> = std::collections::VecDeque::new();
        stack.push_back(root);
        while let Some(current) = stack.pop_front() {
            let mut insert_queue: Vec<(El, Vec<El>)> = Vec::new();
            // The children are visited by index: nodes inserted after the current child are visited too.
            let mut i = 0;
            while i < md.tree.len(current) {
                let child = md.tree.child(current, i);
                if let Some(text) = md
                    .tree
                    .text(child)
                    .filter(|t| !t.s.is_empty() && !t.atomic)
                    .cloned()
                {
                    md.tree.set_text(child, None);
                    let handled = self.handle_inline(md, text, 0)?;
                    let lst = self.process_placeholders(md, Some(handled), child, true);
                    stack.extend(lst.iter().copied());
                    insert_queue.push((child, lst));
                }
                if let Some(tail) = md.tree.tail(child).filter(|t| !t.s.is_empty()).cloned() {
                    let tail = self.handle_inline(md, tail, 0)?;
                    let dumby = md.tree.element("d");
                    md.tree.set_tail(child, None);
                    let mut tail_result = self.process_placeholders(md, Some(tail), dumby, false);
                    if md.tree.has_tail(dumby) {
                        let t = md.tree.tail(dumby).cloned();
                        md.tree.set_tail(child, t);
                    }
                    let pos = md.tree.index_of(current, child).map_or(0, |p| p + 1);
                    tail_result.reverse();
                    for new_child in tail_result {
                        md.tree.insert(current, pos, new_child);
                    }
                }
                if md.tree.len(child) > 0 {
                    stack.push_back(child);
                }
                i += 1;
            }
            for (element, lst) in insert_queue {
                for (i, new_child) in lst.into_iter().enumerate() {
                    md.tree.insert(element, i, new_child);
                }
            }
        }
        Ok(())
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Add line breaks to the serialized HTML: after block level elements and `<br>`, and at the end of code.
pub struct PrettifyTreeprocessor;

impl PrettifyTreeprocessor {
    fn prettify(md: &mut Markdown, elem: El) {
        let i = "\n";
        let tag = md.tree.tag(elem).to_string();
        if md.is_block_level(&tag) && tag != "code" && tag != "pre" {
            let blank_text = md
                .tree
                .text_str(elem)
                .is_none_or(|t| pystr::strip(t).is_empty());
            if blank_text
                && md.tree.len(elem) > 0
                && md.is_block_level(md.tree.tag(md.tree.child(elem, 0)))
            {
                md.tree.set_text_str(elem, i);
            }
            for e in md.tree.children(elem).to_vec() {
                if md.is_block_level(md.tree.tag(e)) {
                    Self::prettify(md, e);
                }
            }
        }
        if md
            .tree
            .tail_str(elem)
            .is_none_or(|t| pystr::strip(t).is_empty())
        {
            md.tree.set_tail_str(elem, i);
        }
    }

    /// Prettify a tree (also used on the table of contents).
    pub fn prettify_tree(md: &mut Markdown, root: El) {
        Self::prettify(md, root);
        for br in md.tree.iter_tag(root, "br") {
            let tail = match md.tree.tail_str(br).filter(|t| !pystr::strip(t).is_empty()) {
                Some(t) => format!("\n{t}"),
                None => "\n".to_string(),
            };
            md.tree.set_tail_str(br, tail);
        }
        for pre in md.tree.iter_tag(root, "pre") {
            if md.tree.len(pre) > 0 && md.tree.tag(md.tree.child(pre, 0)) == "code" {
                let code = md.tree.child(pre, 0);
                if md.tree.len(code) == 0 {
                    if let Some(text) = md.tree.text_str(code) {
                        let text = format!("{}\n", pystr::rstrip(text));
                        md.tree.set_text(code, Some(Text::atomic(text)));
                    }
                }
            }
        }
    }
}

impl Treeprocessor for PrettifyTreeprocessor {
    fn run(&self, md: &mut Markdown, root: El) -> PyResult<()> {
        Self::prettify_tree(md, root);
        Ok(())
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Replace escape placeholders (`STX` code point `ETX`) with their characters.
pub fn unescape_escapes(text: &str) -> String {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| Regex::new("\u{2}(\\d+)\u{3}", 0));
    re.sub_fn(text, |m| {
        let n: u32 = m
            .str(1)
            .chars()
            .filter_map(pystr::decimal_value)
            .fold(0u32, |acc, d| acc.saturating_mul(10).saturating_add(d));
        char::from_u32(n).map(String::from).unwrap_or_default()
    })
}

/// Restore backslash escaped characters.
pub struct UnescapeTreeprocessor;

impl Treeprocessor for UnescapeTreeprocessor {
    fn run(&self, md: &mut Markdown, root: El) -> PyResult<()> {
        for elem in md.tree.iter(root) {
            if md.tree.has_text(elem) && md.tree.tag(elem) != "code" {
                let text = unescape_escapes(md.tree.text_str(elem).unwrap_or(""));
                md.tree.set_text_str(elem, text);
            }
            if md.tree.has_tail(elem) {
                let tail = unescape_escapes(md.tree.tail_str(elem).unwrap_or(""));
                md.tree.set_tail_str(elem, tail);
            }
            let attrib: Vec<(String, String)> = md.tree.attrib(elem).to_vec();
            for (k, v) in attrib {
                md.tree.set(elem, &k, unescape_escapes(&v));
            }
        }
        Ok(())
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}
