// Table of Contents Extension for Python-Markdown
// ===============================================
//
// See https://Python-Markdown.github.io/extensions/toc
// for documentation.
//
// Original code Copyright 2008 [Jack Miller](https://codezen.org/)
//
// All changes Copyright 2008-2024 The Python Markdown Project
//
// License: [BSD](https://opensource.org/licenses/bsd-license.php)

//! Table of contents: header ids (slugs), optional anchor and permalink links, and a nested list of the
//! headers that replaces the `[TOC]` marker.

use std::any::Any;
use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;
use std::sync::OnceLock;

use super::Config;
use crate::core::{Extension, Markdown, PyError, PyResult, Treeprocessor};
use crate::etree::El;
use crate::pyre::Regex;
use crate::pystr;
use crate::treeprocessors::{unescape_escapes, PrettifyTreeprocessor};
use crate::unicode_tables::NFKD_ASCII;
use crate::util::AMP_SUBSTITUTE;
use crate::value::{parse_bool_value, Value};

pub struct TocExtension {
    config: Config,
}

impl TocExtension {
    pub fn create(configs: &[(String, Value)]) -> PyResult<Rc<dyn Extension>> {
        let mut config = Config::new(&[
            ("marker", Value::Str("[TOC]".into())),
            ("title", Value::Str(String::new())),
            ("title_class", Value::Str("toctitle".into())),
            ("toc_class", Value::Str("toc".into())),
            ("anchorlink", Value::Bool(false)),
            ("anchorlink_class", Value::Str("toclink".into())),
            ("permalink", Value::Int(0)),
            ("permalink_class", Value::Str("headerlink".into())),
            ("permalink_title", Value::Str("Permanent link".into())),
            ("permalink_leading", Value::Bool(false)),
            ("baselevel", Value::Str("1".into())),
            ("slugify", Value::Callable("slugify")),
            ("separator", Value::Str("-".into())),
            ("toc_depth", Value::Int(6)),
        ]);
        config.set_all(configs)?;
        Ok(Rc::new(TocExtension { config }))
    }
}

impl Extension for TocExtension {
    fn extend_markdown(&self, md: &mut Markdown, this: Rc<dyn Extension>) -> PyResult<()> {
        md.register_extension(this);
        let processor = TocTreeprocessor::new(&self.config)?;
        md.treeprocessors.register(Rc::new(processor), "toc", 5.0);
        Ok(())
    }

    fn class_path(&self) -> &'static str {
        "markdown.extensions.toc.TocExtension"
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// `int(value)` for a configuration value.
fn py_int(value: &Value) -> PyResult<i64> {
    match value {
        Value::Str(s) => pystr::parse_int(s).map_err(|e| PyError::new("ValueError", e)),
        Value::Int(n) => Ok(*n as i64),
        Value::Bool(b) => Ok(i64::from(*b)),
        Value::Float(f) if f.is_nan() => Err(PyError::new(
            "ValueError",
            "cannot convert float NaN to integer",
        )),
        Value::Float(f) if f.is_infinite() => Err(PyError::new(
            "OverflowError",
            "cannot convert float infinity to integer",
        )),
        Value::Float(f) => Ok(f.trunc() as i64),
        other => Err(PyError::new(
            "TypeError",
            format!(
                "int() argument must be a string, a bytes-like object or a real number, not '{}'",
                other.type_name()
            ),
        )),
    }
}

/// The ASCII characters NFKD normalization leaves of a character.
fn ascii_fold(c: char) -> Option<&'static str> {
    let cp = c as u32;
    NFKD_ASCII
        .binary_search_by_key(&cp, |&(k, _)| k)
        .ok()
        .map(|i| NFKD_ASCII[i].1)
}

