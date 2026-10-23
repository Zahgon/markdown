// Add x/html serialization to `Elementree`
// Taken from ElementTree 1.3 preview with slight modifications
//
// Copyright (c) 1999-2007 by Fredrik Lundh.  All rights reserved.
//
// fredrik@pythonware.com
// https://www.pythonware.com/
//
// --------------------------------------------------------------------
// The ElementTree toolkit is
//
// Copyright (c) 1999-2007 by Fredrik Lundh
//
// By obtaining, using, and/or copying this software and/or its
// associated documentation, you agree that you have read, understood,
// and will comply with the following terms and conditions:
//
// Permission to use, copy, modify, and distribute this software and
// its associated documentation for any purpose and without fee is
// hereby granted, provided that the above copyright notice appears in
// all copies, and that both that copyright notice and this permission
// notice appear in supporting documentation, and that the name of
// Secret Labs AB or the author not be used in advertising or publicity
// pertaining to distribution of the software without specific, written
// prior permission.
//
// SECRET LABS AB AND THE AUTHOR DISCLAIMS ALL WARRANTIES WITH REGARD
// TO THIS SOFTWARE, INCLUDING ALL IMPLIED WARRANTIES OF MERCHANT-
// ABILITY AND FITNESS.  IN NO EVENT SHALL SECRET LABS AB OR THE AUTHOR
// BE LIABLE FOR ANY SPECIAL, INDIRECT OR CONSEQUENTIAL DAMAGES OR ANY
// DAMAGES WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS,
// WHETHER IN AN ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS
// ACTION, ARISING OUT OF OR IN CONNECTION WITH THE USE OR PERFORMANCE
// OF THIS SOFTWARE.
// --------------------------------------------------------------------

//! Serialize an element tree to HTML or XHTML text.
//!
//! This is Python-Markdown's own serializer (a modified copy of ElementTree's HTML writer): attributes are
//! written in sorted order, an attribute whose value equals its name is minimized in HTML output, an `&` that
//! already starts a character or entity reference is not escaped again, and XHTML closes void elements as
//! `<br />`. It also holds ElementTree's plain HTML writer, which `md_in_html` uses for void elements.

use crate::etree::{El, Tree};

/// The void elements of ElementTree's `HTML_EMPTY`.
pub const HTML_EMPTY: [&str; 17] = [
    "area", "base", "basefont", "br", "col", "embed", "frame", "hr", "img", "input", "isindex",
    "link", "meta", "param", "source", "track", "wbr",
];

fn is_html_empty(tag: &str) -> bool {
    HTML_EMPTY.contains(&crate::pystr::lower(tag).as_str())
}

fn is_ref_char(c: char) -> bool {
    // `[0-9a-z]` under re.IGNORECASE also matches the long s and the Kelvin sign.
    c.is_ascii_alphanumeric() || c == '\u{17f}' || c == '\u{212a}'
}

fn is_hex_char(c: char) -> bool {
    c.is_ascii_hexdigit()
}

/// Whether the `&` at the start of `rest` begins a reference that `RE_AMP` leaves alone.
fn starts_reference(rest: &str) -> bool {
    let after = &rest[1..];
    let run = |s: &str, pred: fn(char) -> bool| -> Option<usize> {
        let n: usize = s.chars().take_while(|&c| pred(c)).map(char::len_utf8).sum();
        (n > 0).then_some(n)
    };
    if let Some(digits) = after.strip_prefix('#') {
        if let Some(n) = run(digits, |c| c.is_ascii_digit()) {
            if digits[n..].starts_with(';') {
                return true;
            }
        }
        if let Some(hex) = digits.strip_prefix(['x', 'X']) {
            if let Some(n) = run(hex, is_hex_char) {
                if hex[n..].starts_with(';') {
                    return true;
                }
            }
        }
        return false;
    }
    match run(after, is_ref_char) {
        Some(n) => after[n..].starts_with(';'),
        None => false,
    }
}

fn escape_amp(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 8);
    for (i, c) in text.char_indices() {
        if c == '&' && !starts_reference(&text[i..]) {
            out.push_str("&amp;");
        } else {
            out.push(c);
        }
    }
    out
}

/// `_escape_cdata`: escape character data.
pub fn escape_cdata(text: &str) -> String {
    let mut text = if text.contains('&') {
        escape_amp(text)
    } else {
        text.to_string()
    };
    if text.contains('<') {
        text = text.replace('<', "&lt;");
    }
    if text.contains('>') {
        text = text.replace('>', "&gt;");
    }
    text
}

