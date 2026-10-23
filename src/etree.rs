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

//! The element tree the parser builds: the subset of Python's `xml.etree.ElementTree` that Python-Markdown uses.
//!
//! Elements live in an arena owned by [`Tree`] and are referred to by [`El`] handles, which gives them the
//! identity semantics of Python objects: an element can be detached, moved to another parent, kept in a stash
//! and compared by identity. Text and tail values remember whether they are atomic (Python-Markdown's
//! `AtomicString`), i.e. excluded from further inline processing.

/// A handle to an element of a [`Tree`].
pub type El = usize;

/// The text or tail of an element.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Text {
    pub s: String,
    pub atomic: bool,
}

impl Text {
    /// A plain string.
    pub fn plain(s: impl Into<String>) -> Text {
        Text {
            s: s.into(),
            atomic: false,
        }
    }

    /// An `AtomicString`.
    pub fn atomic(s: impl Into<String>) -> Text {
        Text {
            s: s.into(),
            atomic: true,
        }
    }
}

/// One element.
#[derive(Clone, Debug, Default)]
pub struct Node {
    pub tag: String,
    /// Attributes in insertion order, as a Python dict keeps them.
    pub attrib: Vec<(String, String)>,
    pub text: Option<Text>,
    pub tail: Option<Text>,
    pub children: Vec<El>,
}

/// The arena of elements of one conversion.
#[derive(Default)]
pub struct Tree {
    nodes: Vec<Node>,
}

impl Tree {
    pub fn new() -> Tree {
        Tree::default()
    }

    /// `etree.Element(tag)`.
    pub fn element(&mut self, tag: &str) -> El {
        self.nodes.push(Node {
            tag: tag.to_string(),
            ..Node::default()
        });
        self.nodes.len() - 1
    }

    /// `etree.Element(tag, attrib)`.
    pub fn element_with(&mut self, tag: &str, attrib: Vec<(String, String)>) -> El {
        let el = self.element(tag);
        self.nodes[el].attrib = attrib;
        el
    }

    /// `etree.SubElement(parent, tag)`.
    pub fn sub_element(&mut self, parent: El, tag: &str) -> El {
        let el = self.element(tag);
        self.nodes[parent].children.push(el);
        el
    }

    pub fn node(&self, el: El) -> &Node {
        &self.nodes[el]
    }

    pub fn node_mut(&mut self, el: El) -> &mut Node {
        &mut self.nodes[el]
    }

    pub fn tag(&self, el: El) -> &str {
        &self.nodes[el].tag
    }

    pub fn set_tag(&mut self, el: El, tag: &str) {
        self.nodes[el].tag = tag.to_string();
    }

    pub fn children(&self, el: El) -> &[El] {
        &self.nodes[el].children
    }

    /// `len(el)`.
    pub fn len(&self, el: El) -> usize {
        self.nodes[el].children.len()
    }

    /// `el[-1]` if the element has children.
    pub fn last_child(&self, el: El) -> Option<El> {
        self.nodes[el].children.last().copied()
    }

    pub fn child(&self, el: El, index: usize) -> El {
        self.nodes[el].children[index]
    }

    pub fn text(&self, el: El) -> Option<&Text> {
        self.nodes[el].text.as_ref()
    }

    pub fn tail(&self, el: El) -> Option<&Text> {
        self.nodes[el].tail.as_ref()
    }

    /// The text as a string slice, `None` when unset.
    pub fn text_str(&self, el: El) -> Option<&str> {
        self.nodes[el].text.as_ref().map(|t| t.s.as_str())
    }

    /// The tail as a string slice, `None` when unset.
    pub fn tail_str(&self, el: El) -> Option<&str> {
        self.nodes[el].tail.as_ref().map(|t| t.s.as_str())
    }

    /// Whether the text is set and not empty (Python truthiness of `el.text`).
    pub fn has_text(&self, el: El) -> bool {
        self.text_str(el).is_some_and(|s| !s.is_empty())
    }

    /// Whether the tail is set and not empty.
    pub fn has_tail(&self, el: El) -> bool {
        self.tail_str(el).is_some_and(|s| !s.is_empty())
    }

    pub fn set_text(&mut self, el: El, text: Option<Text>) {
        self.nodes[el].text = text;
    }

    pub fn set_tail(&mut self, el: El, tail: Option<Text>) {
        self.nodes[el].tail = tail;
    }

    /// `el.text = s` with a plain string.
    pub fn set_text_str(&mut self, el: El, s: impl Into<String>) {
        self.nodes[el].text = Some(Text::plain(s));
    }

