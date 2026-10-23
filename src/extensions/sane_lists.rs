// Sane List Extension for Python-Markdown
// =======================================
//
// Modify the behavior of Lists in Python-Markdown to act in a sane manor.
//
// See https://Python-Markdown.github.io/extensions/sane_lists
// for documentation.
//
// Original code Copyright 2011 [Waylan Limberg](https://github.com/waylan)
//
// All changes Copyright 2011-2014 The Python Markdown Project
//
// License: [BSD](https://opensource.org/licenses/bsd-license.php)

//! Sane lists: ordered and unordered lists do not mix, and an ordered list keeps its start number.

use std::any::Any;
use std::rc::Rc;

use super::Config;
use crate::blockprocessors::ListProcessor;
use crate::core::{Extension, Markdown, PyResult};
use crate::value::Value;

pub struct SaneListExtension;

impl SaneListExtension {
    pub fn create(configs: &[(String, Value)]) -> PyResult<Rc<dyn Extension>> {
        Config::default().set_all(configs)?;
        Ok(Rc::new(SaneListExtension))
    }
}

impl Extension for SaneListExtension {
    fn extend_markdown(&self, md: &mut Markdown, _this: Rc<dyn Extension>) -> PyResult<()> {
        let tab = md.tab_length;
        md.block_processors
            .register(Rc::new(ListProcessor::olist(tab, true)), "olist", 40.0);
        md.block_processors
            .register(Rc::new(ListProcessor::ulist(tab, true)), "ulist", 30.0);
        Ok(())
    }

    fn class_path(&self) -> &'static str {
        "markdown.extensions.sane_lists.SaneListExtension"
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}
