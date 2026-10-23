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

//! Inline patterns: code spans, escapes, links, images, autolinks, raw HTML, entities, line breaks and the
//! delimiter based emphasis processor. The inline tree processor applies them in priority order.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

use crate::core::{InlinePattern, InlineResult, Markdown, PyResult};
use crate::etree::{El, Text};
use crate::pyre::{Match, Regex, S};
use crate::pystr;
use crate::treeprocessors::{InlineProcessor, StashNode};
use crate::util::{code_escape, AMP_SUBSTITUTE, ETX, STX};

/// What an inline pattern puts in place of the text it matched.
#[derive(Clone, Debug)]
pub enum InlineNode {
    El(El),
    Str(String),
}

const NOIMG: &str = r"(?<!\!)";
pub const BACKTICK_RE: &str = r"(?:(?<!\\)((?:\\{2})+)(?=`+)|(?<!\\)`)";
pub const ESCAPE_RE: &str = r"\\(.)";
pub const IMAGE_LINK_RE: &str = r"\!\[";
pub const AUTOLINK_RE: &str = r"<((?:[Ff]|[Hh][Tt])[Tt][Pp][Ss]?://[^<>]*)>";
pub const AUTOMAIL_RE: &str = r"<([^<> !]+@[^@<> ]+)>";
pub const HTML_RE: &str = concat!(
    r#"(<(\/?+[a-zA-Z][^\s"'<>@]*+(?:\s+[^\s"'=<>]++(?:\s*+=\s*+(?:"[^"]*+"|'[^']*+'|[^\s"'=<>]++))?+)*+\s*+/?|"#,
    r"!--(?:(?!<!--|-->).)*--|",
    r"[?](?:(?!<[?]|[?]>).)*[?]|",
    r"!\[CDATA\[(?:(?!<!\[CDATA\[|\]\]>).)*\]\]",
    ")>)"
);
pub const ENTITY_RE: &str = r"(&(?:\#[0-9]+|\#x[0-9a-fA-F]+|[a-zA-Z0-9]+);)";
pub const LINE_BREAK_RE: &str = "  \n";

/// The link pattern: a `[` that does not start an image.
pub fn link_re() -> String {
    format!(r"{NOIMG}\[")
}

/// Register the default inline patterns.
pub fn build_inlinepatterns(md: &mut Markdown) {
    let link = link_re();
    md.inline_patterns
        .register(Rc::new(BacktickInlineProcessor::new()), "backtick", 190.0);
    md.inline_patterns
        .register(Rc::new(EscapeInlineProcessor::new()), "escape", 180.0);
    md.inline_patterns.register(
        Rc::new(LinkLike::new(&link, LinkKind::Reference)),
        "reference",
        170.0,
    );
    md.inline_patterns
        .register(Rc::new(LinkLike::new(&link, LinkKind::Link)), "link", 160.0);
    md.inline_patterns.register(
        Rc::new(LinkLike::new(IMAGE_LINK_RE, LinkKind::Image)),
        "image_link",
        150.0,
    );
    md.inline_patterns.register(
        Rc::new(LinkLike::new(IMAGE_LINK_RE, LinkKind::ImageReference)),
        "image_reference",
        140.0,
    );
    md.inline_patterns.register(
        Rc::new(LinkLike::new(&link, LinkKind::ShortReference)),
        "short_reference",
        130.0,
    );
    md.inline_patterns.register(
        Rc::new(LinkLike::new(IMAGE_LINK_RE, LinkKind::ShortImageReference)),
        "short_image_ref",
        125.0,
    );
    md.inline_patterns
        .register(Rc::new(AutolinkInlineProcessor::new()), "autolink", 120.0);
    md.inline_patterns
        .register(Rc::new(AutomailInlineProcessor::new()), "automail", 110.0);
    md.inline_patterns.register(
        Rc::new(SubstituteTagInlineProcessor::new(LINE_BREAK_RE, "br")),
        "linebreak",
        100.0,
    );
    md.inline_patterns
        .register(Rc::new(HtmlInlineProcessor::new(HTML_RE)), "html", 90.0);
    md.inline_patterns
        .register(Rc::new(HtmlInlineProcessor::new(ENTITY_RE)), "entity", 80.0);
    let delimiters = Rc::new(DelimiterProcessor::new());
    delimiters.add("*", "strong,em", false, false);
    md.delimiters = Some(delimiters.clone());
    md.inline_patterns
        .register(delimiters.clone(), "em_strong", 60.0);
    delimiters.add("_", "strong,em", true, false);
}

/// `dequote`: remove one pair of matching quotes.
fn dequote(s: &str) -> &str {
    if s.len() >= 2
        && ((s.starts_with('"') && s.ends_with('"')) || (s.starts_with('\'') && s.ends_with('\'')))
    {
        &s[1..s.len() - 1]
    } else if s == "\"" || s == "'" {
        // A lone quote both starts and ends with itself.
        ""
    } else {
        s
    }
}

/// The stash of the `inline` tree processor, if it is registered.
fn inline_stash(md: &Markdown) -> Option<Rc<dyn crate::core::Treeprocessor>> {
    md.inline_treeprocessor()
}

fn placeholder_re() -> &'static Regex {
    static RE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    RE.get_or_init(|| Regex::new(crate::util::INLINE_PLACEHOLDER_RE, 0))
}

