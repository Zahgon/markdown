// Fenced Code Extension for Python Markdown
// =========================================
//
// This extension adds Fenced Code Blocks to Python-Markdown.
//
// See https://Python-Markdown.github.io/extensions/fenced_code_blocks
// for documentation.
//
// Original code Copyright 2007-2008 [Waylan Limberg](https://github.com/waylan).
//
// All changes Copyright 2008-2014 The Python Markdown Project
//
// License: [BSD](https://opensource.org/licenses/bsd-license.php)

//! Fenced code blocks: ``` or ~~~ fences with an optional language, `hl_lines` or an attribute list.
//! Syntax highlighting (codehilite with Pygments) is not part of this port.

use std::any::Any;
use std::cell::Cell;
use std::rc::Rc;
use std::sync::OnceLock;

use super::attr_list::{get_attrs_and_remainder, sanitize_name, AttrListExtension};
use super::Config;
use crate::core::{Extension, Markdown, Preprocessor, PyError, PyResult};
use crate::pyre::{Regex, M, S, X};
use crate::serializers::escape_attrib_html;
use crate::value::{parse_bool_value, Value};

pub struct FencedCodeExtension {
    config: Config,
}

impl FencedCodeExtension {
    pub fn create(configs: &[(String, Value)]) -> PyResult<Rc<dyn Extension>> {
        let mut config = Config::new(&[("lang_prefix", Value::Str("language-".into()))]);
        config.set_all(configs)?;
        Ok(Rc::new(FencedCodeExtension { config }))
    }
}

impl Extension for FencedCodeExtension {
    fn extend_markdown(&self, md: &mut Markdown, this: Rc<dyn Extension>) -> PyResult<()> {
        md.register_extension(this);
        let pre = FencedBlockPreprocessor {
            lang_prefix: self.config.get("lang_prefix").clone(),
            checked_for_deps: Cell::new(false),
            use_attr_list: Cell::new(false),
        };
        md.preprocessors
            .register(Rc::new(pre), "fenced_code_block", 25.0);
        Ok(())
    }

