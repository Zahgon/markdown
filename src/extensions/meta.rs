// Meta Data Extension for Python-Markdown
// =======================================
//
// This extension adds Meta Data handling to markdown.
//
// See https://Python-Markdown.github.io/extensions/meta_data
// for documentation.
//
// Original code Copyright 2007-2008 [Waylan Limberg](https://github.com/waylan).
//
// All changes Copyright 2008-2014 The Python Markdown Project
//
// License: [BSD](https://opensource.org/licenses/bsd-license.php)

//! Meta-data: `key: value` lines at the start of a document are removed and kept in `Markdown::meta`.

use std::any::Any;
use std::rc::Rc;
use std::sync::OnceLock;

use super::Config;
use crate::core::{Extension, Markdown, Preprocessor, PyResult};
use crate::pyre::Regex;
use crate::pystr;
use crate::value::Value;

pub struct MetaExtension;

impl MetaExtension {
    pub fn create(configs: &[(String, Value)]) -> PyResult<Rc<dyn Extension>> {
        Config::default().set_all(configs)?;
        Ok(Rc::new(MetaExtension))
    }
}

impl Extension for MetaExtension {
    fn extend_markdown(&self, md: &mut Markdown, this: Rc<dyn Extension>) -> PyResult<()> {
        md.register_extension(this);
        md.preprocessors
            .register(Rc::new(MetaPreprocessor), "meta", 27.0);
        Ok(())
    }

    fn reset(&self, md: &mut Markdown) {
        md.meta = Some(Vec::new());
    }

    fn class_path(&self) -> &'static str {
        "markdown.extensions.meta.MetaExtension"
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

struct MetaPatterns {
    meta: Regex,
    more: Regex,
    begin: Regex,
    end: Regex,
}

fn patterns() -> &'static MetaPatterns {
    static P: OnceLock<MetaPatterns> = OnceLock::new();
    P.get_or_init(|| MetaPatterns {
        meta: Regex::new(r"^[ ]{0,3}(?P<key>[A-Za-z0-9_-]+):\s*(?P<value>.*)", 0),
        more: Regex::new(r"^[ ]{4,}(?P<value>.*)", 0),
        begin: Regex::new(r"^-{3}(\s.*)?", 0),
        end: Regex::new(r"^(-{3}|\.{3})(\s.*)?", 0),
    })
}

/// Read and remove the meta-data lines.
pub struct MetaPreprocessor;

impl Preprocessor for MetaPreprocessor {
    fn run(&self, md: &mut Markdown, lines: Vec<String>) -> PyResult<Vec<String>> {
        let p = patterns();
        let mut meta: Vec<(String, Vec<String>)> = Vec::new();
        let mut key: Option<String> = None;
        let mut lines: std::collections::VecDeque<String> = lines.into();
        if lines
            .front()
            .is_some_and(|l| p.begin.match_at(l, 0).is_some())
        {
            lines.pop_front();
        }
        while let Some(line) = lines.pop_front() {
            let m1 = p.meta.match_at(&line, 0);
            if pystr::strip(&line).is_empty() || p.end.match_at(&line, 0).is_some() {
                break;
            }
            if let Some(m1) = m1 {
                let k = pystr::strip(&pystr::lower(m1.name("key").unwrap_or(""))).to_string();
                let value = pystr::strip(m1.name("value").unwrap_or("")).to_string();
                match meta.iter_mut().find(|(mk, _)| *mk == k) {
                    Some(entry) => entry.1.push(value),
                    None => meta.push((k.clone(), vec![value])),
                }
                key = Some(k);
            } else {
                let m2 = p.more.match_at(&line, 0);
                match (m2, &key) {
                    (Some(m2), Some(k)) => {
                        let value = pystr::strip(m2.name("value").unwrap_or("")).to_string();
                        if let Some(entry) = meta.iter_mut().find(|(mk, _)| mk == k) {
                            entry.1.push(value);
                        }
                    }
                    _ => {
                        lines.push_front(line);
                        break;
                    }
                }
            }
        }
        md.meta = Some(meta);
        Ok(lines.into())
    }
}