/// `Pattern.unescape`: replace inline placeholders with the text of the stashed nodes.
pub fn unescape_placeholders(md: &Markdown, text: &str) -> String {
    let Some(tp) = inline_stash(md) else {
        return text.to_string();
    };
    let Some(inline) = tp.as_any().downcast_ref::<InlineProcessor>() else {
        return text.to_string();
    };
    placeholder_re().sub_fn(text, |m| match inline.stashed(m.str(1)) {
        Some(StashNode::Str(s)) => s,
        Some(StashNode::El(el)) => md.tree.itertext(el),
        None => String::new(),
    })
}

/// A code span, or an even run of backslashes before backticks.
pub struct BacktickInlineProcessor {
    re: Rc<Regex>,
}

impl BacktickInlineProcessor {
    pub fn new() -> BacktickInlineProcessor {
        BacktickInlineProcessor {
            re: Rc::new(Regex::new(BACKTICK_RE, S)),
        }
    }

    /// `find_code_spans` over the characters of the text: the span of the code between the opening run of
    /// backticks at `start` and its closing run (Python's arithmetic, in character indices).
    fn find_code_spans(start: usize, text: &[char]) -> Option<(usize, usize)> {
        let last = text.len();
        let mut start = start;
        let mut max_ticks = 0;
        while start < last && text[start] == '`' {
            max_ticks += 1;
            start += 1;
        }
        if max_ticks == 0 {
            return None;
        }
        let mut longest_span = 0;
        let mut end = 0;
        let mut i = start;
        while i < last {
            let mut span_length = 0;
            while i < last && text[i] == '`' {
                span_length += 1;
                i += 1;
            }
            if span_length == 0 {
                i += 1;
                continue;
            }
            if max_ticks == span_length {
                return Some((start, i - span_length));
            }
            if span_length > longest_span {
                longest_span = span_length;
                end = i;
            }
        }
        if longest_span > 0 {
            let start = (start as isize - (max_ticks as isize - longest_span as isize)) as usize;
            return Some((start, end - longest_span));
        }
        None
    }
}

impl Default for BacktickInlineProcessor {
    fn default() -> Self {
        Self::new()
    }
}

impl InlinePattern for BacktickInlineProcessor {
    fn regex(&self) -> Rc<Regex> {
        self.re.clone()
    }

    fn handle_match(&self, _md: &mut Markdown, m: &Match, data: &str) -> PyResult<InlineResult> {
        if let Some(slashes) = m.group(1).filter(|s| !s.is_empty()) {
            let escaped = format!("{STX}{}{ETX}", '\\' as u32);
            return Ok((
                Some(InlineNode::Str(slashes.replace("\\\\", &escaped))),
                Some(m.start(0)),
                Some(m.end(0)),
            ));
        }
        let index = pystr::CharIndex::new(data);
        let chars: Vec<char> = data.chars().collect();
        let begin = index.to_char(m.start(0));
        match Self::find_code_spans(begin, &chars) {
            Some((start, end)) => {
                let el = _md.tree.element("code");
                let code = code_escape(pystr::strip(index.slice(
                    data,
                    start as isize,
                    end as isize,
                )));
                _md.tree.set_text(el, Some(Text::atomic(code)));
                let stop = (end as isize + (start as isize - begin as isize)).max(0) as usize;
                Ok((
                    Some(InlineNode::El(el)),
                    Some(m.start(0)),
                    Some(index.to_byte(stop)),
                ))
            }
            None => Ok((None, None, None)),
        }
    }
}

/// A backslash escape.
pub struct EscapeInlineProcessor {
    re: Rc<Regex>,
}

impl EscapeInlineProcessor {
    pub fn new() -> EscapeInlineProcessor {
        EscapeInlineProcessor {
            re: Rc::new(Regex::new(ESCAPE_RE, S)),
        }
    }
}

impl Default for EscapeInlineProcessor {
    fn default() -> Self {
        Self::new()
    }
}

impl InlinePattern for EscapeInlineProcessor {
    fn regex(&self) -> Rc<Regex> {
        self.re.clone()
    }

    fn handle_match(&self, md: &mut Markdown, m: &Match, _data: &str) -> PyResult<InlineResult> {
        let c = m.str(1).chars().next().unwrap_or('\0');
        if md.escaped_chars.contains(&c) {
            Ok((
                Some(InlineNode::Str(format!("{STX}{}{ETX}", c as u32))),
                Some(m.start(0)),
                Some(m.end(0)),
            ))
        } else {
            Ok((None, Some(m.start(0)), Some(m.end(0))))
        }
    }
}

/// The kinds of link and image patterns.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum LinkKind {
    Link,
    Image,
    Reference,
    ShortReference,
    ImageReference,
    ShortImageReference,
}

/// Links and images, inline or by reference.
pub struct LinkLike {
    re: Rc<Regex>,
    kind: LinkKind,
}

struct LinkPatterns {
    re_link: Regex,
    title_clean: Regex,
    newline_cleanup: Regex,
    ref_link: Regex,
}

