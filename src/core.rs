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

//! The `Markdown` converter: it wires the preprocessors, block processors, inline patterns, tree processors and
//! postprocessors together, loads extensions and converts a document.

use std::any::Any;
use std::collections::HashMap;
use std::fmt;
use std::rc::Rc;

use crate::etree::{El, Tree};
use crate::extensions;
use crate::inlinepatterns::{self, DelimiterProcessor, InlineNode};
use crate::pyre::{Match, Regex};
use crate::util::{HtmlStash, Registry};
use crate::value::Value;
use crate::{blockprocessors, postprocessors, preprocessors, serializers, treeprocessors};

/// A Python exception: its type name and message, as the last line of a traceback shows them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PyError {
    pub kind: String,
    pub message: String,
}

impl PyError {
    pub fn new(kind: &str, message: impl Into<String>) -> PyError {
        PyError {
            kind: kind.to_string(),
            message: message.into(),
        }
    }

    /// A `KeyError` for a missing key: Python shows the key's `repr()`.
    pub fn key_error(key: &str) -> PyError {
        PyError::new("KeyError", crate::pystr::repr_str(key))
    }

    /// An error given as `"Type: message"`.
    pub fn parse(text: &str) -> PyError {
        match text.split_once(": ") {
            Some((kind, message)) => PyError::new(kind, message),
            None => PyError::new(text, ""),
        }
    }
}

impl fmt::Display for PyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.message.is_empty() {
            write!(f, "{}", self.kind)
        } else {
            write!(f, "{}: {}", self.kind, self.message)
        }
    }
}

impl std::error::Error for PyError {}

pub type PyResult<T> = Result<T, PyError>;

/// Runs on the lines of the document before block parsing.
pub trait Preprocessor {
    fn run(&self, md: &mut Markdown, lines: Vec<String>) -> PyResult<Vec<String>>;
}

/// Parses one kind of block. `run` returns `false` when it declines the block after all.
pub trait BlockProcessor {
    fn test(&self, md: &mut Markdown, parent: El, block: &str) -> bool;
    fn run(&self, md: &mut Markdown, parent: El, blocks: &mut Vec<String>) -> PyResult<bool>;
}

/// The result of an inline pattern: the node, and the start and end of the text it replaces. A missing start
/// or end means the pattern did not apply at this match; a missing node with a span means "skip this text".
pub type InlineResult = (Option<InlineNode>, Option<usize>, Option<usize>);

/// An inline pattern (Python-Markdown's `InlineProcessor`).
pub trait InlinePattern {
    fn regex(&self) -> Rc<Regex>;
    fn handle_match(&self, md: &mut Markdown, m: &Match, data: &str) -> PyResult<InlineResult>;
}

/// Runs on the element tree after block parsing.
pub trait Treeprocessor {
    fn run(&self, md: &mut Markdown, root: El) -> PyResult<()>;
    fn as_any(&self) -> &dyn Any;
}

/// Runs on the serialized text.
pub trait Postprocessor {
    fn run(&self, md: &mut Markdown, text: String) -> PyResult<String>;
}

/// An extension.
pub trait Extension {
    /// Register the extension's processors.
    fn extend_markdown(&self, md: &mut Markdown, this: Rc<dyn Extension>) -> PyResult<()>;
    /// Reset per-document state; called for registered extensions.
    fn reset(&self, _md: &mut Markdown) {}
    /// `module.ClassName`, as the debug log names it.
    fn class_path(&self) -> &'static str;
    fn as_any(&self) -> &dyn Any;
}

/// The logging levels of the `MARKDOWN` logger.
pub const DEBUG: u32 = 10;
pub const WARNING: u32 = 30;
pub const CRITICAL: u32 = 50;

/// A Markdown to HTML converter (Python-Markdown's `Markdown` class).
pub struct Markdown {
    pub tab_length: usize,
    pub escaped_chars: Vec<char>,
    pub block_level_elements: Vec<String>,
    pub registered_extensions: Vec<Rc<dyn Extension>>,
    pub strip_top_level_tags: bool,
    pub delimiters: Option<Rc<DelimiterProcessor>>,
    pub references: HashMap<String, (String, Option<String>)>,
    pub html_stash: HtmlStash,
    pub preprocessors: Registry<Rc<dyn Preprocessor>>,
    pub block_processors: Registry<Rc<dyn BlockProcessor>>,
    pub inline_patterns: Registry<Rc<dyn InlinePattern>>,
    pub treeprocessors: Registry<Rc<dyn Treeprocessor>>,
    pub postprocessors: Registry<Rc<dyn Postprocessor>>,
    /// The block parser's nesting state.
    pub state: Vec<&'static str>,
    pub output_format: String,
    pub tree: Tree,
    /// `md.Meta`, present once the `meta` extension is loaded.
    pub meta: Option<Vec<(String, Vec<String>)>>,
    /// The level of the `MARKDOWN` logger; messages at or above it go to standard error.
    pub log_level: u32,
}

