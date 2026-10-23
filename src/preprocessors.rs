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

//! Preprocessors work on the source text before it is split into blocks.

use std::rc::Rc;
use std::sync::OnceLock;

use crate::core::{Markdown, Preprocessor, PyResult};
use crate::htmlparser::HtmlExtractor;
use crate::pyre::Regex;
use crate::util::{ETX, STX};

/// Register the default preprocessors.
pub fn build_preprocessors(md: &mut Markdown) {
    md.preprocessors
        .register(Rc::new(NormalizeWhitespace), "normalize_whitespace", 30.0);
    md.preprocessors.register(
        Rc::new(HtmlBlockPreprocessor { extra: false }),
        "html_block",
        20.0,
    );
}

/// Normalize line endings, expand tabs and blank out whitespace-only lines.
pub struct NormalizeWhitespace;

impl Preprocessor for NormalizeWhitespace {
    fn run(&self, md: &mut Markdown, lines: Vec<String>) -> PyResult<Vec<String>> {
        static BLANK_LINE: OnceLock<Regex> = OnceLock::new();
        let source = lines.join("\n");
        let source = source.replace([STX, ETX], "");
        let source = source.replace("\r\n", "\n").replace('\r', "\n") + "\n\n";
        let source = crate::pystr::expandtabs(&source, md.tab_length);
        let source = BLANK_LINE
            .get_or_init(|| Regex::new(r"(?<=\n) +\n", 0))
            .sub(&source, "\n");
        Ok(source.split('\n').map(str::to_string).collect())
    }
}

/// Move block level raw HTML into the stash (`md_in_html` uses the extended extractor).
pub struct HtmlBlockPreprocessor {
    pub extra: bool,
}

impl Preprocessor for HtmlBlockPreprocessor {
    fn run(&self, md: &mut Markdown, lines: Vec<String>) -> PyResult<Vec<String>> {
        let source = lines.join("\n");
        let mut parser = HtmlExtractor::new(self.extra);
        parser.feed(md, &source)?;
        parser.close(md)?;
        Ok(parser
            .cleandoc
            .concat()
            .split('\n')
            .map(str::to_string)
            .collect())
    }
}