fn link_patterns() -> &'static LinkPatterns {
    static P: std::sync::OnceLock<LinkPatterns> = std::sync::OnceLock::new();
    P.get_or_init(|| LinkPatterns {
        re_link: Regex::new(r#"\(\s*(?:(<[^<>]*>)\s*(?:('[^']*'|"[^"]*")\s*)?\))?"#, S),
        title_clean: Regex::new(r"\s", 0),
        newline_cleanup: Regex::new(r"\s+", crate::pyre::M),
        ref_link: Regex::new(r"\s?\[([^\]]*)\]", S),
    })
}

impl LinkLike {
    pub fn new(pattern: &str, kind: LinkKind) -> LinkLike {
        LinkLike {
            re: Rc::new(Regex::new(pattern, S)),
            kind,
        }
    }

    /// `getText`: the bracketed text, with nested brackets balanced.
    fn get_text(data: &str, index: usize) -> (String, usize, bool) {
        let mut bracket_count = 1;
        let mut text = String::new();
        let mut index = index;
        for c in data[index..].chars() {
            match c {
                ']' => bracket_count -= 1,
                '[' => bracket_count += 1,
                _ => {}
            }
            index += c.len_utf8();
            if bracket_count == 0 {
                break;
            }
            text.push(c);
        }
        (text, index, bracket_count == 0)
    }

    /// `getLink`: the parenthesized destination and title.
    fn get_link(md: &Markdown, data: &str, index: usize) -> (String, Option<String>, usize, bool) {
        let p = link_patterns();
        let mut href = String::new();
        let mut title: Option<String> = None;
        let mut handled = false;
        let mut index = index;
        let m = p.re_link.match_at(data, index);
        match &m {
            Some(m) if m.group(1).is_some_and(|g| !g.is_empty()) => {
                let g1 = m.str(1);
                href = pystr::strip(&g1[1..g1.len() - 1]).to_string();
                if let Some(g2) = m.group(2).filter(|g| !g.is_empty()) {
                    title = Some(g2[1..g2.len() - 1].to_string());
                }
                index = m.end(0);
                handled = true;
            }
            Some(m) => {
                let mut bracket_count: i64 = 1;
                let mut backtrack_count: i64 = 1;
                let start_index = m.end(0);
                index = start_index;
                let mut last_bracket: isize = -1;
                let mut quote: Option<char> = None;
                let mut start_quote: isize = -1;
                let mut exit_quote: isize = -1;
                let mut ignore_matches = false;
                let mut alt_quote: Option<char> = None;
                let mut start_alt_quote: isize = -1;
                let mut exit_alt_quote: isize = -1;
                let mut last: Option<char> = None;
                for c in data[start_index..].chars() {
                    if c == '(' {
                        if !ignore_matches {
                            bracket_count += 1;
                        } else if backtrack_count > 0 {
                            backtrack_count -= 1;
                        }
                    } else if c == ')' {
                        if (exit_quote != -1 && quote == last)
                            || (exit_alt_quote != -1 && alt_quote == last)
                        {
                            bracket_count = 0;
                        } else if !ignore_matches {
                            bracket_count -= 1;
                        } else if backtrack_count > 0 {
                            backtrack_count -= 1;
                            if backtrack_count == 0 {
                                last_bracket = index as isize + 1;
                            }
                        }
                    } else if c == '\'' || c == '"' {
                        if quote.is_none() {
                            ignore_matches = true;
                            backtrack_count = bracket_count;
                            bracket_count = 1;
                            start_quote = index as isize + 1;
                            quote = Some(c);
                        } else if Some(c) != quote && alt_quote.is_none() {
                            start_alt_quote = index as isize + 1;
                            alt_quote = Some(c);
                        } else if Some(c) == quote {
                            exit_quote = index as isize + 1;
                        } else if alt_quote.is_some() && Some(c) == alt_quote {
                            exit_alt_quote = index as isize + 1;
                        }
                    }
                    index += c.len_utf8();
                    if bracket_count == 0 {
                        if exit_quote >= 0 && quote == last {
                            href = slice(data, start_index as isize, start_quote - 1).to_string();
                            title = Some(slice(data, start_quote, exit_quote - 1).to_string());
                        } else if exit_alt_quote >= 0 && alt_quote == last {
                            href =
                                slice(data, start_index as isize, start_alt_quote - 1).to_string();
                            title =
                                Some(slice(data, start_alt_quote, exit_alt_quote - 1).to_string());
                        } else {
                            href =
                                slice(data, start_index as isize, index as isize - 1).to_string();
                        }
                        break;
                    }
                    if c != ' ' {
                        last = Some(c);
                    }
                }
                if bracket_count != 0 && backtrack_count == 0 {
                    href = slice(data, start_index as isize, last_bracket - 1).to_string();
                    // A negative index counts from the end, as Python's slicing does with it later.
                    index = if last_bracket < 0 {
                        let back = data
                            .chars()
                            .rev()
                            .take(last_bracket.unsigned_abs())
                            .map(char::len_utf8)
                            .sum::<usize>();
                        data.len() - back
                    } else {
                        last_bracket as usize
                    };
                    bracket_count = 0;
                }
                handled = bracket_count == 0;
            }
            None => {}
        }
        if let Some(t) = title {
            let cleaned = unescape_placeholders(md, pystr::strip(&t));
            title = Some(p.title_clean.sub(dequote(&cleaned), " "));
        }
        let href = pystr::strip(&unescape_placeholders(md, &href)).to_string();
        (href, title, index, handled)
    }