    fn class_path(&self) -> &'static str {
        "markdown.extensions.fenced_code.FencedCodeExtension"
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

fn fenced_block_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r#"
(?P<fence>^(?:~{3,}|`{3,}))[ ]*                          # opening fence
((\{(?P<attrs>[^\n]*)\})|                                # (optional {attrs} or
(\.?(?P<lang>[\w#.+-]*)[ ]*)?                            # optional (.)lang
(hl_lines=(?P<quot>"|')(?P<hl_lines>.*?)(?P=quot)[ ]*)?) # optional hl_lines)
\n                                                       # newline (end of opening fence)
(?P<code>.*?)(?<=\n)                                     # the code block
(?P=fence)[ ]*$                                          # closing fence
"#,
            M | S | X,
        )
    })
}

/// A configuration value collected from the fence's options.
#[derive(Clone)]
enum Opt {
    Str(String),
    Lines(Vec<i64>),
    Bool(Option<bool>),
}

/// `parse_hl_lines`: the line numbers, or none at all if one is not a number.
fn parse_hl_lines(expr: &str) -> Vec<i64> {
    let mut out = Vec::new();
    for word in crate::pystr::split_whitespace(expr) {
        match crate::pystr::parse_int(word) {
            Ok(n) => out.push(n),
            Err(_) => return Vec::new(),
        }
    }
    out
}

/// Replace fenced blocks with stashed `<pre><code>` HTML.
pub struct FencedBlockPreprocessor {
    lang_prefix: Value,
    checked_for_deps: Cell<bool>,
    use_attr_list: Cell<bool>,
}

impl FencedBlockPreprocessor {
    fn handle_attrs(attrs: &[(String, String)]) -> (String, Vec<String>, Vec<(String, Opt)>) {
        let mut id = String::new();
        let mut classes = Vec::new();
        let mut configs: Vec<(String, Opt)> = Vec::new();
        for (k, v) in attrs {
            let opt = match k.as_str() {
                "id" => {
                    id = v.clone();
                    continue;
                }
                "." => {
                    classes.push(v.clone());
                    continue;
                }
                "hl_lines" => Opt::Lines(parse_hl_lines(v)),
                "linenums" | "guess_lang" | "noclasses" | "use_pygments" => {
                    Opt::Bool(parse_bool_value(&Value::Str(v.clone()), false, true).unwrap_or(None))
                }
                _ => Opt::Str(v.clone()),
            };
            match configs.iter_mut().find(|(key, _)| key == k) {
                Some(slot) => slot.1 = opt,
                None => configs.push((k.clone(), opt)),
            }
        }
        (id, classes, configs)
    }

    fn escape(txt: &str) -> String {
        txt.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;")
    }

    /// An option value as `_escape_attrib_html` and an f-string render it.
    fn render_opt(opt: &Opt) -> PyResult<String> {
        match opt {
            Opt::Str(s) => Ok(escape_attrib_html(s)),
            Opt::Lines(lines) => Ok(format!(
                "[{}]",
                lines
                    .iter()
                    .map(i64::to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            )),
            Opt::Bool(b) => {
                let (repr, kind) = match b {
                    Some(true) => ("True", "bool"),
                    Some(false) => ("False", "bool"),
                    None => ("None", "NoneType"),
                };
                Err(PyError::new(
                    "TypeError",
                    format!("cannot serialize {repr} (type {kind})"),
                ))
            }
        }
    }
}

impl Preprocessor for FencedBlockPreprocessor {
    fn run(&self, md: &mut Markdown, lines: Vec<String>) -> PyResult<Vec<String>> {
        if !self.checked_for_deps.get() {
            if md
                .registered_extensions
                .iter()
                .any(|e| e.as_any().is::<AttrListExtension>())
            {
                self.use_attr_list.set(true);
            }
            self.checked_for_deps.set(true);
        }
        let mut text = lines.join("\n");
        let mut index = 0;
        while let Some(m) = fenced_block_re().search(&text, index) {
            let mut lang: Option<String> = None;
            let mut id = String::new();
            let mut classes: Vec<String> = Vec::new();
            let mut config: Vec<(String, Opt)> = Vec::new();
            if let Some(attrs) = m.name("attrs").filter(|a| !a.is_empty()) {
                let (attrs, remainder) = get_attrs_and_remainder(attrs);
                if !remainder.is_empty() {
                    // Unbalanced braces: not a fence with attributes; look again after them.
                    index = m.name_span("attrs").map_or(m.end(0), |(_, e)| e);
                    continue;
                }
                let (i, c, cfg) = Self::handle_attrs(&attrs);
                id = i;
                classes = c;
                config = cfg;
                if !classes.is_empty() {
                    lang = Some(classes.remove(0));
                }
            } else {
                if let Some(l) = m.name("lang").filter(|l| !l.is_empty()) {
                    lang = Some(l.to_string());
                }
                if let Some(h) = m.name("hl_lines").filter(|h| !h.is_empty()) {
                    config.push(("hl_lines".into(), Opt::Lines(parse_hl_lines(h))));
                }
            }
            let mut lang_attr = String::new();
            let mut class_attr = String::new();
            let mut id_attr = String::new();
            let mut kv_pairs = String::new();
            if let Some(lang) = lang.filter(|l| !l.is_empty()) {
                lang_attr = format!(
                    " class=\"{}{}\"",
                    self.lang_prefix.py_str(),
                    escape_attrib_html(&lang)
                );
            }
            if !classes.is_empty() {
                class_attr = format!(" class=\"{}\"", escape_attrib_html(&classes.join(" ")));
            }
            if !id.is_empty() {
                id_attr = format!(" id=\"{}\"", escape_attrib_html(&id));
            }
            let use_pygments = config
                .iter()
                .find(|(k, _)| k == "use_pygments")
                .map(|(_, v)| v.clone());
            let pygments_truthy = match &use_pygments {
                Some(Opt::Bool(Some(true))) => true,
                Some(Opt::Str(s)) => !s.is_empty(),
                Some(Opt::Lines(l)) => !l.is_empty(),
                _ => false,
            };
            if self.use_attr_list.get() && !config.is_empty() && !pygments_truthy {
                for (k, v) in &config {
                    if k != "use_pygments" {
                        kv_pairs.push_str(&format!(
                            " {}=\"{}\"",
                            sanitize_name(k),
                            Self::render_opt(v)?
                        ));
                    }
                }
            }
            let code = Self::escape(m.name("code").unwrap_or(""));
            let html =
                format!("<pre{id_attr}{class_attr}><code{lang_attr}{kv_pairs}>{code}</code></pre>");
            let placeholder = md.html_stash.store_str(html);
            let (start, end) = (m.start(0), m.end(0));
            text = format!("{}\n{placeholder}\n{}", &text[..start], &text[end..]);
            index = start + 1 + placeholder.len();
        }
        Ok(text.split('\n').map(str::to_string).collect())
    }
}