impl Markdown {
    /// `Markdown(extensions=..., extension_configs=..., output_format=...)`.
    pub fn new(
        extension_names: &[String],
        configs: &Value,
        output_format: &str,
        log_level: u32,
    ) -> PyResult<Markdown> {
        let mut md = Markdown {
            tab_length: 4,
            escaped_chars: "\\`*_{}[]()>#+-.!".chars().collect(),
            block_level_elements: crate::util::block_level_elements(),
            registered_extensions: Vec::new(),
            strip_top_level_tags: true,
            delimiters: None,
            references: HashMap::new(),
            html_stash: HtmlStash::default(),
            preprocessors: Registry::new(),
            block_processors: Registry::new(),
            inline_patterns: Registry::new(),
            treeprocessors: Registry::new(),
            postprocessors: Registry::new(),
            state: Vec::new(),
            output_format: String::new(),
            tree: Tree::new(),
            meta: None,
            log_level,
        };
        md.build_parser();
        md.register_extensions(extension_names, configs)?;
        md.set_output_format(output_format)?;
        md.reset();
        Ok(md)
    }

    fn build_parser(&mut self) {
        preprocessors::build_preprocessors(self);
        blockprocessors::build_block_parser(self);
        inlinepatterns::build_inlinepatterns(self);
        treeprocessors::build_treeprocessors(self);
        postprocessors::build_postprocessors(self);
    }

    /// Write a message of the `MARKDOWN` logger.
    pub fn log(&self, level: u32, message: &str) {
        if level >= self.log_level {
            eprintln!("{message}");
        }
    }

    /// Load extensions by name, each with its entry of `configs`.
    pub fn register_extensions(&mut self, names: &[String], configs: &Value) -> PyResult<()> {
        for name in names {
            let config = match configs {
                Value::Dict(_) => configs
                    .get(name)
                    .cloned()
                    .unwrap_or(Value::Dict(Vec::new())),
                other => {
                    return Err(PyError::new(
                        "AttributeError",
                        format!("'{}' object has no attribute 'get'", other.type_name()),
                    ))
                }
            };
            let ext = self.build_extension(name, &config)?;
            ext.extend_markdown(self, ext.clone())?;
            self.log(
                DEBUG,
                &format!("Successfully loaded extension \"{}\".", ext.class_path()),
            );
        }
        Ok(())
    }

    /// Build an extension from its name: an entry point name, a module path or `module:Class`.
    fn build_extension(&mut self, name: &str, configs: &Value) -> PyResult<Rc<dyn Extension>> {
        let configs = dict_from(configs)?;
        if let Some(ext) = extensions::by_entry_point(name, &configs) {
            return ext;
        }
        let (module, class_name) = match name.split_once(':') {
            Some((m, c)) => (m, Some(c)),
            None => (name, None),
        };
        extensions::import_module(module)?;
        self.log(
            DEBUG,
            &format!("Successfully imported extension module \"{module}\"."),
        );
        match class_name {
            Some(class_name) => extensions::by_class(module, class_name, &configs),
            None => extensions::make_extension(module, &configs).map_err(|e| {
                if e.kind == "AttributeError" {
                    PyError::new(
                        "AttributeError",
                        format!("Failed to initiate extension '{module}': {}", e.message),
                    )
                } else {
                    e
                }
            }),
        }
    }

    /// Register an extension as having a resettable state.
    pub fn register_extension(&mut self, ext: Rc<dyn Extension>) {
        self.registered_extensions.push(ext);
    }

    /// Reset all state for a new document.
    pub fn reset(&mut self) {
        self.html_stash.reset();
        self.references.clear();
        if let Some(delimiters) = self.delimiters.clone() {
            delimiters.reset();
        }
        for ext in self.registered_extensions.clone() {
            ext.reset(self);
        }
    }

    /// Choose the serializer: `html` or `xhtml`, ignoring case and trailing digits 1, 4 and 5.
    pub fn set_output_format(&mut self, format: &str) -> PyResult<()> {
        let lowered = crate::pystr::lower(format);
        let name = crate::pystr::rstrip_chars(&lowered, "145").to_string();
        if name != "html" && name != "xhtml" {
            let message =
                format!("Invalid Output Format: \"{name}\". Use one of \"html\", \"xhtml\".");
            return Err(PyError::key_error(&message));
        }
        self.output_format = name;
        Ok(())
    }

    /// Serialize an element with the chosen output format.
    pub fn serialize(&self, el: El) -> String {
        if self.output_format == "html" {
            serializers::to_html_string(&self.tree, el)
        } else {
            serializers::to_xhtml_string(&self.tree, el)
        }
    }

    /// Whether a tag is a block level element.
    pub fn is_block_level(&self, tag: &str) -> bool {
        let lowered = crate::pystr::lower(tag);
        let tag = crate::pystr::rstrip_chars(&lowered, "/");
        self.block_level_elements.iter().any(|t| t == tag)
    }

