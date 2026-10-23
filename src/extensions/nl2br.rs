// `NL2BR` Extension
// ===============
//
// A Python-Markdown extension to treat newlines as hard breaks; like
// GitHub-flavored Markdown does.
//
// See https://Python-Markdown.github.io/extensions/nl2br
// for documentation.
//
// Original code Copyright 2011 [Brian Neal](https://deathofagremmie.com/)
//
// All changes Copyright 2011-2014 The Python Markdown Project
//
// License: [BSD](https://opensource.org/licenses/bsd-license.php)

//! Treat every newline in a paragraph as a hard line break.

use std::any::Any;
use std::rc::Rc;

use super::Config;
use crate::core::{Extension, Markdown, PyResult};
use crate::inlinepatterns::SubstituteTagInlineProcessor;
use crate::value::Value;

pub struct Nl2BrExtension;

impl Nl2BrExtension {
    pub fn create(configs: &[(String, Value)]) -> PyResult<Rc<dyn Extension>> {
        Config::default().set_all(configs)?;
        Ok(Rc::new(Nl2BrExtension))
    }
}

impl Extension for Nl2BrExtension {
    fn extend_markdown(&self, md: &mut Markdown, _this: Rc<dyn Extension>) -> PyResult<()> {
        md.inline_patterns.register(
            Rc::new(SubstituteTagInlineProcessor::new(r"\n", "br")),
            "nl",
            5.0,
        );
        Ok(())
    }

    fn class_path(&self) -> &'static str {
        "markdown.extensions.nl2br.Nl2BrExtension"
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}
