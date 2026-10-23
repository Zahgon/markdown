// Legacy Em Extension for Python-Markdown
// =======================================
//
// This extension provides legacy behavior for _connected_words_.
//
// Copyright 2015-2018 The Python Markdown Project
//
// License: [BSD](https://opensource.org/licenses/bsd-license.php)

//! Legacy emphasis: `_` inside words starts and ends emphasis, as in Python-Markdown before 3.0.

use std::any::Any;
use std::rc::Rc;

use super::Config;
use crate::core::{Extension, Markdown, PyResult};
use crate::value::Value;

pub struct LegacyEmExtension;

impl LegacyEmExtension {
    pub fn create(configs: &[(String, Value)]) -> PyResult<Rc<dyn Extension>> {
        Config::default().set_all(configs)?;
        Ok(Rc::new(LegacyEmExtension))
    }
}

impl Extension for LegacyEmExtension {
    fn extend_markdown(&self, md: &mut Markdown, _this: Rc<dyn Extension>) -> PyResult<()> {
        match &md.delimiters {
            Some(delimiters) => delimiters.add("_", "strong,em", false, false),
            None => {
                let delimiters = Rc::new(crate::inlinepatterns::DelimiterProcessor::new());
                delimiters.add("_", "strong,em", false, false);
                md.delimiters = Some(delimiters.clone());
                md.inline_patterns.register(delimiters, "em_strong", 60.0);
            }
        }
        Ok(())
    }

    fn class_path(&self) -> &'static str {
        "markdown.extensions.legacy_em.LegacyEmExtension"
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}
