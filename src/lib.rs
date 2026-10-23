// Python Markdown
//
// A Python implementation of John Gruber's Markdown.
//
// - Documentation: https://python-markdown.github.io/
// - GitHub: https://github.com/Python-Markdown/markdown/
// - PyPI: https://pypi.org/project/Markdown/
//
// Started by Manfred Stienstra (http://www.dwerg.net/).
// Maintained for a few years by Yuri Takhteyev (http://www.freewisdom.org).
// Currently maintained by Waylan Limberg (https://github.com/waylan),
// Dmitry Shachnev (https://github.com/mitya57) and Isaac Muse (https://github.com/facelessuser).
//
// - Copyright 2007-2023 The Python Markdown Project (v. 1.7 and later)
// - Copyright 2004, 2005, 2006 Yuri Takhteyev (v. 0.2-1.6b)
// - Copyright 2004 Manfred Stienstra (the original version)
//
// License: BSD (see LICENSE.md for details).

//! A Rust implementation of Python-Markdown 3.11.2: John Gruber's Markdown with Python-Markdown's own
//! processing pipeline (preprocessors, block processors, inline patterns, tree processors, postprocessors and
//! its serializer) and its bundled extensions.
//!
//! ```
//! let html = markdown::markdown("*boo!*", &[], &markdown::value::Value::Dict(Vec::new()), "xhtml").unwrap();
//! assert_eq!(html, "<p><em>boo!</em></p>");
//! ```

pub mod blockprocessors;
pub mod core;
pub mod encoding;
mod encoding_tables;
pub mod entities;
mod entities_table;
pub mod etree;
pub mod extensions;
pub mod htmlparser;
pub mod inlinepatterns;
pub mod postprocessors;
pub mod preprocessors;
pub mod pyre;
pub mod pystr;
pub mod serializers;
pub mod treeprocessors;
mod unicode_tables;
pub mod util;
pub mod value;

pub use crate::core::{Markdown, PyError, PyResult};

/// The version of Python-Markdown this crate reproduces.
pub const VERSION: &str = "3.11.2";

/// Convert a Markdown string to HTML with the named extensions and their configurations.
pub fn markdown(
    text: &str,
    extensions: &[String],
    configs: &value::Value,
    output_format: &str,
) -> PyResult<String> {
    let mut md = Markdown::new(extensions, configs, output_format, core::CRITICAL)?;
    md.convert(text)
}
