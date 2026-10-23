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

//! Extensions and how they are found by name.
//!
//! An extension is named as Python-Markdown names it: by its entry point name (`toc`), by its module path
//! (`markdown.extensions.toc`, whose `makeExtension` builds it) or as `module:Class`. Configuration values are
//! checked like `Extension.setConfig` does: unknown options are a `KeyError` and options whose default is a
//! boolean go through `parseBoolValue`.

use std::rc::Rc;

use crate::core::{Extension, Markdown, PyError, PyResult};
use crate::value::{parse_bool_value, Value};

pub mod abbr;
pub mod admonition;
pub mod attr_list;
pub mod def_list;
pub mod extra;
pub mod fenced_code;
pub mod footnotes;
pub mod legacy_attrs;
pub mod legacy_em;
pub mod md_in_html;
pub mod meta;
pub mod nl2br;
pub mod sane_lists;
pub mod smarty;
pub mod tables;
pub mod toc;
pub mod wikilinks;

/// The configuration options of an extension: name, value, in definition order.
#[derive(Clone, Debug, Default)]
pub struct Config {
    items: Vec<(String, Value)>,
}

impl Config {
    pub fn new(defaults: &[(&str, Value)]) -> Config {
        Config {
            items: defaults
                .iter()
                .map(|(k, v)| (k.to_string(), v.clone()))
                .collect(),
        }
    }

    /// `Extension.setConfig`.
    pub fn set(&mut self, key: &str, value: Value) -> PyResult<()> {
        let slot = self
            .items
            .iter_mut()
            .find(|(k, _)| k == key)
            .ok_or_else(|| PyError::key_error(key))?;
        let value = match slot.1 {
            Value::Bool(_) => match parse_bool_value(&value, true, false) {
                Ok(b) => Value::Bool(b.unwrap_or(false)),
                Err(message) => return Err(PyError::new("ValueError", message)),
            },
            Value::None => match parse_bool_value(&value, true, true) {
                Ok(Some(b)) => Value::Bool(b),
                Ok(None) => Value::None,
                Err(message) => return Err(PyError::new("ValueError", message)),
            },
            _ => value,
        };
        slot.1 = value;
        Ok(())
    }

    /// `Extension.setConfigs`.
    pub fn set_all(&mut self, items: &[(String, Value)]) -> PyResult<()> {
        for (k, v) in items {
            self.set(k, v.clone())?;
        }
        Ok(())
    }

    /// `Extension.getConfig(key)`.
    pub fn get(&self, key: &str) -> &Value {
        self.items
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v)
            .unwrap_or(&Value::None)
    }

    /// A string option; other values are converted as `str()` would.
    pub fn get_str(&self, key: &str) -> String {
        self.get(key).py_str()
    }
}

type Ctor = fn(&[(String, Value)]) -> PyResult<Rc<dyn Extension>>;

/// A bundled extension: entry point name, module, class and constructor.
struct Known {
    entry: &'static str,
    module: &'static str,
    class: &'static str,
    ctor: Ctor,
}

const KNOWN: &[Known] = &[
    Known {
        entry: "abbr",
        module: "markdown.extensions.abbr",
        class: "AbbrExtension",
        ctor: abbr::AbbrExtension::create,
    },
    Known {
        entry: "admonition",
        module: "markdown.extensions.admonition",
        class: "AdmonitionExtension",
        ctor: admonition::AdmonitionExtension::create,
    },
    Known {
        entry: "attr_list",
        module: "markdown.extensions.attr_list",
        class: "AttrListExtension",
        ctor: attr_list::AttrListExtension::create,
    },
    Known {
        entry: "def_list",
        module: "markdown.extensions.def_list",
        class: "DefListExtension",
        ctor: def_list::DefListExtension::create,
    },
    Known {
        entry: "extra",
        module: "markdown.extensions.extra",
        class: "ExtraExtension",
        ctor: extra::ExtraExtension::create,
    },
    Known {
        entry: "fenced_code",
        module: "markdown.extensions.fenced_code",
        class: "FencedCodeExtension",
        ctor: fenced_code::FencedCodeExtension::create,
    },
    Known {
        entry: "footnotes",
        module: "markdown.extensions.footnotes",
        class: "FootnoteExtension",
        ctor: footnotes::FootnoteExtension::create,
    },
    Known {
        entry: "md_in_html",
        module: "markdown.extensions.md_in_html",
        class: "MarkdownInHtmlExtension",
        ctor: md_in_html::MarkdownInHtmlExtension::create,
    },
    Known {
        entry: "meta",
        module: "markdown.extensions.meta",
        class: "MetaExtension",
        ctor: meta::MetaExtension::create,
    },
    Known {
        entry: "nl2br",
        module: "markdown.extensions.nl2br",
        class: "Nl2BrExtension",
        ctor: nl2br::Nl2BrExtension::create,
    },
    Known {
        entry: "sane_lists",
        module: "markdown.extensions.sane_lists",
        class: "SaneListExtension",
        ctor: sane_lists::SaneListExtension::create,
    },
    Known {
        entry: "smarty",
        module: "markdown.extensions.smarty",
        class: "SmartyExtension",
        ctor: smarty::SmartyExtension::create,
    },
    Known {
        entry: "tables",
        module: "markdown.extensions.tables",
        class: "TableExtension",
        ctor: tables::TableExtension::create,
    },
    Known {
        entry: "toc",
        module: "markdown.extensions.toc",
        class: "TocExtension",
        ctor: toc::TocExtension::create,
    },
    Known {
        entry: "wikilinks",
        module: "markdown.extensions.wikilinks",
        class: "WikiLinkExtension",
        ctor: wikilinks::WikiLinkExtension::create,
    },
    Known {
        entry: "legacy_attrs",
        module: "markdown.extensions.legacy_attrs",
        class: "LegacyAttrExtension",
        ctor: legacy_attrs::LegacyAttrExtension::create,
    },
    Known {
        entry: "legacy_em",
        module: "markdown.extensions.legacy_em",
        class: "LegacyEmExtension",
        ctor: legacy_em::LegacyEmExtension::create,
    },
];

