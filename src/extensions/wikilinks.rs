// WikiLinks Extension for Python-Markdown
// ======================================
//
// Converts [[WikiLinks]] to relative links.
//
// See https://Python-Markdown.github.io/extensions/wikilinks
// for documentation.
//
// Original code Copyright [Waylan Limberg](https://github.com/waylan).
//
// All changes Copyright The Python Markdown Project
//
// License: [BSD](https://opensource.org/licenses/bsd-license.php)

//! Wiki links: `[[Label]]` becomes a link built from a base URL, the label and an end URL.

use std::any::Any;
use std::rc::Rc;
use std::sync::OnceLock;

use super::Config;
use crate::core::{Extension, InlinePattern, InlineResult, Markdown, PyError, PyResult};
use crate::inlinepatterns::InlineNode;
use crate::pyre::{Match, Regex, S};
use crate::pystr;
use crate::value::Value;

pub struct WikiLinkExtension {
    config: Config,
}

impl WikiLinkExtension {
    pub fn create(configs: &[(String, Value)]) -> PyResult<Rc<dyn Extension>> {
        let mut config = Config::new(&[
            ("base_url", Value::Str("/".into())),
            ("end_url", Value::Str("/".into())),
            ("html_class", Value::Str("wikilink".into())),
            ("build_url", Value::Callable("build_url")),
        ]);
        config.set_all(configs)?;
        Ok(Rc::new(WikiLinkExtension { config }))
    }
}

impl Extension for WikiLinkExtension {
    fn extend_markdown(&self, md: &mut Markdown, _this: Rc<dyn Extension>) -> PyResult<()> {
        let pattern = WikiLinksInlineProcessor {
            re: Rc::new(Regex::new(r"\[\[([\w0-9_ -]+)\]\]", S)),
            config: self.config.clone(),
        };
        md.inline_patterns
            .register(Rc::new(pattern), "wikilink", 75.0);
        Ok(())
    }

    fn class_path(&self) -> &'static str {
        "markdown.extensions.wikilinks.WikiLinkExtension"
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// `build_url`: base, the label with runs of spaces (and spaces next to `_`) as `_`, and the end.
pub fn build_url(label: &str, base: &str, end: &str) -> String {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r"([ ]+_)|(_[ ]+)|([ ]+)", 0));
    format!("{base}{}{end}", re.sub(label, "_"))
}

struct WikiLinksInlineProcessor {
    re: Rc<Regex>,
    config: Config,
}

impl WikiLinksInlineProcessor {
    /// The base URL, end URL and class, which the document's meta-data may override.
    fn get_meta(&self, md: &Markdown) -> (Value, Value, Value) {
        let mut base = self.config.get("base_url").clone();
        let mut end = self.config.get("end_url").clone();
        let mut class = self.config.get("html_class").clone();
        if let Some(meta) = &md.meta {
            let first = |key: &str| {
                meta.iter()
                    .find(|(k, _)| k == key)
                    .and_then(|(_, v)| v.first())
                    .cloned()
            };
            if let Some(v) = first("wiki_base_url") {
                base = Value::Str(v);
            }
            if let Some(v) = first("wiki_end_url") {
                end = Value::Str(v);
            }
            if let Some(v) = first("wiki_html_class") {
                class = Value::Str(v);
            }
        }
        (base, end, class)
    }
}

impl InlinePattern for WikiLinksInlineProcessor {
    fn regex(&self) -> Rc<Regex> {
        self.re.clone()
    }

    fn handle_match(&self, md: &mut Markdown, m: &Match, _data: &str) -> PyResult<InlineResult> {
        let label = pystr::strip(m.str(1));
        if label.is_empty() {
            return Ok((
                Some(InlineNode::Str(String::new())),
                Some(m.start(0)),
                Some(m.end(0)),
            ));
        }
        let (base, end, class) = self.get_meta(md);
        let url = match self.config.get("build_url") {
            Value::Callable(_) => build_url(label, &base.py_str(), &end.py_str()),
            other => {
                return Err(PyError::new(
                    "TypeError",
                    format!("'{}' object is not callable", other.type_name()),
                ));
            }
        };
        let a = md.tree.element("a");
        md.tree.set_text_str(a, label);
        md.tree.set(a, "href", url);
        if class.truthy() {
            md.tree.set(a, "class", class.py_str());
        }
        Ok((Some(InlineNode::El(a)), Some(m.start(0)), Some(m.end(0))))
    }
}