    /// `evalId` of the reference patterns.
    fn eval_id(&self, data: &str, index: usize, text: &str) -> (Option<String>, usize, bool) {
        if matches!(
            self.kind,
            LinkKind::ShortReference | LinkKind::ShortImageReference
        ) {
            return (Some(pystr::lower(text)), index, true);
        }
        match link_patterns().ref_link.match_at(data, index) {
            None => (None, index, false),
            Some(m) => {
                let mut id = pystr::lower(m.str(1));
                if id.is_empty() {
                    id = pystr::lower(text);
                }
                (Some(id), m.end(0), true)
            }
        }
    }
}

/// `data[a:b]` with Python's handling of negative and reversed bounds.
fn slice(data: &str, a: isize, b: isize) -> &str {
    let len = data.len() as isize;
    let norm = |x: isize| if x < 0 { (len + x).max(0) } else { x.min(len) } as usize;
    let (a, b) = (norm(a), norm(b));
    if a >= b {
        ""
    } else {
        &data[a..b]
    }
}

impl InlinePattern for LinkLike {
    fn regex(&self) -> Rc<Regex> {
        self.re.clone()
    }

    fn handle_match(&self, md: &mut Markdown, m: &Match, data: &str) -> PyResult<InlineResult> {
        let (text, index, handled) = Self::get_text(data, m.end(0));
        if !handled {
            return Ok((None, None, None));
        }
        match self.kind {
            LinkKind::Link | LinkKind::Image => {
                let (href, title, index, handled) = Self::get_link(md, data, index);
                if !handled {
                    return Ok((None, None, None));
                }
                let el;
                if self.kind == LinkKind::Link {
                    el = md.tree.element("a");
                    md.tree.set_text_str(el, text);
                    md.tree.set(el, "href", href);
                    if let Some(t) = title {
                        md.tree.set(el, "title", t);
                    }
                } else {
                    el = md.tree.element("img");
                    md.tree.set(el, "src", href);
                    if let Some(t) = title {
                        md.tree.set(el, "title", t);
                    }
                    let alt = unescape_placeholders(md, &text);
                    md.tree.set(el, "alt", alt);
                }
                Ok((Some(InlineNode::El(el)), Some(m.start(0)), Some(index)))
            }
            _ => {
                let (id, end, handled) = self.eval_id(data, index, &text);
                if !handled {
                    return Ok((None, None, None));
                }
                let id = link_patterns()
                    .newline_cleanup
                    .sub(&id.unwrap_or_default(), " ");
                let Some((href, title)) = md.references.get(&id).cloned() else {
                    return Ok((None, Some(m.start(0)), Some(end)));
                };
                let image = matches!(
                    self.kind,
                    LinkKind::ImageReference | LinkKind::ShortImageReference
                );
                let el;
                if image {
                    el = md.tree.element("img");
                    md.tree.set(el, "src", href);
                    if let Some(t) = title.filter(|t| !t.is_empty()) {
                        md.tree.set(el, "title", t);
                    }
                    let alt = unescape_placeholders(md, &text);
                    md.tree.set(el, "alt", alt);
                } else {
                    el = md.tree.element("a");
                    md.tree.set(el, "href", href);
                    if let Some(t) = title.filter(|t| !t.is_empty()) {
                        md.tree.set(el, "title", t);
                    }
                    md.tree.set_text_str(el, text);
                }
                Ok((Some(InlineNode::El(el)), Some(m.start(0)), Some(end)))
            }
        }
    }
}

/// `<http://...>` autolinks.
pub struct AutolinkInlineProcessor {
    re: Rc<Regex>,
}

impl AutolinkInlineProcessor {
    pub fn new() -> AutolinkInlineProcessor {
        AutolinkInlineProcessor {
            re: Rc::new(Regex::new(AUTOLINK_RE, S)),
        }
    }
}

impl Default for AutolinkInlineProcessor {
    fn default() -> Self {
        Self::new()
    }
}

impl InlinePattern for AutolinkInlineProcessor {
    fn regex(&self) -> Rc<Regex> {
        self.re.clone()
    }

    fn handle_match(&self, md: &mut Markdown, m: &Match, _data: &str) -> PyResult<InlineResult> {
        let el = md.tree.element("a");
        let href = unescape_placeholders(md, m.str(1));
        md.tree.set(el, "href", href);
        md.tree.set_text(el, Some(Text::atomic(m.str(1))));
        Ok((Some(InlineNode::El(el)), Some(m.start(0)), Some(m.end(0))))
    }
}

/// `<user@example.com>` automatic e-mail links, obfuscated with character references.
pub struct AutomailInlineProcessor {
    re: Rc<Regex>,
}

impl AutomailInlineProcessor {
    pub fn new() -> AutomailInlineProcessor {
        AutomailInlineProcessor {
            re: Rc::new(Regex::new(AUTOMAIL_RE, S)),
        }
    }
}

impl Default for AutomailInlineProcessor {
    fn default() -> Self {
        Self::new()
    }
}

impl InlinePattern for AutomailInlineProcessor {
    fn regex(&self) -> Rc<Regex> {
        self.re.clone()
    }

