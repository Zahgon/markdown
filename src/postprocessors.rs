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

//! Postprocessors work on the serialized HTML text: they put the stashed raw HTML back in place and restore
//! protected ampersands.

use std::rc::Rc;
use std::sync::OnceLock;

use crate::core::{Markdown, Postprocessor, PyResult};
use crate::pyre::Regex;
use crate::util::{Stashed, AMP_SUBSTITUTE};

/// Register the default postprocessors.
pub fn build_postprocessors(md: &mut Markdown) {
    md.postprocessors.register(
        Rc::new(RawHtmlPostprocessor { md_in_html: false }),
        "raw_html",
        30.0,
    );
    md.postprocessors
        .register(Rc::new(AndSubstitutePostprocessor), "amp_substitute", 20.0);
}

/// Restore raw HTML from the stash. A placeholder alone in a paragraph replaces the paragraph unless the HTML
/// is inline. With `md_in_html`, stashed elements are serialized first.
pub struct RawHtmlPostprocessor {
    pub md_in_html: bool,
}

fn placeholder_pattern() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        let base = crate::util::HTML_PLACEHOLDER_RE;
        Regex::new(&format!("<p>{base}</p>|{base}"), 0)
    })
}

impl RawHtmlPostprocessor {
    fn is_block_level(md: &Markdown, html: &str) -> bool {
        static RE: OnceLock<Regex> = OnceLock::new();
        let re = RE.get_or_init(|| Regex::new(r"^\<\/?([^ >]+)", 0));
        match re.match_at(html, 0) {
            Some(m) => {
                let tag = m.str(1);
                matches!(tag.chars().next(), Some('!' | '?' | '@' | '%')) || md.is_block_level(tag)
            }
            None => false,
        }
    }

    fn stash_to_string(&self, md: &Markdown, stashed: &Stashed) -> String {
        match stashed {
            Stashed::Str(s) => s.clone(),
            Stashed::El(el) if self.md_in_html => md.serialize(*el),
            // Without md_in_html an element would be printed by Python's `str()`; none is ever stashed.
            Stashed::El(_) => String::new(),
        }
    }

    fn substitute(&self, md: &Markdown, text: &str) -> String {
        placeholder_pattern().sub_fn(text, |m| {
            let (key, wrapped) = match m.group(1).filter(|k| !k.is_empty()) {
                Some(k) => (k, true),
                None => (m.str(2), false),
            };
            let key: usize = key.parse().unwrap_or(usize::MAX);
            if key >= md.html_stash.html_counter {
                return m.str(0).to_string();
            }
            let html = self.stash_to_string(md, &md.html_stash.raw_html_blocks[key]);
            if !wrapped || Self::is_block_level(md, &html) {
                self.substitute(md, &html)
            } else {
                self.substitute(md, &format!("<p>{html}</p>"))
            }
        })
    }
}

impl Postprocessor for RawHtmlPostprocessor {
    fn run(&self, md: &mut Markdown, text: String) -> PyResult<String> {
        if md.html_stash.html_counter == 0 {
            return Ok(text);
        }
        Ok(self.substitute(md, &text))
    }
}

/// Restore ampersands that were protected from escaping.
pub struct AndSubstitutePostprocessor;

impl Postprocessor for AndSubstitutePostprocessor {
    fn run(&self, _md: &mut Markdown, text: String) -> PyResult<String> {
        Ok(text.replace(AMP_SUBSTITUTE, "&"))
    }
}
