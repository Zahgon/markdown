// Footnotes Extension for Python-Markdown
// =======================================
//
// Adds footnote handling to Python-Markdown.
//
// See https://Python-Markdown.github.io/extensions/footnotes
// for documentation.
//
// Copyright The Python Markdown Project
//
// License: [BSD](https://opensource.org/licenses/bsd-license.php)

//! Footnotes: `[^label]` references and `[^label]: text` definitions, collected in a list at the end of the
//! document (or where the place marker is).

use std::any::Any;
use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;
use std::sync::OnceLock;

use super::Config;
use crate::core::{
    BlockProcessor, Extension, InlinePattern, InlineResult, Markdown, Postprocessor, PyError,
    PyResult, Treeprocessor,
};
use crate::etree::El;
use crate::inlinepatterns::InlineNode;
use crate::pyre::{Match, Regex, M, S};
use crate::pystr::{self, FormatArg};
use crate::value::Value;

const FN_BACKLINK_TEXT: &str = "\u{2}zz1337820767766393qq\u{3}";
const NBSP_PLACEHOLDER: &str = "\u{2}qq3936677670287331zz\u{3}";

/// The state the footnote processors share.
struct State {
    config: Config,
    footnote_order: Vec<String>,
    /// Definitions in definition order.
    footnotes: Vec<(String, String)>,
    unique_prefix: i64,
    found_refs: Vec<(String, i64)>,
    used_refs: HashSet<String>,
}

type Shared = Rc<RefCell<State>>;

impl State {
    fn separator(&self) -> PyResult<String> {
        match self.config.get("SEPARATOR") {
            Value::Str(s) => Ok(s.clone()),
            other => Err(PyError::new(
                "TypeError",
                format!("must be str or None, not {}", other.type_name()),
            )),
        }
    }

    fn reset(&mut self) {
        self.footnote_order.clear();
        self.footnotes.clear();
        self.unique_prefix += 1;
        self.found_refs.clear();
        self.used_refs.clear();
    }

    fn unique_ids(&self) -> bool {
        self.config.get("UNIQUE_IDS").truthy()
    }

    fn make_footnote_id(&self, id: &str) -> PyResult<String> {
        let sep = self.config.get("SEPARATOR").py_str();
        Ok(if self.unique_ids() {
            format!("fn{sep}{}-{id}", self.unique_prefix)
        } else {
            format!("fn{sep}{id}")
        })
    }

    fn make_footnote_ref_id(&mut self, id: &str, found: bool) -> PyResult<String> {
        let sep = self.config.get("SEPARATOR").py_str();
        let reference = if self.unique_ids() {
            format!("fnref{sep}{}-{id}", self.unique_prefix)
        } else {
            format!("fnref{sep}{id}")
        };
        self.unique_ref(reference, found)
    }

    /// Make a reference id unique (`fnref2:...`, `fnref3:...`) and count the references of each footnote.
    fn unique_ref(&mut self, reference: String, found: bool) -> PyResult<String> {
        if !found {
            return Ok(reference);
        }
        static REF_ID: OnceLock<Regex> = OnceLock::new();
        let ref_id = REF_ID.get_or_init(|| Regex::new(r"(fnref)(\d+)", 0));
        let original = reference.clone();
        let mut reference = reference;
        while self.used_refs.contains(&reference) {
            let sep = self.separator()?;
            let (r, rest) = split_once_py(&reference, &sep)?;
            reference = match ref_id.match_at(&r, 0) {
                Some(m) => {
                    let n = pystr::parse_int(m.str(2)).unwrap_or(0);
                    format!("{}{}{sep}{rest}", m.str(1), n + 1)
                }
                None => format!("{r}2{sep}{rest}"),
            };
        }
        self.used_refs.insert(reference.clone());
        match self.found_refs.iter_mut().find(|(k, _)| *k == original) {
            Some(slot) => slot.1 += 1,
            None => self.found_refs.push((original, 1)),
        }
        Ok(reference)
    }
}

/// `a, b = text.split(sep, 1)`, with Python's errors.
fn split_once_py(text: &str, sep: &str) -> PyResult<(String, String)> {
    if sep.is_empty() {
        return Err(PyError::new("ValueError", "empty separator"));
    }
    match text.split_once(sep) {
        Some((a, b)) => Ok((a.to_string(), b.to_string())),
        None => Err(PyError::new(
            "ValueError",
            "not enough values to unpack (expected 2, got 1)",
        )),
    }
}

