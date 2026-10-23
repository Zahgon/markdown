// Python-Markdown Markdown in HTML Extension
// ===============================
//
// An implementation of [PHP Markdown Extra](http://michelf.com/projects/php-markdown/extra/)'s
// parsing of Markdown syntax in raw HTML.
//
// See https://Python-Markdown.github.io/extensions/raw_html
// for documentation.
//
// Copyright The Python Markdown Project
//
// License: [BSD](https://opensource.org/licenses/bsd-license.php)

//! Markdown in HTML: block level raw HTML with a `markdown` attribute has its content parsed as Markdown.

use std::any::Any;
use std::rc::Rc;
use std::sync::OnceLock;

use super::Config;
use crate::core::{BlockProcessor, Extension, Markdown, PyResult};
use crate::etree::{El, Text};
use crate::postprocessors::RawHtmlPostprocessor;
use crate::preprocessors::HtmlBlockPreprocessor;
use crate::pyre::Regex;
use crate::util::Stashed;
use crate::value::Value;

pub struct MarkdownInHtmlExtension;

impl MarkdownInHtmlExtension {
    pub fn create(configs: &[(String, Value)]) -> PyResult<Rc<dyn Extension>> {
        Config::default().set_all(configs)?;
        Ok(Rc::new(MarkdownInHtmlExtension))
    }
}

impl Extension for MarkdownInHtmlExtension {
    fn extend_markdown(&self, md: &mut Markdown, _this: Rc<dyn Extension>) -> PyResult<()> {
        md.preprocessors.register(
            Rc::new(HtmlBlockPreprocessor { extra: true }),
            "html_block",
            20.0,
        );
        md.block_processors
            .register(Rc::new(MarkdownInHtmlProcessor), "markdown_block", 105.0);
        md.postprocessors.register(
            Rc::new(RawHtmlPostprocessor { md_in_html: true }),
            "raw_html",
            30.0,
        );
        Ok(())
    }

    fn class_path(&self) -> &'static str {
        "markdown.extensions.md_in_html.MarkdownInHtmlExtension"
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

fn placeholder_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(crate::util::HTML_PLACEHOLDER_RE, 0))
}

/// Put the stashed `markdown` elements back into the tree and parse their content.
pub struct MarkdownInHtmlProcessor;

impl MarkdownInHtmlProcessor {
    /// Take a stashed element out of the stash, leaving an empty string in its place.
    fn unstash(md: &mut Markdown, index: usize) {
        md.html_stash.raw_html_blocks.remove(index);
        md.html_stash
            .raw_html_blocks
            .insert(index, Stashed::Str(String::new()));
    }

    fn parse_element_content(md: &mut Markdown, element: El) -> PyResult<()> {
        let md_attr = md
            .tree
            .remove_attr(element, "markdown")
            .unwrap_or_else(|| "off".into());
        if md_attr == "block" {
            if md.tree.has_text(element) {
                let text = md.tree.text_str(element).unwrap_or("").to_string();
                let block = crate::pystr::rstrip_chars(&text, "\n").to_string();
                md.tree.set_text_str(element, "");
                md.parse_blocks(element, block.split("\n\n").map(str::to_string).collect())?;
            }
        } else if md_attr == "span" {
            let block = md.tree.text_str(element).unwrap_or("").to_string();
            md.tree.set_text_str(element, "");
            let mut child: Option<El> = None;
            let mut start = 0;
            let append = |md: &mut Markdown, child: Option<El>, piece: &str| match child {
                None => {
                    let text = format!("{}{piece}", md.tree.text_str(element).unwrap_or(""));
                    md.tree.set_text_str(element, text);
                }
                Some(c) => {
                    let tail = format!("{}{piece}", md.tree.tail_str(c).unwrap_or(""));
                    md.tree.set_tail_str(c, tail);
                }
            };
            for m in placeholder_re().finditer(&block, 0) {
                let index: usize = m.str(1).parse().unwrap_or(usize::MAX);
                let end = m.start(0);
                let stashed = md.html_stash.raw_html_blocks.get(index).cloned();
                append(md, child, &block[start..end]);
                if let Some(Stashed::El(el)) = stashed {
                    md.tree.append(element, el);
                    Self::parse_element_content(md, el)?;
                    child = Some(el);
                    if md.tree.tail(el).is_none() {
                        md.tree.set_tail_str(el, "");
                    }
                    Self::unstash(md, index);
                }
                start = end;
            }
            append(md, child, &block[start..]);
        } else {
            let text = md.tree.text_str(element).unwrap_or("").to_string();
            md.tree.set_text(element, Some(Text::atomic(text)));
            for child in md.tree.children(element).to_vec() {
                Self::parse_element_content(md, child)?;
                if let Some(tail) = md.tree.tail(child).filter(|t| !t.s.is_empty()).cloned() {
                    md.tree.set_tail(child, Some(Text::atomic(tail.s)));
                }
            }
        }
        Ok(())
    }
}

impl BlockProcessor for MarkdownInHtmlProcessor {
    fn test(&self, _md: &mut Markdown, _parent: El, _block: &str) -> bool {
        true
    }

    fn run(&self, md: &mut Markdown, parent: El, blocks: &mut Vec<String>) -> PyResult<bool> {
        let Some((index, end)) = placeholder_re()
            .match_at(&blocks[0], 0)
            .map(|m| (m.str(1).parse::<usize>().unwrap_or(usize::MAX), m.end(0)))
        else {
            return Ok(false);
        };
        let Some(Stashed::El(element)) = md.html_stash.raw_html_blocks.get(index).cloned() else {
            return Ok(false);
        };
        let block = blocks.remove(0);
        md.tree.append(parent, element);
        Self::parse_element_content(md, element)?;
        Self::unstash(md, index);
        let content = &block[end..];
        if !content.is_empty() {
            blocks.insert(0, content.to_string());
        }
        Ok(true)
    }
}