    /// `el.tail = s` with a plain string.
    pub fn set_tail_str(&mut self, el: El, s: impl Into<String>) {
        self.nodes[el].tail = Some(Text::plain(s));
    }

    /// `el.get(key)`.
    pub fn get(&self, el: El, key: &str) -> Option<&str> {
        self.nodes[el]
            .attrib
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }

    /// `el.set(key, value)`: an existing key keeps its position.
    pub fn set(&mut self, el: El, key: &str, value: impl Into<String>) {
        let value = value.into();
        let attrib = &mut self.nodes[el].attrib;
        match attrib.iter_mut().find(|(k, _)| k == key) {
            Some(slot) => slot.1 = value,
            None => attrib.push((key.to_string(), value)),
        }
    }

    /// `el.attrib.pop(key, None)`.
    pub fn remove_attr(&mut self, el: El, key: &str) -> Option<String> {
        let attrib = &mut self.nodes[el].attrib;
        let index = attrib.iter().position(|(k, _)| k == key)?;
        Some(attrib.remove(index).1)
    }

    pub fn attrib(&self, el: El) -> &[(String, String)] {
        &self.nodes[el].attrib
    }

    /// `parent.append(child)`.
    pub fn append(&mut self, parent: El, child: El) {
        self.nodes[parent].children.push(child);
    }

    /// `parent.insert(index, child)`, with Python's clamping of the index.
    pub fn insert(&mut self, parent: El, index: usize, child: El) {
        let children = &mut self.nodes[parent].children;
        let index = index.min(children.len());
        children.insert(index, child);
    }

    /// `parent.remove(child)`: the first occurrence, by identity.
    pub fn remove(&mut self, parent: El, child: El) {
        let children = &mut self.nodes[parent].children;
        if let Some(i) = children.iter().position(|&c| c == child) {
            children.remove(i);
        }
    }

    /// `list(parent).index(child)`.
    pub fn index_of(&self, parent: El, child: El) -> Option<usize> {
        self.nodes[parent].children.iter().position(|&c| c == child)
    }

    /// `parent[index] = child`.
    pub fn replace_child(&mut self, parent: El, index: usize, child: El) {
        self.nodes[parent].children[index] = child;
    }

    /// `el.iter()`: the element and its descendants in document order.
    pub fn iter(&self, el: El) -> Vec<El> {
        let mut out = Vec::new();
        let mut stack = vec![el];
        while let Some(e) = stack.pop() {
            out.push(e);
            for &c in self.nodes[e].children.iter().rev() {
                stack.push(c);
            }
        }
        out
    }

    /// `el.iter(tag)`.
    pub fn iter_tag(&self, el: El, tag: &str) -> Vec<El> {
        self.iter(el)
            .into_iter()
            .filter(|&e| self.nodes[e].tag == tag)
            .collect()
    }

    /// `''.join(el.itertext())`.
    pub fn itertext(&self, el: El) -> String {
        let mut out = String::new();
        self.itertext_into(el, &mut out);
        out
    }

    fn itertext_into(&self, el: El, out: &mut String) {
        if let Some(t) = &self.nodes[el].text {
            out.push_str(&t.s);
        }
        for &c in &self.nodes[el].children {
            self.itertext_into(c, out);
            if let Some(t) = &self.nodes[c].tail {
                out.push_str(&t.s);
            }
        }
    }

    /// The pieces of `el.itertext()` with their atomic flags.
    pub fn itertext_parts(&self, el: El) -> Vec<Text> {
        let mut out = Vec::new();
        self.itertext_parts_into(el, &mut out);
        out
    }

    fn itertext_parts_into(&self, el: El, out: &mut Vec<Text>) {
        if let Some(t) = &self.nodes[el].text {
            out.push(t.clone());
        }
        for &c in &self.nodes[el].children {
            self.itertext_parts_into(c, out);
            if let Some(t) = &self.nodes[c].tail {
                out.push(t.clone());
            }
        }
    }

    /// `copy.deepcopy(el)`.
    pub fn deepcopy(&mut self, el: El) -> El {
        let node = self.nodes[el].clone();
        let children: Vec<El> = node.children.iter().map(|&c| self.deepcopy(c)).collect();
        self.nodes.push(Node { children, ..node });
        self.nodes.len() - 1
    }

    /// `parent.find(tag)`: the first direct child with that tag.
    pub fn find_child(&self, parent: El, tag: &str) -> Option<El> {
        self.nodes[parent]
            .children
            .iter()
            .copied()
            .find(|&c| self.nodes[c].tag == tag)
    }
}