/// `slugify`: an ASCII, lower case slug with words joined by the separator.
pub fn slugify(value: &str, separator: &str) -> String {
    static NOT_WORD: OnceLock<Regex> = OnceLock::new();
    let mut ascii = String::with_capacity(value.len());
    for c in value.chars() {
        if c.is_ascii() {
            ascii.push(c);
        } else if let Some(folded) = ascii_fold(c) {
            ascii.push_str(folded);
        }
    }
    let cleaned = NOT_WORD
        .get_or_init(|| Regex::new(r"[^\w\s-]", 0))
        .sub(&ascii, "");
    let lowered = pystr::lower(pystr::strip(&cleaned));
    Regex::new(&format!(r"[{separator}\s]+"), 0).sub(&lowered, separator)
}

/// `unique`: make an id unique among `ids` with an `_<n>` suffix.
fn unique(id: &str, ids: &mut HashSet<String>) -> String {
    static IDCOUNT: OnceLock<Regex> = OnceLock::new();
    let re = IDCOUNT.get_or_init(|| Regex::new(r"^(.*)_([0-9]+)$", 0));
    let mut id = id.to_string();
    while ids.contains(&id) || id.is_empty() {
        id = match re.match_at(&id, 0) {
            Some(m) => format!(
                "{}_{}",
                m.str(1),
                pystr::parse_int(m.str(2)).unwrap_or(0) + 1
            ),
            None => format!("{id}_1"),
        };
    }
    ids.insert(id.clone());
    id
}

/// `strip_tags`: remove comments and tags and collapse whitespace.
fn strip_tags(text: &str) -> String {
    let mut text = text.to_string();
    while let Some(start) = text.find("<!--") {
        let Some(end) = text[start..].find("-->").map(|e| e + start) else {
            break;
        };
        text = format!("{}{}", &text[..start], &text[end + 3..]);
    }
    while let Some(start) = text.find('<') {
        let Some(end) = text[start..].find('>').map(|e| e + start) else {
            break;
        };
        text = format!("{}{}", &text[..start], &text[end + 1..]);
    }
    pystr::split_whitespace(&text).join(" ")
}

fn run_postprocessors(md: &mut Markdown, text: String) -> PyResult<String> {
    let mut text = text;
    for pp in md.postprocessors.items() {
        text = pp.run(md, text)?;
    }
    Ok(pystr::strip(&text).to_string())
}

/// The HTML inside an element, with postprocessors applied.
fn render_inner_html(md: &mut Markdown, el: El) -> PyResult<String> {
    let text = unescape_escapes(&md.serialize(el));
    let start = text.find('>').map_or(0, |i| i + 1);
    let end = text.rfind('<').unwrap_or(text.len());
    let inner = if start < end {
        pystr::strip(&text[start..end]).to_string()
    } else {
        String::new()
    };
    run_postprocessors(md, inner)
}

/// A copy of a header without its footnote references (their tails are kept).
fn remove_fnrefs(md: &mut Markdown, root: El) -> El {
    if md.tree.iter_tag(root, "sup").is_empty() {
        return root;
    }
    let root = md.tree.deepcopy(root);
    let mut parents: Vec<El> = Vec::new();
    for el in md.tree.iter(root) {
        for &child in md.tree.children(el) {
            if md.tree.tag(child) == "sup" && !parents.contains(&el) {
                parents.push(el);
            }
        }
    }
    // `findall('.//sup/..')` yields the parents in the document order of their `sup` children.
    let order: Vec<El> = md
        .tree
        .iter(root)
        .into_iter()
        .filter(|&e| md.tree.tag(e) == "sup")
        .collect();
    let mut ordered: Vec<El> = Vec::new();
    for sup in order {
        if let Some(&p) = parents
            .iter()
            .find(|&&p| md.tree.children(p).contains(&sup))
        {
            if !ordered.contains(&p) {
                ordered.push(p);
            }
        }
    }
    for parent in ordered {
        let mut carry = String::new();
        for child in md.tree.children(parent).to_vec().into_iter().rev() {
            let is_fnref = md.tree.tag(child) == "sup"
                && md.tree.get(child, "id").unwrap_or("").starts_with("fnref");
            if is_fnref {
                carry = format!("{}{carry}", md.tree.tail_str(child).unwrap_or(""));
                md.tree.remove(parent, child);
            } else if !carry.is_empty() {
                let tail = format!("{}{carry}", md.tree.tail_str(child).unwrap_or(""));
                md.tree.set_tail_str(child, tail);
                carry.clear();
            }
        }
        if !carry.is_empty() {
            let text = format!("{}{carry}", md.tree.text_str(parent).unwrap_or(""));
            md.tree.set_text_str(parent, text);
        }
    }
    root
}

