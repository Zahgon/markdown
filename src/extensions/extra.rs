// Python-Markdown Extra Extension
// ===============================
//
// A compilation of various Python-Markdown extensions that imitates
// [PHP Markdown Extra](http://michelf.com/projects/php-markdown/extra/).
//
// See https://Python-Markdown.github.io/extensions/extra
// for documentation.
//
// Copyright The Python Markdown Project
//
// License: [BSD](https://opensource.org/licenses/bsd-license.php)

//! Markdown Extra: abbr, attr_list, def_list, fenced_code, footnotes, md_in_html and tables together.

use std::any::Any;
use std::rc::Rc;

use crate::core::{Extension, Markdown, PyResult};
use crate::value::Value;

/// The extensions that make up Markdown Extra, in loading order.
const EXTENSIONS: [&str; 7] = [
    "fenced_code",
    "footnotes",
    "attr_list",
    "def_list",
    "tables",
    "abbr",
    "md_in_html",
];

pub struct ExtraExtension {
    /// The configuration of each member extension, keyed by its name.
    config: Vec<(String, Value)>,
}

impl ExtraExtension {
    pub fn create(configs: &[(String, Value)]) -> PyResult<Rc<dyn Extension>> {
        Ok(Rc::new(ExtraExtension {
            config: configs.to_vec(),
        }))
    }
}

impl Extension for ExtraExtension {
    fn extend_markdown(&self, md: &mut Markdown, _this: Rc<dyn Extension>) -> PyResult<()> {
        let names: Vec<String> = EXTENSIONS.iter().map(|s| s.to_string()).collect();
        md.register_extensions(&names, &Value::Dict(self.config.clone()))
    }

    fn class_path(&self) -> &'static str {
        "markdown.extensions.extra.ExtraExtension"
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}