    fn handle_match(&self, md: &mut Markdown, m: &Match, _data: &str) -> PyResult<InlineResult> {
        let el = md.tree.element("a");
        let mut email = unescape_placeholders(md, m.str(1));
        if let Some(rest) = email.strip_prefix("mailto:") {
            email = rest.to_string();
        }
        let letters: String = email
            .chars()
            .map(|c| match crate::entities::codepoint2name(c as u32) {
                Some(name) => format!("{AMP_SUBSTITUTE}{name};"),
                None => format!("{AMP_SUBSTITUTE}#{};", c as u32),
            })
            .collect();
        md.tree.set_text(el, Some(Text::atomic(letters)));
        let mailto: String = format!("mailto:{email}")
            .chars()
            .map(|c| format!("{AMP_SUBSTITUTE}#{};", c as u32))
            .collect();
        md.tree.set(el, "href", mailto);
        Ok((Some(InlineNode::El(el)), Some(m.start(0)), Some(m.end(0))))
    }
}

/// A pattern replaced by an empty element (`<br>`).
pub struct SubstituteTagInlineProcessor {
    re: Rc<Regex>,
    tag: &'static str,
}

impl SubstituteTagInlineProcessor {
    pub fn new(pattern: &str, tag: &'static str) -> SubstituteTagInlineProcessor {
        SubstituteTagInlineProcessor {
            re: Rc::new(Regex::new(pattern, S)),
            tag,
        }
    }
}

impl InlinePattern for SubstituteTagInlineProcessor {
    fn regex(&self) -> Rc<Regex> {
        self.re.clone()
    }

    fn handle_match(&self, md: &mut Markdown, m: &Match, _data: &str) -> PyResult<InlineResult> {
        let el = md.tree.element(self.tag);
        Ok((Some(InlineNode::El(el)), Some(m.start(0)), Some(m.end(0))))
    }
}

/// Inline raw HTML and entities: stored in the stash and replaced with a placeholder.
pub struct HtmlInlineProcessor {
    re: Rc<Regex>,
}

impl HtmlInlineProcessor {
    pub fn new(pattern: &str) -> HtmlInlineProcessor {
        HtmlInlineProcessor {
            re: Rc::new(Regex::new(pattern, S)),
        }
    }
}

/// `HtmlInlineProcessor.unescape`: restore stashed nodes as HTML source, and stashed strings as escapes.
fn html_unescape_placeholders(md: &Markdown, text: &str) -> String {
    let Some(tp) = inline_stash(md) else {
        return text.to_string();
    };
    let Some(inline) = tp.as_any().downcast_ref::<InlineProcessor>() else {
        return text.to_string();
    };
    placeholder_re().sub_fn(text, |m| match inline.stashed(m.str(1)) {
        Some(StashNode::El(el)) => html_unescape_placeholders(md, &md.serialize(el)),
        Some(StashNode::Str(s)) => format!("\\{s}"),
        None => String::new(),
    })
}

/// `HtmlInlineProcessor.backslash_unescape`: turn escape placeholders back into their characters.
pub fn backslash_unescape(md: &Markdown, text: &str) -> String {
    if !md.treeprocessors.contains("unescape") {
        return text.to_string();
    }
    crate::treeprocessors::unescape_escapes(text)
}

impl InlinePattern for HtmlInlineProcessor {
    fn regex(&self) -> Rc<Regex> {
        self.re.clone()
    }

    fn handle_match(&self, md: &mut Markdown, m: &Match, _data: &str) -> PyResult<InlineResult> {
        let rawhtml = backslash_unescape(md, &html_unescape_placeholders(md, m.str(1)));
        let placeholder = md.html_stash.store_str(rawhtml);
        Ok((
            Some(InlineNode::Str(placeholder)),
            Some(m.start(0)),
            Some(m.end(0)),
        ))
    }
}

/// A delimiter stack entry: start, end, whether it is ambiguous, and the remaining length.
type Entry = (usize, usize, bool, usize);
/// An emphasis region: opener start and end, closer start and end, the tags, and the size (1 or 2).
type Region = (usize, usize, usize, usize, (String, String), usize);

/// One delimiter token (`*` or `_`) of the emphasis processor.
struct Delimiter {
    stack: VecDeque<Entry>,
    tag_count: usize,
    tags: (String, String),
    double: bool,
    max_size: usize,
    boundary: Regex,
    bad: Regex,
}

impl Delimiter {
    fn new(token: &str, tags: &str, smart: bool, double: bool) -> Delimiter {
        let temp: Vec<&str> = tags.split(',').collect();
        let tag_count = temp.len();
        let tags = if tag_count == 2 {
            (temp[0].to_string(), temp[1].to_string())
        } else {
            (temp[0].to_string(), temp[0].to_string())
        };
        let double = tag_count != 2 && double;
        let etoken = crate::pyre::escape(token);
        let (xstart, xend) = if token != "_" {
            (
                format!(r"(?:(?<=_)|(?<![\w{etoken}]))"),
                format!(r"(?:(?=_)|(?![\w{etoken}]))"),
            )
        } else {
            (format!(r"(?<![\w{etoken}])"), format!(r"(?![\w{etoken}])"))
        };
        let max_size = if tag_count != 2 && !double { 1 } else { 2 };
        let boundary = if smart {
            format!(
                "(?x)
                (?P<ambiguous>(?<!^)(?<![\\s{etoken}]){xstart}{etoken}{{1,}}{xend}(?![\\s{etoken}])(?!$))|
                (?P<end>(?<!^)(?<![\\s{etoken}]){etoken}{{1,}}{xend})|
                (?P<start>{xstart}{etoken}{{1,}}(?![\\s{etoken}])(?!$))
                "
            )
        } else {
            format!(
                "(?x)
                (?P<ambiguous>(?<!^)(?<![\\s{etoken}]){etoken}{{1,}}(?![\\s{etoken}])(?!$))|
                (?P<end>(?<!^)(?<![\\s{etoken}]){etoken}{{1,}})|
                (?P<start>{etoken}{{1,}}(?![\\s{etoken}])(?!$))
                "
            )
        };
        Delimiter {
            stack: VecDeque::new(),
            tag_count,
            tags,
            double,
            max_size,
            boundary: Regex::new(&boundary, 0),
            bad: Regex::new(&format!("{etoken}+"), 0),
        }
    }
}