/// A table of contents entry.
struct Token {
    level: i64,
    id: String,
    name: String,
    children: Vec<Rc<RefCell<Token>>>,
}

/// `nest_toc_tokens`: nest the flat list of headers by level.
fn nest_toc_tokens(toc_list: Vec<Rc<RefCell<Token>>>) -> Vec<Rc<RefCell<Token>>> {
    let mut ordered: Vec<Rc<RefCell<Token>>> = Vec::new();
    let mut toc_list: std::collections::VecDeque<Rc<RefCell<Token>>> = toc_list.into();
    let Some(mut last) = toc_list.pop_front() else {
        return ordered;
    };
    let mut levels = vec![last.borrow().level];
    ordered.push(last.clone());
    let mut parents: Vec<Rc<RefCell<Token>>> = Vec::new();
    while let Some(t) = toc_list.pop_front() {
        let current = t.borrow().level;
        if current < *levels.last().expect("a level") {
            levels.pop();
            let mut to_pop = 0;
            for p in parents.iter().rev() {
                if current <= p.borrow().level {
                    to_pop += 1;
                } else {
                    break;
                }
            }
            if to_pop > 0 {
                levels.truncate(levels.len().saturating_sub(to_pop));
                parents.truncate(parents.len().saturating_sub(to_pop));
            }
            levels.push(current);
        }
        if current == *levels.last().expect("a level") {
            match parents.last() {
                Some(p) => p.borrow_mut().children.push(t.clone()),
                None => ordered.push(t.clone()),
            }
        } else {
            last.borrow_mut().children.push(t.clone());
            parents.push(last.clone());
            levels.push(current);
        }
        last = t;
    }
    ordered
}

/// How permalinks are added: not at all, with the default `&para;` text, or with this text.
enum Permalink {
    Off,
    Default,
    Text(Value),
}

/// Build the table of contents and give headers their ids.
pub struct TocTreeprocessor {
    marker: Value,
    title: Value,
    base_level: i64,
    slugify: Value,
    sep: String,
    toc_class: Value,
    title_class: Value,
    use_anchors: bool,
    anchorlink_class: Value,
    permalink: Permalink,
    permalink_class: Value,
    permalink_title: Value,
    permalink_leading: bool,
    toc_top: i64,
    toc_bottom: i64,
    header_rgx: Regex,
}