/// `template.format(n)` for the title and superscript options.
fn format_number(template: &Value, n: usize) -> PyResult<String> {
    match template {
        Value::Str(s) => {
            pystr::format(s, &[FormatArg::Int(n as i64)]).map_err(|e| PyError::parse(&e))
        }
        other => Err(PyError::new(
            "AttributeError",
            format!("'{}' object has no attribute 'format'", other.type_name()),
        )),
    }
}

pub struct FootnoteExtension {
    state: Shared,
}

impl FootnoteExtension {
    pub fn create(configs: &[(String, Value)]) -> PyResult<Rc<dyn Extension>> {
        let mut config = Config::new(&[
            ("PLACE_MARKER", Value::Str("///Footnotes Go Here///".into())),
            ("UNIQUE_IDS", Value::Bool(false)),
            ("BACKLINK_TEXT", Value::Str("&#8617;".into())),
            ("SUPERSCRIPT_TEXT", Value::Str("{}".into())),
            (
                "BACKLINK_TITLE",
                Value::Str("Jump back to footnote %d in the text".into()),
            ),
            ("SEPARATOR", Value::Str(":".into())),
            ("USE_DEFINITION_ORDER", Value::Bool(true)),
        ]);
        config.set_all(configs)?;
        let title = match config.get("BACKLINK_TITLE") {
            Value::Str(s) => s.replace("%d", "{}"),
            other => {
                return Err(PyError::new(
                    "AttributeError",
                    format!("'{}' object has no attribute 'replace'", other.type_name()),
                ))
            }
        };
        config.set("BACKLINK_TITLE", Value::Str(title))?;
        let mut state = State {
            config,
            footnote_order: Vec::new(),
            footnotes: Vec::new(),
            unique_prefix: 0,
            found_refs: Vec::new(),
            used_refs: HashSet::new(),
        };
        state.reset();
        Ok(Rc::new(FootnoteExtension {
            state: Rc::new(RefCell::new(state)),
        }))
    }
}

impl Extension for FootnoteExtension {
    fn extend_markdown(&self, md: &mut Markdown, this: Rc<dyn Extension>) -> PyResult<()> {
        md.register_extension(this);
        let st = &self.state;
        md.block_processors.register(
            Rc::new(FootnoteBlockProcessor { state: st.clone() }),
            "footnote",
            17.0,
        );
        md.inline_patterns.register(
            Rc::new(FootnoteInlineProcessor {
                state: st.clone(),
                re: Rc::new(Regex::new(r"\[\^([^\]]*)\]", S)),
            }),
            "footnote",
            175.0,
        );
        md.treeprocessors.register(
            Rc::new(FootnoteTreeprocessor { state: st.clone() }),
            "footnote",
            50.0,
        );
        if !st.borrow().config.get("USE_DEFINITION_ORDER").truthy() {
            md.treeprocessors.register(
                Rc::new(FootnoteReorderingProcessor { state: st.clone() }),
                "footnote-reorder",
                19.0,
            );
        }
        md.treeprocessors.register(
            Rc::new(FootnotePostTreeprocessor { state: st.clone() }),
            "footnote-duplicate",
            15.0,
        );
        md.postprocessors.register(
            Rc::new(FootnotePostprocessor { state: st.clone() }),
            "footnote",
            25.0,
        );
        Ok(())
    }

    fn reset(&self, _md: &mut Markdown) {
        self.state.borrow_mut().reset();
    }

    fn class_path(&self) -> &'static str {
        "markdown.extensions.footnotes.FootnoteExtension"
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

fn block_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^[ ]{0,3}\[\^([^\]]*)\]:[ ]*(.*)$", M))
}