/// `_escape_attrib_html`: escape an attribute value.
pub fn escape_attrib_html(text: &str) -> String {
    let mut text = escape_cdata(text);
    if text.contains('"') {
        text = text.replace('"', "&quot;");
    }
    text
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Format {
    Html,
    Xhtml,
}

enum Item {
    El(El),
    Str(String),
}

fn serialize(tree: &Tree, root: El, format: Format) -> String {
    let mut out = String::new();
    let mut stack: std::collections::VecDeque<Item> = std::collections::VecDeque::new();
    stack.push_back(Item::El(root));
    while let Some(item) = stack.pop_front() {
        let el = match item {
            Item::Str(s) => {
                out.push_str(&s);
                continue;
            }
            Item::El(el) => el,
        };
        let node = tree.node(el);
        let tag = node.tag.as_str();
        out.push('<');
        out.push_str(tag);
        let mut items: Vec<&(String, String)> = node.attrib.iter().collect();
        items.sort();
        for (k, v) in items {
            let v = escape_attrib_html(v);
            if *k == v && format == Format::Html {
                out.push(' ');
                out.push_str(&v);
            } else {
                out.push_str(&format!(" {k}=\"{v}\""));
            }
        }
        let lower = crate::pystr::lower(tag);
        if format == Format::Xhtml && is_html_empty(tag) {
            out.push_str(" />");
        } else {
            out.push('>');
            if let Some(text) = node.text.as_ref().filter(|t| !t.s.is_empty()) {
                if lower == "script" || lower == "style" {
                    out.push_str(&text.s);
                } else {
                    out.push_str(&escape_cdata(&text.s));
                }
            }
            if let Some(tail) = node.tail.as_ref().filter(|t| !t.s.is_empty()) {
                stack.push_front(Item::Str(escape_cdata(&tail.s)));
            }
            if !is_html_empty(tag) {
                stack.push_front(Item::Str(format!("</{tag}>")));
            }
            for &child in node.children.iter().rev() {
                stack.push_front(Item::El(child));
            }
            continue;
        }
        if let Some(tail) = node.tail.as_ref().filter(|t| !t.s.is_empty()) {
            out.push_str(&escape_cdata(&tail.s));
        }
    }
    out
}

/// Serialize as HTML.
pub fn to_html_string(tree: &Tree, root: El) -> String {
    serialize(tree, root, Format::Html)
}

/// Serialize as XHTML.
pub fn to_xhtml_string(tree: &Tree, root: El) -> String {
    serialize(tree, root, Format::Xhtml)
}

fn et_escape_cdata(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn et_escape_attrib_html(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// `etree.tostring(el, encoding='unicode', method='html')`: ElementTree's own HTML writer, which keeps the
/// attribute order and minimizes nothing.
pub fn et_tostring_html(tree: &Tree, el: El) -> String {
    let mut out = String::new();
    et_serialize(tree, el, &mut out);
    out
}

fn et_serialize(tree: &Tree, el: El, out: &mut String) {
    let node = tree.node(el);
    out.push('<');
    out.push_str(&node.tag);
    for (k, v) in &node.attrib {
        out.push_str(&format!(" {k}=\"{}\"", et_escape_attrib_html(v)));
    }
    out.push('>');
    let lower = crate::pystr::lower(&node.tag);
    if let Some(text) = node.text.as_ref().filter(|t| !t.s.is_empty()) {
        if lower == "script" || lower == "style" {
            out.push_str(&text.s);
        } else {
            out.push_str(&et_escape_cdata(&text.s));
        }
    }
    for &child in &node.children {
        et_serialize(tree, child, out);
    }
    if !HTML_EMPTY.contains(&lower.as_str()) {
        out.push_str(&format!("</{}>", node.tag));
    }
    if let Some(tail) = node.tail.as_ref().filter(|t| !t.s.is_empty()) {
        out.push_str(&et_escape_cdata(&tail.s));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ampersands_before_references_are_kept() {
        assert_eq!(
            escape_cdata("&amp; & &#38; &#x26; &x"),
            "&amp; &amp; &#38; &#x26; &amp;x"
        );
    }

    #[test]
    fn attributes_sorted_and_minimized() {
        let mut tree = Tree::new();
        let el = tree.element("input");
        tree.set(el, "type", "checkbox");
        tree.set(el, "checked", "checked");
        assert_eq!(
            to_html_string(&tree, el),
            "<input checked type=\"checkbox\">"
        );
        assert_eq!(
            to_xhtml_string(&tree, el),
            "<input checked=\"checked\" type=\"checkbox\" />"
        );
    }
}