impl TocTreeprocessor {
    fn new(config: &Config) -> PyResult<TocTreeprocessor> {
        let base_level = py_int(config.get("baselevel"))? - 1;
        let use_anchors = parse_bool_value(config.get("anchorlink"), true, false)
            .map_err(|e| PyError::new("ValueError", e))?
            .unwrap_or(false);
        let permalink = match parse_bool_value(config.get("permalink"), false, false) {
            Ok(Some(true)) => Permalink::Default,
            Ok(Some(false)) => Permalink::Off,
            _ => match config.get("permalink") {
                Value::None => Permalink::Off,
                v => Permalink::Text(v.clone()),
            },
        };
        let permalink_leading = matches!(
            parse_bool_value(config.get("permalink_leading"), false, false),
            Ok(Some(true))
        );
        let depth = config.get("toc_depth");
        let (toc_top, toc_bottom) = match depth {
            Value::Str(s) if s.contains('-') => {
                let parts: Vec<i64> = s
                    .split('-')
                    .map(|x| pystr::parse_int(x).map_err(|e| PyError::new("ValueError", e)))
                    .collect::<PyResult<_>>()?;
                match parts.as_slice() {
                    [top, bottom] => (*top, *bottom),
                    _ if parts.len() > 2 => {
                        return Err(PyError::new(
                            "ValueError",
                            "too many values to unpack (expected 2)",
                        ))
                    }
                    _ => {
                        return Err(PyError::new(
                            "ValueError",
                            format!(
                                "not enough values to unpack (expected 2, got {})",
                                parts.len()
                            ),
                        ))
                    }
                }
            }
            other => (1, py_int(other)?),
        };
        Ok(TocTreeprocessor {
            marker: config.get("marker").clone(),
            title: config.get("title").clone(),
            base_level,
            slugify: config.get("slugify").clone(),
            sep: config.get_str("separator"),
            toc_class: config.get("toc_class").clone(),
            title_class: config.get("title_class").clone(),
            use_anchors,
            anchorlink_class: config.get("anchorlink_class").clone(),
            permalink,
            permalink_class: config.get("permalink_class").clone(),
            permalink_title: config.get("permalink_title").clone(),
            permalink_leading,
            toc_top,
            toc_bottom,
            header_rgx: Regex::new("[Hh][123456]", 0),
        })
    }

    fn is_header(&self, tag: &str) -> bool {
        self.header_rgx.match_at(tag, 0).is_some()
    }

    fn slug(&self, value: &str) -> PyResult<String> {
        match &self.slugify {
            Value::Callable(_) => Ok(slugify(value, &self.sep)),
            other => Err(PyError::new(
                "TypeError",
                format!("'{}' object is not callable", other.type_name()),
            )),
        }
    }

    fn set_level(&self, md: &mut Markdown, elem: El) {
        let tag = md.tree.tag(elem).to_string();
        let last = tag
            .chars()
            .last()
            .and_then(pystr::decimal_value)
            .unwrap_or(0) as i64;
        let level = (last + self.base_level).min(6);
        md.tree.set_tag(elem, &format!("h{level}"));
    }

    fn add_anchor(&self, md: &mut Markdown, c: El, elem_id: &str) {
        let anchor = md.tree.element("a");
        let text = md.tree.text(c).cloned();
        md.tree.set_text(anchor, text);
        md.tree.set(anchor, "href", format!("#{elem_id}"));
        md.tree.set(anchor, "class", self.anchorlink_class.py_str());
        md.tree.set_text_str(c, "");
        for el in md.tree.children(c).to_vec() {
            md.tree.append(anchor, el);
        }
        md.tree.node_mut(c).children.clear();
        md.tree.append(c, anchor);
    }

    fn add_permalink(&self, md: &mut Markdown, c: El, elem_id: &str) {
        let permalink = md.tree.element("a");
        let text = match &self.permalink {
            Permalink::Text(v) => v.py_str(),
            _ => format!("{AMP_SUBSTITUTE}para;"),
        };
        md.tree.set_text_str(permalink, text);
        md.tree.set(permalink, "href", format!("#{elem_id}"));
        md.tree
            .set(permalink, "class", self.permalink_class.py_str());
        if self.permalink_title.truthy() {
            md.tree
                .set(permalink, "title", self.permalink_title.py_str());
        }
        if self.permalink_leading {
            let text = md.tree.text(c).cloned();
            md.tree.set_tail(permalink, text);
            md.tree.set_text_str(c, "");
            md.tree.insert(c, 0, permalink);
        } else {
            md.tree.append(c, permalink);
        }
    }