/// Remove one level (four spaces) of indent from every line that has it.
fn fn_detab(block: &str) -> String {
    block
        .split('\n')
        .map(|l| l.strip_prefix("    ").unwrap_or(l))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Read footnote definitions and their indented continuation blocks.
pub struct FootnoteBlockProcessor {
    state: Shared,
}

impl FootnoteBlockProcessor {
    fn detect_tabbed(blocks: &mut Vec<String>) -> Vec<String> {
        let mut fn_blocks = Vec::new();
        while !blocks.is_empty() {
            if !blocks[0].starts_with("    ") {
                break;
            }
            let block = blocks.remove(0);
            match block_re().search(&block, 0).map(|m| m.start(0)) {
                Some(start) => {
                    let before = pystr::rstrip_chars(&block[..start], "\n");
                    fn_blocks.push(fn_detab(before));
                    blocks.insert(0, block[start..].to_string());
                    break;
                }
                None => fn_blocks.push(fn_detab(&block)),
            }
        }
        fn_blocks
    }
}

impl BlockProcessor for FootnoteBlockProcessor {
    fn test(&self, _md: &mut Markdown, _parent: El, _block: &str) -> bool {
        true
    }

    fn run(&self, _md: &mut Markdown, _parent: El, blocks: &mut Vec<String>) -> PyResult<bool> {
        let block = blocks.remove(0);
        let Some(m) = block_re().search(&block, 0) else {
            blocks.insert(0, block);
            return Ok(false);
        };
        let id = m.str(1).to_string();
        let mut fn_blocks = vec![m.str(2).to_string()];
        let therest = pystr::lstrip_chars(&block[m.end(0)..], "\n").to_string();
        match block_re().search(&therest, 0).map(|m2| m2.start(0)) {
            Some(start2) => {
                let before = pystr::rstrip_chars(&therest[..start2], "\n");
                fn_blocks[0] =
                    pystr::lstrip_chars(&format!("{}\n{}", fn_blocks[0], fn_detab(before)), "\n")
                        .to_string();
                blocks.insert(0, therest[start2..].to_string());
            }
            None => {
                fn_blocks[0] =
                    pystr::strip_chars(&format!("{}\n{}", fn_blocks[0], fn_detab(&therest)), "\n")
                        .to_string();
                fn_blocks.extend(Self::detect_tabbed(blocks));
            }
        }
        let footnote = fn_blocks.join("\n\n");
        {
            let mut st = self.state.borrow_mut();
            let text = pystr::rstrip(&footnote).to_string();
            match st.footnotes.iter_mut().find(|(k, _)| *k == id) {
                Some(slot) => slot.1 = text,
                None => st.footnotes.push((id, text)),
            }
        }
        if !pystr::strip(&block[..m.start(0)]).is_empty() {
            blocks.insert(
                0,
                pystr::rstrip_chars(&block[..m.start(0)], "\n").to_string(),
            );
        }
        Ok(true)
    }
}

/// A footnote reference: a superscript link to the footnote.
pub struct FootnoteInlineProcessor {
    state: Shared,
    re: Rc<Regex>,
}

impl InlinePattern for FootnoteInlineProcessor {
    fn regex(&self) -> Rc<Regex> {
        self.re.clone()
    }

    fn handle_match(&self, md: &mut Markdown, m: &Match, _data: &str) -> PyResult<InlineResult> {
        let id = m.str(1);
        let mut st = self.state.borrow_mut();
        if !st.footnotes.iter().any(|(k, _)| k == id) {
            return Ok((None, None, None));
        }
        if !st.footnote_order.iter().any(|k| k == id) {
            st.footnote_order.push(id.to_string());
        }
        let number = if !st.config.get("USE_DEFINITION_ORDER").truthy() {
            st.footnote_order.iter().position(|k| k == id).unwrap_or(0) + 1
        } else {
            st.footnotes.iter().position(|(k, _)| k == id).unwrap_or(0) + 1
        };
        let sup = md.tree.element("sup");
        let a = md.tree.sub_element(sup, "a");
        let ref_id = st.make_footnote_ref_id(id, true)?;
        md.tree.set(sup, "id", ref_id);
        md.tree
            .set(a, "href", format!("#{}", st.make_footnote_id(id)?));
        md.tree.set(a, "class", "footnote-ref");
        let text = format_number(st.config.get("SUPERSCRIPT_TEXT"), number)?;
        md.tree.set_text_str(a, text);
        Ok((Some(InlineNode::El(sup)), Some(m.start(0)), Some(m.end(0))))
    }
}

/// Build the footnote list and put it at the place marker or at the end of the document.
pub struct FootnoteTreeprocessor {
    state: Shared,
}

impl FootnoteTreeprocessor {
    fn make_footnotes_div(&self, md: &mut Markdown) -> PyResult<Option<El>> {
        let footnotes = self.state.borrow().footnotes.clone();
        if footnotes.is_empty() {
            return Ok(None);
        }
        let div = md.tree.element("div");
        md.tree.set(div, "class", "footnote");
        md.tree.sub_element(div, "hr");
        let ol = md.tree.sub_element(div, "ol");
        let surrogate_parent = md.tree.element("div");
        for (index, (id, text)) in footnotes.iter().enumerate() {
            let li = md.tree.sub_element(ol, "li");
            let fid = self.state.borrow().make_footnote_id(id)?;
            md.tree.set(li, "id", fid);
            md.parse_chunk(surrogate_parent, text)?;
            for el in md.tree.children(surrogate_parent).to_vec() {
                md.tree.append(li, el);
                md.tree.remove(surrogate_parent, el);
            }
            let backlink = md.tree.element("a");
            let ref_id = self.state.borrow_mut().make_footnote_ref_id(id, false)?;
            md.tree.set(backlink, "href", format!("#{ref_id}"));
            md.tree.set(backlink, "class", "footnote-backref");
            let title = format_number(self.state.borrow().config.get("BACKLINK_TITLE"), index + 1)?;
            md.tree.set(backlink, "title", title);
            md.tree.set_text_str(backlink, FN_BACKLINK_TEXT);
            if let Some(node) = md.tree.last_child(li) {
                if md.tree.tag(node) == "p" {
                    let text = match md.tree.text_str(node) {
                        Some(t) => format!("{t}{NBSP_PLACEHOLDER}"),
                        None => {
                            return Err(PyError::new(
                                "TypeError",
                                "unsupported operand type(s) for +: 'NoneType' and 'str'",
                            ))
                        }
                    };
                    md.tree.set_text_str(node, text);
                    md.tree.append(node, backlink);
                } else {
                    let p = md.tree.sub_element(li, "p");
                    md.tree.append(p, backlink);
                }
            }
        }
        Ok(Some(div))
    }

    /// The first element whose text or tail contains the place marker: (element, parent, in text).
    fn find_placeholder(md: &Markdown, element: El, marker: &str) -> Option<(El, El, bool)> {
        for &child in md.tree.children(element) {
            if md
                .tree
                .text_str(child)
                .is_some_and(|t| !t.is_empty() && t.contains(marker))
            {
                return Some((child, element, true));
            }
            if md
                .tree
                .tail_str(child)
                .is_some_and(|t| !t.is_empty() && t.contains(marker))
            {
                return Some((child, element, false));
            }
            if let Some(found) = Self::find_placeholder(md, child, marker) {
                return Some(found);
            }
        }
        None
    }
}

impl Treeprocessor for FootnoteTreeprocessor {
    fn run(&self, md: &mut Markdown, root: El) -> PyResult<()> {
        let Some(div) = self.make_footnotes_div(md)? else {
            return Ok(());
        };
        let marker = match self.state.borrow().config.get("PLACE_MARKER") {
            Value::Str(s) => s.clone(),
            other => {
                return Err(PyError::new(
                    "TypeError",
                    format!("must be str, not {}", other.type_name()),
                ));
            }
        };
        match Self::find_placeholder(md, root, &marker) {
            Some((child, parent, is_text)) => {
                let ind = md.tree.index_of(parent, child).unwrap_or(0);
                if is_text {
                    md.tree.remove(parent, child);
                    md.tree.insert(parent, ind, div);
                } else {
                    md.tree.insert(parent, ind + 1, div);
                    md.tree.set_tail(child, None);
                }
            }
            None => md.tree.append(root, div),
        }
        Ok(())
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Order the footnote list by first reference (`USE_DEFINITION_ORDER` off).
pub struct FootnoteReorderingProcessor {
    state: Shared,
}

impl FootnoteReorderingProcessor {
    fn reorder(&self, md: &mut Markdown, parent: El) -> PyResult<()> {
        let Some(old_list) = md.tree.find_child(parent, "ol") else {
            return Err(PyError::new(
                "TypeError",
                "remove() argument must be xml.etree.ElementTree.Element, not None",
            ));
        };
        md.tree.remove(parent, old_list);
        let items: Vec<El> = md
            .tree
            .children(old_list)
            .iter()
            .copied()
            .filter(|&c| md.tree.tag(c) == "li")
            .collect();
        let st = self.state.borrow();
        let sep = st.config.get("SEPARATOR").py_str();
        let order_by_id = |li: El| -> usize {
            let id = md.tree.get(li, "id").unwrap_or("");
            let id = if sep.is_empty() {
                id
            } else {
                id.split_once(sep.as_str()).map_or(id, |(_, rest)| rest)
            };
            st.footnote_order
                .iter()
                .position(|k| k == id)
                .unwrap_or(st.footnotes.len())
        };
        let mut keyed: Vec<(usize, El)> = items.iter().map(|&li| (order_by_id(li), li)).collect();
        keyed.sort_by_key(|(k, _)| *k);
        let title_template = st.config.get("BACKLINK_TITLE").clone();
        drop(st);
        let new_list = md.tree.sub_element(parent, "ol");
        for (index, (_, item)) in keyed.into_iter().enumerate() {
            let backlink = md.tree.iter(item).into_iter().skip(1).find(|&e| {
                md.tree.tag(e) == "a" && md.tree.get(e, "class") == Some("footnote-backref")
            });
            let Some(backlink) = backlink else {
                return Err(PyError::new(
                    "AttributeError",
                    "'NoneType' object has no attribute 'set'",
                ));
            };
            let title = format_number(&title_template, index + 1)?;
            md.tree.set(backlink, "title", title);
            md.tree.append(new_list, item);
        }
        Ok(())
    }
}

impl Treeprocessor for FootnoteReorderingProcessor {
    fn run(&self, md: &mut Markdown, root: El) -> PyResult<()> {
        let (empty, reordered) = {
            let st = self.state.borrow();
            let keys: Vec<&String> = st.footnotes.iter().map(|(k, _)| k).collect();
            (
                st.footnotes.is_empty(),
                st.footnote_order.iter().collect::<Vec<_>>() != keys,
            )
        };
        if empty || !reordered {
            return Ok(());
        }
        for div in md.tree.iter_tag(root, "div") {
            if md.tree.get(div, "class") == Some("footnote") {
                self.reorder(md, div)?;
                break;
            }
        }
        Ok(())
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Add a backlink for every further reference to the same footnote.
pub struct FootnotePostTreeprocessor {
    state: Shared,
}

impl FootnotePostTreeprocessor {
    fn add_duplicates(&self, md: &mut Markdown, li: El, duplicates: i64) -> PyResult<()> {
        let sep = self.state.borrow().separator()?;
        for link in md.tree.iter_tag(li, "a") {
            if md.tree.get(link, "class") == Some("footnote-backref") {
                let href = md.tree.get(link, "href").unwrap_or("").to_string();
                let (r, rest) = split_once_py(&href, &sep)?;
                let mut links = Vec::new();
                for index in 2..=duplicates {
                    let sib_link = md.tree.deepcopy(link);
                    md.tree
                        .set(sib_link, "href", format!("{r}{index}{sep}{rest}"));
                    links.push(sib_link);
                }
                let el = md
                    .tree
                    .last_child(li)
                    .expect("a footnote item has children");
                for l in links {
                    md.tree.append(el, l);
                }
                break;
            }
        }
        Ok(())
    }

    fn num_duplicates(&self, md: &Markdown, li: El) -> PyResult<i64> {
        let st = self.state.borrow();
        let sep = st.separator()?;
        let id = md.tree.get(li, "id").unwrap_or("");
        let (f, rest) = split_once_py(id, &sep)?;
        let link_id = format!("{f}ref{sep}{rest}");
        Ok(st
            .found_refs
            .iter()
            .find(|(k, _)| *k == link_id)
            .map_or(0, |(_, n)| *n))
    }
}

impl Treeprocessor for FootnotePostTreeprocessor {
    fn run(&self, md: &mut Markdown, root: El) -> PyResult<()> {
        for div in md.tree.iter_tag(root, "div") {
            if md.tree.get(div, "class") == Some("footnote") {
                if let Some(ol) = md.tree.iter_tag(div, "ol").first().copied() {
                    for li in md.tree.children(ol).to_vec() {
                        let count = self.num_duplicates(md, li)?;
                        if count > 1 {
                            self.add_duplicates(md, li, count)?;
                        }
                    }
                }
            }
        }
        Ok(())
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Put the backlink text and the non-breaking space into the output.
pub struct FootnotePostprocessor {
    state: Shared,
}

impl Postprocessor for FootnotePostprocessor {
    fn run(&self, _md: &mut Markdown, text: String) -> PyResult<String> {
        let backlink = match self.state.borrow().config.get("BACKLINK_TEXT") {
            Value::Str(s) => s.clone(),
            other => {
                return Err(PyError::new(
                    "TypeError",
                    format!(
                        "replace() argument 2 must be str, not {}",
                        other.type_name()
                    ),
                ));
            }
        };
        Ok(text
            .replace(FN_BACKLINK_TEXT, &backlink)
            .replace(NBSP_PLACEHOLDER, "&#160;"))
    }
}
