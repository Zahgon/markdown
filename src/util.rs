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

//! Shared constants and helpers: placeholders, the HTML stash, the priority registry and value parsing.

use crate::etree::El;

/// Block level elements: their raw HTML is kept as a block and they never get wrapped in a paragraph.
pub const BLOCK_LEVEL_ELEMENTS: [&str; 59] = [
    "address",
    "article",
    "aside",
    "blockquote",
    "details",
    "div",
    "dl",
    "fieldset",
    "figcaption",
    "figure",
    "footer",
    "form",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "header",
    "hgroup",
    "hr",
    "main",
    "menu",
    "nav",
    "ol",
    "p",
    "pre",
    "section",
    "table",
    "ul",
    "canvas",
    "colgroup",
    "dd",
    "body",
    "dt",
    "group",
    "html",
    "iframe",
    "li",
    "legend",
    "math",
    "map",
    "noscript",
    "output",
    "object",
    "option",
    "progress",
    "script",
    "style",
    "summary",
    "tbody",
    "td",
    "textarea",
    "tfoot",
    "th",
    "thead",
    "tr",
    "video",
    "center",
];

/// "Start of Text" marker for placeholder templates.
pub const STX: char = '\u{2}';
/// "End of Text" marker for placeholder templates.
pub const ETX: char = '\u{3}';
/// Prefix of inline placeholders.
pub const INLINE_PLACEHOLDER_PREFIX: &str = "\u{2}klzzwxh:";
/// Placeholder for an `&` that must survive escaping.
pub const AMP_SUBSTITUTE: &str = "\u{2}amp\u{3}";

/// The inline placeholder for a stash id.
pub fn inline_placeholder(id: &str) -> String {
    format!("{INLINE_PLACEHOLDER_PREFIX}{id}{ETX}")
}

/// The raw HTML placeholder for a stash index.
pub fn html_placeholder(key: usize) -> String {
    format!("{STX}wzxhzdk:{key}{ETX}")
}

/// The pattern matching raw HTML placeholders (group 1 is the index).
pub const HTML_PLACEHOLDER_RE: &str = "\u{2}wzxhzdk:([0-9]+)\u{3}";
/// The pattern matching inline placeholders (group 1 is the id).
pub const INLINE_PLACEHOLDER_RE: &str = "\u{2}klzzwxh:([0-9]+)\u{3}";

/// The default block level list as owned strings.
pub fn block_level_elements() -> Vec<String> {
    BLOCK_LEVEL_ELEMENTS.iter().map(|s| s.to_string()).collect()
}

/// A stashed piece of raw HTML: a string, or an element `md_in_html` serializes later.
#[derive(Clone, Debug)]
pub enum Stashed {
    Str(String),
    El(El),
}

/// Raw HTML extracted from the document and replaced with placeholders.
#[derive(Default)]
pub struct HtmlStash {
    pub html_counter: usize,
    pub raw_html_blocks: Vec<Stashed>,
}

impl HtmlStash {
    /// Store a segment and return its placeholder.
    pub fn store(&mut self, html: Stashed) -> String {
        self.raw_html_blocks.push(html);
        let placeholder = html_placeholder(self.html_counter);
        self.html_counter += 1;
        placeholder
    }

    /// Store a string segment and return its placeholder.
    pub fn store_str(&mut self, html: impl Into<String>) -> String {
        self.store(Stashed::Str(html.into()))
    }

    pub fn reset(&mut self) {
        self.html_counter = 0;
        self.raw_html_blocks.clear();
    }
}

/// A priority sorted registry: items are kept sorted by priority, highest first, and equal priorities keep
/// their registration order. Registering an existing name replaces the item and moves it behind its peers.
pub struct Registry<T: Clone> {
    items: Vec<(String, f64, T)>,
}

impl<T: Clone> Default for Registry<T> {
    fn default() -> Self {
        Registry { items: Vec::new() }
    }
}

impl<T: Clone> Registry<T> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, item: T, name: &str, priority: f64) {
        self.deregister(name);
        self.items.push((name.to_string(), priority, item));
        // A stable sort keeps registration order among equal priorities.
        self.items
            .sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    }

    pub fn deregister(&mut self, name: &str) {
        self.items.retain(|(n, _, _)| n != name);
    }

    pub fn contains(&self, name: &str) -> bool {
        self.items.iter().any(|(n, _, _)| n == name)
    }

    pub fn get(&self, name: &str) -> Option<&T> {
        self.items
            .iter()
            .find(|(n, _, _)| n == name)
            .map(|(_, _, t)| t)
    }

    /// The items in priority order.
    pub fn items(&self) -> Vec<T> {
        self.items.iter().map(|(_, _, t)| t.clone()).collect()
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// The item at `index` in priority order.
    pub fn at(&self, index: usize) -> T {
        self.items[index].2.clone()
    }
}

/// `util.code_escape`: HTML escape a string of code.
pub fn code_escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}