/// A delimiter match: its span and which named group matched.
#[derive(Clone, Copy)]
struct Boundary {
    start: usize,
    end: usize,
    kind: char,
}

/// The boundary of a match, in character indices.
fn boundary_of(m: &Match, index: &pystr::CharIndex) -> Boundary {
    let kind = m.lastgroup().and_then(|g| g.chars().next()).unwrap_or('s');
    Boundary {
        start: index.to_char(m.start(0)),
        end: index.to_char(m.end(0)),
        kind,
    }
}

#[derive(Default)]
struct DelimiterState {
    tokens: Vec<String>,
    delimiters: Vec<(String, Delimiter)>,
    regions: Vec<Region>,
    stack: Vec<Entry>,
    cache_index: usize,
    cache_pos: usize,
}

impl DelimiterState {
    fn delim(&mut self, token: &str) -> &mut Delimiter {
        &mut self
            .delimiters
            .iter_mut()
            .find(|(t, _)| t == token)
            .expect("a known delimiter")
            .1
    }

    fn any_stack(&self) -> bool {
        self.delimiters.iter().any(|(_, d)| !d.stack.is_empty())
    }

    fn any_other_stack(&self, token: &str) -> bool {
        self.delimiters
            .iter()
            .any(|(t, d)| t != token && !d.stack.is_empty())
    }

    fn reset(&mut self) {
        for (_, d) in self.delimiters.iter_mut() {
            d.stack.clear();
        }
        self.regions.clear();
        self.stack.clear();
        self.cache_index = 0;
        self.cache_pos = 0;
    }

    fn add_region(&mut self, token: &str, a: usize, b: usize, c: usize, d: usize, size: usize) {
        let tags = self.delim(token).tags.clone();
        self.regions.push((a, b, c, d, tags, size));
        for (_, delim) in self.delimiters.iter_mut() {
            while let Some(&(p, ..)) = delim.stack.back() {
                if a <= p && p < d {
                    delim.stack.pop_back();
                    continue;
                }
                break;
            }
        }
    }

    /// The first delimiter whose boundary pattern matches at character index `start`.
    fn get_match(&self, data: &str, index: &pystr::CharIndex, start: usize) -> Option<Boundary> {
        for (_, d) in &self.delimiters {
            if let Some(m) = d.boundary.match_at(data, index.to_byte(start)) {
                return Some(boundary_of(&m, index));
            }
        }
        None
    }

    /// The next delimiter run from character index `start` that can open or close emphasis.
    fn search(
        &self,
        data: &str,
        index: &pystr::CharIndex,
        chars: &[char],
        start: usize,
    ) -> Option<(String, Boundary)> {
        for (i, &c) in chars.iter().enumerate().skip(start) {
            let Some((token, d)) = self
                .delimiters
                .iter()
                .find(|(t, _)| t.chars().eq(std::iter::once(c)))
            else {
                continue;
            };
            if let Some(m) = d.boundary.match_at(data, index.to_byte(i)) {
                let b = boundary_of(&m, index);
                if d.stack.is_empty() && b.kind == 'e' {
                    continue;
                }
                return Some((token.clone(), b));
            }
        }
        None
    }

    fn increment_next_position(&mut self, start: usize, count: usize) {
        self.cache_index += count;
        if self.cache_index < self.regions.len() {
            self.cache_pos = self.regions[self.cache_index].0;
            while !self.stack.is_empty() {
                let entry = self.stack.remove(0);
                if start < entry.0 && entry.0 <= self.cache_pos {
                    self.cache_pos = entry.0;
                    break;
                }
            }
        } else {
            self.reset();
        }
    }
}

/// The emphasis processor: `*` and `_` delimiter runs build `strong` and `em` elements.
pub struct DelimiterProcessor {
    state: RefCell<DelimiterState>,
    re: RefCell<Rc<Regex>>,
}

impl DelimiterProcessor {
    pub fn new() -> DelimiterProcessor {
        DelimiterProcessor {
            state: RefCell::new(DelimiterState::default()),
            re: RefCell::new(Rc::new(Regex::new("(?!)", 0))),
        }
    }