    fn build_toc_div(&self, md: &mut Markdown, toc_list: &[Rc<RefCell<Token>>]) -> El {
        let div = md.tree.element("div");
        md.tree.set(div, "class", self.toc_class.py_str());
        if self.title.truthy() {
            let header = md.tree.sub_element(div, "span");
            if self.title_class.truthy() {
                md.tree.set(header, "class", self.title_class.py_str());
            }
            md.tree.set_text_str(header, self.title.py_str());
        }
        fn build_ul(md: &mut Markdown, toc_list: &[Rc<RefCell<Token>>], parent: El) {
            let ul = md.tree.sub_element(parent, "ul");
            for item in toc_list {
                let item = item.borrow();
                let li = md.tree.sub_element(ul, "li");
                let link = md.tree.sub_element(li, "a");
                md.tree.set_text_str(link, item.name.clone());
                md.tree.set(link, "href", format!("#{}", item.id));
                if !item.children.is_empty() {
                    build_ul(md, &item.children, li);
                }
            }
        }
        build_ul(md, toc_list, div);
        if md.treeprocessors.contains("prettify") {
            PrettifyTreeprocessor::prettify_tree(md, div);
        }
        div
    }

    /// Replace every element whose only text is the marker with the table of contents.
    fn replace_marker(&self, md: &mut Markdown, node: El, div: El) {
        let Value::Str(marker) = &self.marker else {
            return;
        };
        let mut i = 0;
        while i < md.tree.len(node) {
            let child = md.tree.child(node, i);
            let tag = md.tree.tag(child).to_string();
            if !self.is_header(&tag) && tag != "pre" && tag != "code" {
                let text = md.tree.itertext(child);
                if !pystr::strip(&text).is_empty() {
                    let own = md.tree.text_str(child).unwrap_or("");
                    if !own.is_empty() && pystr::strip(own) == marker && md.tree.len(child) == 0 {
                        if let Some(pos) = md.tree.index_of(node, child) {
                            md.tree.replace_child(node, pos, div);
                        }
                    }
                }
                self.replace_marker(md, child, div);
            }
            i += 1;
        }
    }
}

impl Treeprocessor for TocTreeprocessor {
    fn run(&self, md: &mut Markdown, doc: El) -> PyResult<()> {
        let mut used_ids: HashSet<String> = HashSet::new();
        for el in md.tree.iter(doc) {
            if let Some(id) = md.tree.get(el, "id") {
                used_ids.insert(id.to_string());
            }
        }
        let mut tokens: Vec<Rc<RefCell<Token>>> = Vec::new();
        for el in md.tree.iter(doc) {
            if !self.is_header(md.tree.tag(el)) {
                continue;
            }
            self.set_level(md, el);
            let stripped = remove_fnrefs(md, el);
            let innerhtml = render_inner_html(md, stripped)?;
            let mut name = strip_tags(&innerhtml);
            if md.tree.get(el, "id").is_none() {
                let slug = self.slug(&crate::entities::unescape(&name))?;
                let id = unique(&slug, &mut used_ids);
                md.tree.set(el, "id", id);
            }
            if let Some(label) = md.tree.get(el, "data-toc-label").map(str::to_string) {
                let data_toc_label = run_postprocessors(md, unescape_escapes(&label))?;
                name = crate::serializers::escape_cdata(&strip_tags(&data_toc_label));
                md.tree.remove_attr(el, "data-toc-label");
            }
            let id = md.tree.get(el, "id").unwrap_or("").to_string();
            let level = md
                .tree
                .tag(el)
                .chars()
                .last()
                .and_then(pystr::decimal_value)
                .unwrap_or(0) as i64;
            if level >= self.toc_top && level <= self.toc_bottom {
                tokens.push(Rc::new(RefCell::new(Token {
                    level,
                    id: unescape_escapes(&id),
                    name,
                    children: Vec::new(),
                })));
            }
            if self.use_anchors {
                self.add_anchor(md, el, &id);
            }
            if !matches!(self.permalink, Permalink::Off) {
                self.add_permalink(md, el, &id);
            }
        }
        let tokens = nest_toc_tokens(tokens);
        let div = self.build_toc_div(md, &tokens);
        if self.marker.truthy() {
            self.replace_marker(md, doc, div);
        }
        // The rendered table of contents (`md.toc`) goes through the postprocessors as well.
        let toc = md.serialize(div);
        run_postprocessors(md, toc)?;
        Ok(())
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}