/// The modules of the `markdown` package that are not extensions themselves.
const PACKAGE_MODULES: &[&str] = &[
    "markdown",
    "markdown.__main__",
    "markdown.__meta__",
    "markdown.blockparser",
    "markdown.blockprocessors",
    "markdown.core",
    "markdown.extensions",
    "markdown.htmlparser",
    "markdown.inlinepatterns",
    "markdown.postprocessors",
    "markdown.preprocessors",
    "markdown.serializers",
    "markdown.test_tools",
    "markdown.treeprocessors",
    "markdown.util",
];

/// The packages, which can contain modules.
const PACKAGES: &[&str] = &["markdown", "markdown.extensions"];

/// Build an extension from its entry point name, if `name` is one.
pub fn by_entry_point(
    name: &str,
    configs: &[(String, Value)],
) -> Option<PyResult<Rc<dyn Extension>>> {
    KNOWN
        .iter()
        .find(|k| k.entry == name)
        .map(|k| (k.ctor)(configs))
}

/// `importlib.import_module(name)`: whether the module exists, or Python's error.
pub fn import_module(name: &str) -> PyResult<()> {
    if name.is_empty() {
        return Err(PyError::new("ValueError", "Empty module name"));
    }
    if name.starts_with('.') {
        return Err(PyError::new(
            "TypeError",
            format!(
                "the 'package' argument is required to perform a relative import for {}",
                crate::pystr::repr_str(name)
            ),
        ));
    }
    if KNOWN.iter().any(|k| k.module == name) || PACKAGE_MODULES.contains(&name) {
        return Ok(());
    }
    // Import the parents first: the first missing one is the error.
    let parts: Vec<&str> = name.split('.').collect();
    for n in 1..=parts.len() {
        let prefix = parts[..n].join(".");
        let exists =
            KNOWN.iter().any(|k| k.module == prefix) || PACKAGE_MODULES.contains(&prefix.as_str());
        if exists {
            continue;
        }
        if n > 1 {
            let parent = parts[..n - 1].join(".");
            if !PACKAGES.contains(&parent.as_str()) {
                return Err(PyError::new(
                    "ModuleNotFoundError",
                    format!("No module named '{prefix}'; '{parent}' is not a package"),
                ));
            }
        }
        return Err(PyError::new(
            "ModuleNotFoundError",
            format!("No module named '{prefix}'"),
        ));
    }
    Ok(())
}

/// The extension classes a module has as attributes.
fn classes_of(module: &str) -> Vec<(&'static str, Option<Ctor>)> {
    let mut out: Vec<(&'static str, Option<Ctor>)> = Vec::new();
    if let Some(k) = KNOWN.iter().find(|k| k.module == module) {
        out.push((k.class, Some(k.ctor)));
        out.push(("Extension", None));
        if module == "markdown.extensions.fenced_code" {
            out.push((
                "AttrListExtension",
                Some(attr_list::AttrListExtension::create),
            ));
        }
    } else if module == "markdown.extensions" || module == "markdown.core" {
        out.push(("Extension", None));
    }
    out
}

/// `getattr(module, class_name)(**configs)`.
pub fn by_class(
    module: &str,
    class_name: &str,
    configs: &[(String, Value)],
) -> PyResult<Rc<dyn Extension>> {
    match classes_of(module)
        .into_iter()
        .find(|(c, _)| *c == class_name)
    {
        Some((_, Some(ctor))) => ctor(configs),
        Some((_, None)) => BaseExtension::create(configs),
        None => Err(PyError::new(
            "AttributeError",
            format!("module '{module}' has no attribute '{class_name}'"),
        )),
    }
}

/// `module.makeExtension(**configs)`.
pub fn make_extension(module: &str, configs: &[(String, Value)]) -> PyResult<Rc<dyn Extension>> {
    match KNOWN.iter().find(|k| k.module == module) {
        Some(k) => (k.ctor)(configs),
        None => {
            // Python suggests a similar name: the modules that import `Extension` get it suggested.
            let hint = if ["markdown", "markdown.core", "markdown.extensions"].contains(&module) {
                ". Did you mean: 'Extension'?"
            } else {
                ""
            };
            Err(PyError::new(
                "AttributeError",
                format!("module '{module}' has no attribute 'makeExtension'{hint}"),
            ))
        }
    }
}

/// The base `Extension` class: it has no options and cannot extend a converter.
pub struct BaseExtension;

impl BaseExtension {
    fn create(configs: &[(String, Value)]) -> PyResult<Rc<dyn Extension>> {
        Config::default().set_all(configs)?;
        Ok(Rc::new(BaseExtension))
    }
}

impl Extension for BaseExtension {
    fn extend_markdown(&self, _md: &mut Markdown, _this: Rc<dyn Extension>) -> PyResult<()> {
        Err(PyError::new(
            "NotImplementedError",
            "Extension \"markdown.extensions.Extension\" must define an \"extendMarkdown\"method.",
        ))
    }

    fn class_path(&self) -> &'static str {
        "markdown.extensions.Extension"
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