    /// Add (or replace) a delimiter token.
    pub fn add(&self, token: &str, tags: &str, smart: bool, double: bool) {
        let mut st = self.state.borrow_mut();
        if !st.tokens.iter().any(|t| t == token) {
            st.tokens.push(token.to_string());
        }
        let delim = Delimiter::new(token, tags, smart, double);
        match st.delimiters.iter_mut().find(|(t, _)| t == token) {
            Some(slot) => slot.1 = delim,
            None => st.delimiters.push((token.to_string(), delim)),
        }
        let pattern = st
            .tokens
            .iter()
            .map(|t| crate::pyre::escape(t))
            .collect::<Vec<_>>()
            .join("|");
        *self.re.borrow_mut() = Rc::new(Regex::new(&pattern, S));
    }

    pub fn reset(&self) {
        self.state.borrow_mut().reset();
    }

    fn build_element(
        &self,
        md: &mut Markdown,
        data: &str,
        start: usize,
        offset: isize,
    ) -> (El, usize) {
        let st = self.state.borrow();
        let regions = &st.regions;
        let index = pystr::CharIndex::new(data);
        let piece = |a: usize, b: usize| -> String {
            index
                .slice(data, a as isize + offset, b as isize + offset)
                .to_string()
        };
        let mut el: Option<El> = None;
        let mut last: Option<El> = None;
        let mut previous: Option<&Region> = None;
        let mut outer: Vec<El> = Vec::new();
        let mut outer_r: Vec<&Region> = Vec::new();
        let end = regions.len();
        let mut idx = 0;
        for (n, i) in (start..end).enumerate() {
            idx = n + 1;
            let r = &regions[i];
            if r.0 >= regions[start].3 {
                idx -= 1;
                break;
            }
            let el1 = md.tree.element(if r.5 == 2 { &r.4 .0 } else { &r.4 .1 });
            if idx > 1 {
                let l = last.expect("an element");
                let prev = previous.expect("a region");
                if md.tree.text(l).is_none() {
                    let text = if prev.2 < r.0 {
                        piece(prev.1, prev.2)
                    } else {
                        piece(prev.1, r.0)
                    };
                    md.tree.set_text_str(l, text);
                }
                if Some(&l) != outer.last() && md.tree.tail(l).is_none() {
                    let o = outer_r.last().expect("an outer region");
                    if r.0 < o.3 {
                        md.tree.set_tail_str(l, piece(prev.3, r.0));
                    } else {
                        md.tree.set_tail_str(l, piece(prev.3, o.2));
                        let outer_last = *outer.last().expect("an outer element");
                        md.tree.set_tail_str(outer_last, piece(o.3, r.0));
                    }
                }
            }
            if el.is_none() {
                el = Some(el1);
                last = Some(el1);
                outer.push(el1);
                outer_r.push(r);
            } else {
                while outer_r.len() > 1 && r.3 > outer_r.last().expect("an outer region").3 {
                    outer.pop();
                    outer_r.pop();
                }
                md.tree
                    .append(*outer.last().expect("an outer element"), el1);
                if i + 1 < end && r.3 > regions[i + 1].3 {
                    outer.push(el1);
                    outer_r.push(r);
                }
                last = Some(el1);
            }
            previous = Some(r);
        }
        while let Some(&o) = outer.last() {
            let l = last.expect("an element");
            let prev = previous.expect("a region");
            if md.tree.text(l).is_none() {
                md.tree.set_text_str(l, piece(prev.1, prev.2));
            }
            if md.tree.tail(l).is_none() && l != o {
                md.tree
                    .set_tail_str(l, piece(prev.3, outer_r.last().expect("an outer region").2));
            }
            last = outer.pop();
            previous = outer_r.pop();
        }
        (el.expect("at least one region"), idx)
    }

    /// The next element of an emphasis run already worked out, at character index `pos` of the current text.
    fn get_cached_result(&self, md: &mut Markdown, pos: usize, data: &str) -> InlineResult {
        let (offset, start, end, cache_index) = {
            let st = self.state.borrow();
            let offset = if pos != st.cache_pos {
                pos as isize - st.cache_pos as isize
            } else {
                pos as isize - st.regions[st.cache_index].0 as isize
            };
            let r = &st.regions[st.cache_index];
            (offset, r.0, r.3, st.cache_index)
        };
        let (el, count) = self.build_element(md, data, cache_index, offset);
        self.state
            .borrow_mut()
            .increment_next_position(start, count);
        let index = pystr::CharIndex::new(data);
        let at = |x: usize| index.to_byte((x as isize + offset).max(0) as usize);
        (Some(InlineNode::El(el)), Some(at(start)), Some(at(end)))
    }
}

impl Default for DelimiterProcessor {
    fn default() -> Self {
        Self::new()
    }
}

impl InlinePattern for DelimiterProcessor {
    fn regex(&self) -> Rc<Regex> {
        self.re.borrow().clone()
    }