    /// Set a parser state for a nested block.
    pub fn state_set(&mut self, state: &'static str) {
        self.state.push(state);
    }

    /// Step back one nested parser state.
    pub fn state_reset(&mut self) {
        self.state.pop();
    }

    /// Whether the current parser state is `state`.
    pub fn is_state(&self, state: &str) -> bool {
        self.state.last().is_some_and(|s| *s == state)
    }

    /// `BlockParser.parseChunk`: split on blank lines and parse the blocks into `parent`.
    pub fn parse_chunk(&mut self, parent: El, text: &str) -> PyResult<()> {
        let blocks = text.split("\n\n").map(str::to_string).collect();
        self.parse_blocks(parent, blocks)
    }

    /// `BlockParser.parseBlocks`: let the first processor that accepts each block parse it.
    pub fn parse_blocks(&mut self, parent: El, mut blocks: Vec<String>) -> PyResult<()> {
        while !blocks.is_empty() {
            for processor in self.block_processors.items() {
                if processor.test(self, parent, &blocks[0])
                    && processor.run(self, parent, &mut blocks)?
                {
                    break;
                }
            }
        }
        Ok(())
    }

    /// The `InlineProcessor` tree processor registered as `inline`, if any.
    pub fn inline_treeprocessor(&self) -> Option<Rc<dyn Treeprocessor>> {
        self.treeprocessors.get("inline").cloned()
    }

    /// Convert a Markdown string to HTML.
    pub fn convert(&mut self, source: &str) -> PyResult<String> {
        if crate::pystr::strip(source).is_empty() {
            return Ok(String::new());
        }
        let mut lines: Vec<String> = source.split('\n').map(str::to_string).collect();
        for prep in self.preprocessors.items() {
            lines = prep.run(self, lines)?;
        }
        let root = self.tree.element("div");
        self.parse_chunk(root, &lines.join("\n"))?;
        for tp in self.treeprocessors.items() {
            tp.run(self, root)?;
        }
        let mut output = self.serialize(root);
        if self.strip_top_level_tags {
            match (output.find("<div>"), output.rfind("</div>")) {
                (Some(start), Some(end)) if start + 5 <= end => {
                    output = crate::pystr::strip(&output[start + 5..end]).to_string();
                }
                _ => {
                    if crate::pystr::strip(&output).ends_with("<div />") {
                        output = String::new();
                    } else {
                        return Err(PyError::new(
                            "ValueError",
                            format!(
                                "Markdown failed to strip top-level tags. Document={}",
                                crate::pystr::repr_str(crate::pystr::strip(&output))
                            ),
                        ));
                    }
                }
            }
        }
        for pp in self.postprocessors.items() {
            output = pp.run(self, output)?;
        }
        Ok(crate::pystr::strip(&output).to_string())
    }
}

/// `dict(configs)` for the configuration of one extension.
fn dict_from(configs: &Value) -> PyResult<Vec<(String, Value)>> {
    let length_error = |i: usize, n: usize| {
        PyError::new(
            "ValueError",
            format!("dictionary update sequence element #{i} has length {n}; 2 is required"),
        )
    };
    match configs {
        Value::Dict(items) => Ok(items.clone()),
        Value::Str(s) if s.is_empty() => Ok(Vec::new()),
        Value::Str(_) => Err(length_error(0, 1)),
        Value::List(items) => {
            let mut out: Vec<(String, Value)> = Vec::new();
            for (i, item) in items.iter().enumerate() {
                let (key, value) = match item {
                    Value::List(pair) if pair.len() == 2 => (pair[0].py_str(), pair[1].clone()),
                    Value::List(pair) => return Err(length_error(i, pair.len())),
                    Value::Str(s) if crate::pystr::char_len(s) == 2 => {
                        let mut cs = s.chars();
                        let k = cs.next().map(String::from).unwrap_or_default();
                        (k, Value::Str(cs.collect()))
                    }
                    Value::Str(s) => return Err(length_error(i, crate::pystr::char_len(s))),
                    Value::Dict(keys) if keys.len() == 2 => {
                        (keys[0].0.clone(), Value::Str(keys[1].0.clone()))
                    }
                    Value::Dict(keys) => return Err(length_error(i, keys.len())),
                    _ => {
                        return Err(PyError::new(
                            "TypeError",
                            format!(
                            "cannot convert dictionary update sequence element #{i} to a sequence"
                        ),
                        ))
                    }
                };
                insert_last_wins(&mut out, key, value);
            }
            Ok(out)
        }
        other => Err(PyError::new(
            "TypeError",
            format!("'{}' object is not iterable", other.type_name()),
        )),
    }
}

fn insert_last_wins(items: &mut Vec<(String, Value)>, key: String, value: Value) {
    match items.iter_mut().find(|(k, _)| *k == key) {
        Some(slot) => slot.1 = value,
        None => items.push((key, value)),
    }
}