    fn handle_match(&self, md: &mut Markdown, m: &Match, data: &str) -> PyResult<InlineResult> {
        // Python's arithmetic on delimiter positions is in characters.
        let index = pystr::CharIndex::new(data);
        let chars: Vec<char> = data.chars().collect();
        let mstart = index.to_char(m.start(0));
        let mend = index.to_char(m.end(0));
        if !self.state.borrow().regions.is_empty() {
            return Ok(self.get_cached_result(md, mstart, data));
        }
        let span = |a: usize, b: usize| (Some(index.to_byte(a)), Some(index.to_byte(b)));
        let mut st = self.state.borrow_mut();
        let first = st.get_match(data, &index, mstart);
        let m2 = match first {
            Some(b) if b.kind != 'e' => b,
            _ => {
                let (a, b) = match first {
                    Some(b) => (b.start, b.end),
                    None => {
                        let token = m.str(0).to_string();
                        let bad = st
                            .delim(&token)
                            .bad
                            .match_at(data, m.start(0))
                            .expect("a delimiter run");
                        (index.to_char(bad.start(0)), index.to_char(bad.end(0)))
                    }
                };
                let (s, e) = span(a, b);
                return Ok((None, s, e));
            }
        };
        let token = chars[mstart].to_string();
        let mut start = m2.start;
        let mut end = m2.end;
        let length = end - start;
        if st.delim(&token).double && length < 2 {
            let (s, e) = span(mstart, mend);
            return Ok((None, s, e));
        }
        let is_ambiguous = m2.kind != 's';
        st.delim(&token)
            .stack
            .push_back((start, start + length, is_ambiguous, length));
        while st.any_stack() {
            let Some((token, m2)) = st.search(data, &index, &chars, end) else {
                break;
            };
            start = m2.start;
            end = m2.end;
            let mut current = m2.end - m2.start;
            let mut is_start = m2.kind != 'e';
            let mut is_end = !is_start || m2.kind != 's';
            let is_ambiguous = is_start && is_end;
            let (tag_count, max_size, double) = {
                let d = st.delim(&token);
                (d.tag_count, d.max_size, d.double)
            };
            let mut last = st.delim(&token).stack.back().map_or(0, |e| e.3);
            if !st.delim(&token).stack.is_empty()
                && is_end
                && ((!is_ambiguous && current > last) || current == last || current >= 3)
            {
                is_start = false;
                let original = current;
                while !st.delim(&token).stack.is_empty() && current > 0 && last <= current {
                    let delimiter = st.delim(&token).stack.pop_back().expect("an entry");
                    let size = delimiter.3.min(if tag_count == 2 && delimiter.3 == 3 {
                        1
                    } else {
                        max_size
                    });
                    st.add_region(
                        &token,
                        delimiter.1 - size,
                        delimiter.1,
                        start,
                        start + size,
                        size,
                    );
                    start += size;
                    current -= size;
                    if size < delimiter.3 && (!double || (delimiter.3 - size) != 1) {
                        let new = delimiter.3 - size;
                        st.delim(&token).stack.push_back((
                            delimiter.0,
                            delimiter.1 - size,
                            delimiter.2,
                            new,
                        ));
                    }
                    if st.delim(&token).stack.is_empty() {
                        if st.any_other_stack(&token) {
                            st.delim(&token).stack.clear();
                            continue;
                        }
                        break;
                    }
                    last = st.delim(&token).stack.back().expect("an entry").3;
                }
                if original >= 3 && current > 0 && is_ambiguous {
                    st.delim(&token).stack.push_back((
                        m2.start + (original - current),
                        end,
                        false,
                        current,
                    ));
                    is_end = false;
                } else {
                    is_end = current > 0 && !st.delim(&token).stack.is_empty() && last > current;
                }
            }
            if !st.delim(&token).stack.is_empty()
                && is_end
                && (last >= 3 || !is_ambiguous)
                && last > current
            {
                let mut delimiter = st.delim(&token).stack.pop_back().expect("an entry");
                while !st.delim(&token).stack.is_empty() && delimiter.3 != 3 && delimiter.2 {
                    delimiter = st.delim(&token).stack.pop_back().expect("an entry");
                    last = delimiter.3;
                }
                if delimiter.2 && delimiter.3 != 3 {
                    if st.any_other_stack(&token) {
                        st.delim(&token).stack.clear();
                        continue;
                    }
                    break;
                }
                is_start = false;
                let ds = delimiter.0;
                let mut de = delimiter.1;
                while current > 0 && (!double || current != 1) {
                    let size = current.min(if tag_count == 2 && current == 3 {
                        1
                    } else {
                        max_size
                    });
                    let new = last - size;
                    st.add_region(&token, ds + new, de, start, start + size, size);
                    start += size;
                    current -= size;
                    last -= size;
                    de -= size;
                }
                if last > 0 && (!double || last > 1) {
                    st.delim(&token).stack.push_back((ds, de, false, last));
                }
            }
            if is_start && (!double || current != 1) {
                st.delim(&token)
                    .stack
                    .push_back((start, end, is_ambiguous, current));
            }
        }
        let pending: Vec<Entry> = st
            .delimiters
            .iter()
            .flat_map(|(_, d)| d.stack.iter().copied())
            .collect();
        st.stack.extend(pending);
        st.stack.sort_by_key(|e| e.0);
        if !st.regions.is_empty() {
            st.regions.sort_by_key(|r| r.0);
            let (rstart, rend) = (st.regions[0].0, st.regions[0].3);
            drop(st);
            let (el, count) = self.build_element(md, data, 0, 0);
            self.state
                .borrow_mut()
                .increment_next_position(rstart, count);
            let (s, e) = span(rstart, rend);
            return Ok((Some(InlineNode::El(el)), s, e));
        }
        let end = st.stack.last().map_or(mend, |e| e.1);
        st.reset();
        let (s, e) = span(mstart, end);
        Ok((None, s, e))
    }
}
