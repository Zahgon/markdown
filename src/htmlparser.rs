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

//! Extract raw HTML from the document.
//!
//! Python-Markdown finds block level raw HTML with Python's `html.parser.HTMLParser`, patched to suit
//! Markdown: `</>` and a lone `</` are data, processing instructions end only at `?>`, entity references need
//! their `;` and backticks are not allowed in tag names and attribute names or values. This module reproduces
//! that tokenizer (CPython 3.12) with Python-Markdown's patches and its `HTMLExtractor` subclass; with
//! `extra` set it is the `HTMLExtractorExtra` of the `md_in_html` extension, which builds elements for
//! `markdown="1"` blocks.
//!
//! Positions are counted in characters, as Python counts them, and the parser keeps Python's state across
//! its two passes (`feed` and `close`), so its quirks on unterminated markup are the original's.

use std::collections::HashSet;
use std::sync::OnceLock;

use crate::core::{Markdown, PyError, PyResult};
use crate::etree::{El, Text};
use crate::pyre::Regex;
use crate::util::Stashed;

/// A string indexed by characters, with Python's clamping slice semantics.
struct CharStr {
    s: String,
    starts: Vec<usize>,
}

impl CharStr {
    fn new(s: &str) -> CharStr {
        let mut starts: Vec<usize> = s.char_indices().map(|(i, _)| i).collect();
        starts.push(s.len());
        CharStr {
            s: s.to_string(),
            starts,
        }
    }

    fn len(&self) -> usize {
        self.starts.len() - 1
    }

    fn byte(&self, ci: usize) -> usize {
        self.starts[ci.min(self.len())]
    }

    fn char_index(&self, byte: usize) -> usize {
        self.starts.partition_point(|&b| b < byte)
    }

    /// `s[a:b]`.
    fn slice(&self, a: usize, b: usize) -> &str {
        let a = a.min(self.len());
        let b = b.min(self.len());
        if a >= b {
            ""
        } else {
            &self.s[self.byte(a)..self.byte(b)]
        }
    }

    /// `s[a:]`.
    fn from(&self, a: usize) -> &str {
        self.slice(a, self.len())
    }

    fn starts_with_at(&self, pat: &str, i: usize) -> bool {
        i <= self.len() && self.s[self.byte(i)..].starts_with(pat)
    }

    /// `s.find(pat, start)`.
    fn find(&self, pat: &str, start: usize) -> Option<usize> {
        if start > self.len() {
            return None;
        }
        self.s[self.byte(start)..]
            .find(pat)
            .map(|b| self.char_index(self.byte(start) + b))
    }

    /// `regex.search(s, pos)` as character positions of the whole match.
    fn search(&self, re: &Regex, pos: usize) -> Option<(usize, usize)> {
        if pos > self.len() {
            return None;
        }
        re.search(&self.s, self.byte(pos))
            .map(|m| (self.char_index(m.start(0)), self.char_index(m.end(0))))
    }

    fn char_at(&self, i: usize) -> Option<char> {
        if i < self.len() {
            self.s[self.byte(i)..].chars().next()
        } else {
            None
        }
    }
}

struct Patterns {
    interesting_normal: Regex,
    starttagopen: Regex,
    charref: Regex,
    entityref: Regex,
    piclose: Regex,
    commentclose: Regex,
    tagfind_tolerant: Regex,
    attrfind_tolerant: Regex,
    locatetagend: Regex,
    endendtag: Regex,
    blank_line: Regex,
    inner_blank_line: Regex,
}

fn patterns() -> &'static Patterns {
    static P: OnceLock<Patterns> = OnceLock::new();
    P.get_or_init(|| Patterns {
        interesting_normal: Regex::new("[&<]", 0),
        starttagopen: Regex::new("<[a-zA-Z]|</>", 0),
        charref: Regex::new("&#(?:[0-9]+|[xX][0-9a-fA-F]+)[^0-9a-fA-F]", 0),
        entityref: Regex::new(r"&([a-zA-Z][-.a-zA-Z0-9]*);", 0),
        piclose: Regex::new(r"\?>", 0),
        commentclose: Regex::new(r"--!?>", 0),
        tagfind_tolerant: Regex::new(r"([a-zA-Z][^\t\n\r\f />]*)(?:[\t\n\r\f ]|/(?!>))*", 0),
        attrfind_tolerant: Regex::new(
            r#"
  (
    (?<=['"\t\n\r\f /])[^\t\n\r\f />][^\t\n\r\f /=>]*  # attribute name
   )
  ([\t\n\r\f ]*=[\t\n\r\f ]*        # value indicator
    ('[^']*'                        # LITA-enclosed value
    |"[^"]*"                        # LIT-enclosed value
    |(?!['"])[^>\t\n\r\f ]*         # bare value
    )
   )?
  (?:[\t\n\r\f ]|/(?!>))*           # possibly followed by a space
"#,
            crate::pyre::X,
        ),
        locatetagend: Regex::new(
            r#"
  [a-zA-Z][^`\t\n\r\f />]*           # tag name
  [\t\n\r\f /]*                     # optional whitespace before attribute name
  (?:(?<=['"\t\n\r\f /])[^`\t\n\r\f />][^\t\n\r\f /=>]*  # attribute name
    (?:=                            # value indicator
      (?:'[^']*'                    # LITA-enclosed value
        |"[^"]*"                    # LIT-enclosed value
        |(?!['"])[^>\t\n\r\f ]*     # bare value
       )
     )?
    [\t\n\r\f /]*                   # possibly followed by a space
   )*
   >?
"#,
            crate::pyre::X,
        ),
        endendtag: Regex::new(">", 0),
        blank_line: Regex::new(r"^([ ]*\n){2}", 0),
        inner_blank_line: Regex::new(r"\n[ \t]*\n", 0),
    })
}

/// Elements whose content is raw text until their end tag.
const CDATA_CONTENT_ELEMENTS: [&str; 6] =
    ["script", "style", "xmp", "iframe", "noembed", "noframes"];

/// Elements which never have an end tag and so cannot contain a comment.
const VOID_TAGS: [&str; 14] = [
    "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param", "source",
    "track", "wbr",
];

const SPAN_TAGS: [&str; 15] = [
    "address", "dd", "dt", "h1", "h2", "h3", "h4", "h5", "h6", "legend", "li", "p", "summary",
    "td", "th",
];
const RAW_TAGS: [&str; 7] = [
    "canvas", "math", "option", "pre", "script", "style", "textarea",
];

/// Attributes as the parser reports them, or as `md_in_html` turns them into a dict.
#[derive(Clone)]
enum Attrs {
    List(Vec<(String, Option<String>)>),
    Dict(Vec<(String, String)>),
}

fn attrs_to_dict(attrs: &Attrs) -> PyResult<Vec<(String, String)>> {
    match attrs {
        Attrs::List(list) => {
            let mut out: Vec<(String, String)> = Vec::new();
            for (k, v) in list {
                let v = v.clone().unwrap_or_else(|| k.clone());
                match out.iter_mut().find(|(key, _)| key == k) {
                    Some(slot) => slot.1 = v,
                    None => out.push((k.clone(), v)),
                }
            }
            Ok(out)
        }
        // Iterating a dict yields its keys, which then unpack as two-character strings.
        Attrs::Dict(dict) => {
            let mut out: Vec<(String, String)> = Vec::new();
            for (k, _) in dict {
                let chars: Vec<char> = k.chars().collect();
                if chars.len() != 2 {
                    return Err(if chars.len() > 2 {
                        PyError::new("ValueError", "too many values to unpack (expected 2)")
                    } else {
                        PyError::new(
                            "ValueError",
                            format!(
                                "not enough values to unpack (expected 2, got {})",
                                chars.len()
                            ),
                        )
                    });
                }
                let (key, value) = (chars[0].to_string(), chars[1].to_string());
                match out.iter_mut().find(|(k2, _)| *k2 == key) {
                    Some(slot) => slot.1 = value,
                    None => out.push((key, value)),
                }
            }
            Ok(out)
        }
    }
}

fn dict_pop(dict: &mut Vec<(String, String)>, key: &str) -> Option<String> {
    let i = dict.iter().position(|(k, _)| k == key)?;
    Some(dict.remove(i).1)
}

fn dict_set(dict: &mut Vec<(String, String)>, key: &str, value: &str) {
    match dict.iter_mut().find(|(k, _)| k == key) {
        Some(slot) => slot.1 = value.to_string(),
        None => dict.push((key.to_string(), value.to_string())),
    }
}

/// ElementTree's `TreeBuilder`, building into the converter's tree.
#[derive(Default)]
struct TreeBuilder {
    data: Vec<String>,
    elem: Vec<El>,
    last: Option<El>,
    root: Option<El>,
    tail: bool,
}

impl TreeBuilder {
    fn flush(&mut self, md: &mut Markdown) {
        if self.data.is_empty() {
            return;
        }
        if let Some(last) = self.last {
            let text = self.data.concat();
            if self.tail {
                md.tree.set_tail(last, Some(Text::plain(text)));
            } else {
                md.tree.set_text(last, Some(Text::plain(text)));
            }
        }
        self.data.clear();
    }

    fn start(&mut self, md: &mut Markdown, tag: &str, attrs: Vec<(String, String)>) {
        self.flush(md);
        let el = md.tree.element_with(tag, attrs);
        self.last = Some(el);
        match self.elem.last() {
            Some(&parent) => md.tree.append(parent, el),
            None => {
                if self.root.is_none() {
                    self.root = Some(el);
                }
            }
        }
        self.elem.push(el);
        self.tail = false;
    }

    fn end(&mut self, md: &mut Markdown) {
        self.flush(md);
        self.last = self.elem.pop();
        self.tail = true;
    }

    fn data(&mut self, data: &str) {
        self.data.push(data.to_string());
    }
}

/// Python-Markdown's `HTMLExtractor` (or, with `extra`, `md_in_html`'s `HTMLExtractorExtra`).
pub struct HtmlExtractor {
    extra: bool,
    // html.parser state
    rawdata: CharStr,
    lineno: usize,
    offset: usize,
    interesting_cdata: Option<Regex>,
    cdata_elem: Option<String>,
    starttag_text: Option<String>,
    // HTMLExtractor state
    empty_tags: HashSet<&'static str>,
    lineno_start_cache: Vec<usize>,
    inraw: bool,
    intail: bool,
    stack: Vec<String>,
    inline_stack: Vec<String>,
    cache: Vec<String>,
    /// The document with block level raw HTML replaced by placeholders.
    pub cleandoc: Vec<String>,
    // HTMLExtractorExtra state
    mdstack: Vec<String>,
    treebuilder: TreeBuilder,
    mdstate: Vec<Option<&'static str>>,
    mdstarted: Vec<bool>,
}

impl HtmlExtractor {
    pub fn new(extra: bool) -> HtmlExtractor {
        HtmlExtractor {
            extra,
            rawdata: CharStr::new(""),
            lineno: 1,
            offset: 0,
            interesting_cdata: None,
            cdata_elem: None,
            starttag_text: None,
            empty_tags: ["hr"].into_iter().collect(),
            lineno_start_cache: vec![0],
            inraw: false,
            intail: false,
            stack: Vec::new(),
            inline_stack: Vec::new(),
            cache: Vec::new(),
            cleandoc: Vec::new(),
            mdstack: Vec::new(),
            treebuilder: TreeBuilder::default(),
            mdstate: Vec::new(),
            mdstarted: Vec::new(),
        }
    }

    /// Feed the whole document.
    pub fn feed(&mut self, md: &mut Markdown, data: &str) -> PyResult<()> {
        if data.is_empty() {
            return Ok(());
        }
        let joined = format!("{}{}", self.rawdata.s, data);
        self.rawdata = CharStr::new(&joined);
        self.goahead(md, false)
    }

    /// Process the rest of the document.
    pub fn close(&mut self, md: &mut Markdown) -> PyResult<()> {
        self.goahead(md, true)?;
        if self.rawdata.len() > 0 {
            let rest = self.rawdata.s.clone();
            self.handle_data(md, &rest)?;
        }
        if !self.cache.is_empty() {
            let placeholder = md.html_stash.store_str(self.cache.concat());
            self.cleandoc.push(placeholder);
            self.cache.clear();
        }
        if self.extra && !self.mdstack.is_empty() {
            let tag = self.mdstack[0].clone();
            self.handle_endtag(md, &tag)?;
        }
        Ok(())
    }

    // ---- _markupbase.ParserBase ----

    fn updatepos(&mut self, i: usize, j: usize) -> usize {
        if i >= j {
            return j;
        }
        let segment = self.rawdata.slice(i, j);
        let nlines = segment.matches('\n').count();
        if nlines > 0 {
            self.lineno += nlines;
            let last_nl = segment.rfind('\n').unwrap_or(0);
            let pos = i + crate::pystr::char_len(&segment[..last_nl]);
            self.offset = j - (pos + 1);
        } else {
            self.offset += j - i;
        }
        j
    }

    // ---- html.parser.HTMLParser ----

    fn set_cdata_mode(&mut self, elem: &str) {
        let elem = crate::pystr::lower(elem);
        // re.IGNORECASE | re.ASCII: only ASCII letters match in either case.
        let name: String = elem
            .chars()
            .map(|c| {
                if c.is_ascii_alphabetic() {
                    format!("[{}{}]", c.to_ascii_lowercase(), c.to_ascii_uppercase())
                } else {
                    crate::pyre::escape(&c.to_string())
                }
            })
            .collect();
        self.interesting_cdata = Some(Regex::new(&format!(r"</{name}(?=[\t\n\r\f />])"), 0));
        self.cdata_elem = Some(elem);
    }

    fn clear_cdata_mode(&mut self) {
        self.interesting_cdata = None;
        self.cdata_elem = None;
    }

    fn goahead(&mut self, md: &mut Markdown, end: bool) -> PyResult<()> {
        let p = patterns();
        let mut i = 0;
        let n = self.rawdata.len();
        while i < n {
            let found = match &self.interesting_cdata {
                Some(re) => self.rawdata.search(re, i),
                None => self.rawdata.search(&p.interesting_normal, i),
            };
            let j = match found {
                Some((start, _)) => start,
                None => {
                    if self.cdata_elem.is_some() {
                        break;
                    }
                    n
                }
            };
            if i < j {
                let data = self.rawdata.slice(i, j).to_string();
                self.handle_data(md, &data)?;
            }
            i = self.updatepos(i, j);
            if i == n {
                break;
            }
            if self.rawdata.starts_with_at("<", i) {
                let mut k: isize = if self.match_at(&p.starttagopen, i) {
                    self.parse_starttag(md, i)?
                } else if self.rawdata.starts_with_at("</", i) {
                    self.parse_endtag(md, i)?
                } else if self.rawdata.starts_with_at("<!--", i) {
                    self.parse_comment(md, i)?
                } else if self.rawdata.starts_with_at("<?", i) {
                    self.parse_pi(md, i)?
                } else if self.rawdata.starts_with_at("<!", i) {
                    self.parse_html_declaration(md, i)?
                } else if i + 1 < n || end {
                    self.handle_data(md, "<")?;
                    (i + 1) as isize
                } else {
                    break;
                };
                if k < 0 {
                    if !end {
                        break;
                    }
                    if self.match_at(&p.starttagopen, i) {
                        // An unterminated start tag is dropped.
                    } else if self.rawdata.starts_with_at("</", i) {
                        if i + 2 == n {
                            self.handle_data(md, "</")?;
                        }
                        // Otherwise the patched `endtagopen` matches any `</`, and the text is dropped.
                    } else if self.rawdata.starts_with_at("<!--", i) {
                        let mut j = n;
                        for suffix in ["--!", "--", "-"] {
                            if self.rawdata.from(i + 4).ends_with(suffix) {
                                j -= suffix.len();
                                break;
                            }
                        }
                        let data = self.rawdata.slice(i + 4, j).to_string();
                        self.handle_comment(md, &data)?;
                    } else if self.rawdata.starts_with_at("<![CDATA[", i) {
                        let data = self.rawdata.from(i + 3).to_string();
                        self.unknown_decl(md, &data)?;
                    } else if crate::pystr::lower(self.rawdata.slice(i, i + 9)) == "<!doctype" {
                        let data = self.rawdata.from(i + 2).to_string();
                        self.handle_decl(md, &data)?;
                    } else if self.rawdata.starts_with_at("<!", i) {
                        let data = self.rawdata.from(i + 2).to_string();
                        self.handle_comment(md, &data)?;
                    } else if self.rawdata.starts_with_at("<?", i) {
                        let data = self.rawdata.from(i + 2).to_string();
                        self.handle_pi(md, &data)?;
                    }
                    k = n as isize;
                }
                i = self.updatepos(i, k as usize);
            } else if self.rawdata.starts_with_at("&#", i) {
                if let Some((_, mend)) = self.match_span(&p.charref, i) {
                    let name = self.rawdata.slice(i + 2, mend - 1).to_string();
                    self.handle_charref(md, &name)?;
                    let mut k = mend;
                    if !self.rawdata.starts_with_at(";", k - 1) {
                        k -= 1;
                    }
                    i = self.updatepos(i, k);
                    continue;
                } else {
                    if self.rawdata.from(i).contains(';') {
                        let data = self.rawdata.slice(i, i + 2).to_string();
                        self.handle_data(md, &data)?;
                        i = self.updatepos(i, i + 2);
                    }
                    break;
                }
            } else if self.rawdata.starts_with_at("&", i) {
                if let Some(caps_end) = self.match_span(&p.entityref, i).map(|(_, e)| e) {
                    let name = self.rawdata.slice(i + 1, caps_end - 1).to_string();
                    self.handle_entityref(md, &name)?;
                    let mut k = caps_end;
                    if !self.rawdata.starts_with_at(";", k - 1) {
                        k -= 1;
                    }
                    i = self.updatepos(i, k);
                    continue;
                }
                // The patched `incomplete` pattern is `entityref` again, so it cannot match here.
                if i + 1 < n {
                    self.handle_data(md, "&")?;
                    i = self.updatepos(i, i + 1);
                } else {
                    break;
                }
            } else {
                unreachable!("interesting.search() lied");
            }
        }
        if end && i < n {
            let data = self.rawdata.slice(i, n).to_string();
            self.handle_data(md, &data)?;
            i = self.updatepos(i, n);
        }
        let rest = self.rawdata.from(i).to_string();
        self.rawdata = CharStr::new(&rest);
        Ok(())
    }

    fn match_at(&self, re: &Regex, i: usize) -> bool {
        self.match_span(re, i).is_some()
    }

    /// `re.match(rawdata, i)` as the character span of the match.
    fn match_span(&self, re: &Regex, i: usize) -> Option<(usize, usize)> {
        if i > self.rawdata.len() {
            return None;
        }
        re.match_at(&self.rawdata.s, self.rawdata.byte(i)).map(|m| {
            (
                self.rawdata.char_index(m.start(0)),
                self.rawdata.char_index(m.end(0)),
            )
        })
    }

    fn parse_html_declaration_base(&mut self, md: &mut Markdown, i: usize) -> PyResult<isize> {
        if self.rawdata.starts_with_at("<!--", i) {
            return self.parse_comment(md, i);
        }
        if self.rawdata.starts_with_at("<![CDATA[", i) {
            return Ok(match self.rawdata.find("]]>", i + 9) {
                None => -1,
                Some(j) => {
                    let data = self.rawdata.slice(i + 3, j).to_string();
                    self.unknown_decl(md, &data)?;
                    (j + 3) as isize
                }
            });
        }
        if crate::pystr::lower(self.rawdata.slice(i, i + 9)) == "<!doctype" {
            return Ok(match self.rawdata.find(">", i + 9) {
                None => -1,
                Some(gtpos) => {
                    let data = self.rawdata.slice(i + 2, gtpos).to_string();
                    self.handle_decl(md, &data)?;
                    (gtpos + 1) as isize
                }
            });
        }
        if self.rawdata.starts_with_at("<![", i) {
            return Ok(match self.rawdata.find(">", i + 3) {
                None => -1,
                Some(j) => {
                    if self.rawdata.char_at(j - 1) == Some(']') {
                        let data = self.rawdata.slice(i + 3, j - 1).to_string();
                        self.unknown_decl(md, &data)?;
                    } else {
                        let data = self.rawdata.slice(i + 2, j).to_string();
                        self.handle_comment(md, &data)?;
                    }
                    (j + 1) as isize
                }
            });
        }
        self.parse_bogus_comment(md, i)
    }

    fn parse_pi_base(&mut self, md: &mut Markdown, i: usize) -> PyResult<isize> {
        Ok(match self.rawdata.search(&patterns().piclose, i + 2) {
            None => -1,
            Some((start, end)) => {
                let data = self.rawdata.slice(i + 2, start).to_string();
                self.handle_pi(md, &data)?;
                end as isize
            }
        })
    }

    fn check_for_whole_start_tag(&self, i: usize) -> isize {
        let (_, j) = self
            .match_span(&patterns().locatetagend, i + 1)
            .expect("locatetagend always matches here");
        if self.rawdata.char_at(j - 1) != Some('>') {
            return -1;
        }
        j as isize
    }

    fn parse_endtag_base(&mut self, md: &mut Markdown, i: usize) -> PyResult<isize> {
        if self.rawdata.find(">", i + 2).is_none() {
            return Ok(-1);
        }
        let p = patterns();
        let (_, j) = self
            .match_span(&p.locatetagend, i + 2)
            .expect("locatetagend matches after a letter");
        if self.rawdata.char_at(j - 1) != Some('>') {
            return Ok(-1);
        }
        let m = p
            .tagfind_tolerant
            .match_at(&self.rawdata.s, self.rawdata.byte(i + 2))
            .expect("a tag name");
        let tag = crate::pystr::lower(m.str(1));
        self.handle_endtag(md, &tag)?;
        self.clear_cdata_mode();
        Ok(j as isize)
    }

    // ---- _HTMLParser patches and HTMLExtractor overrides ----

    fn parse_endtag(&mut self, md: &mut Markdown, i: usize) -> PyResult<isize> {
        let start = self.rawdata.slice(i, i + 3).to_string();
        let c = start.chars().last().map_or(0, |c| c as u32);
        let is_data = crate::pystr::char_len(&start) < 3
            || !((65..=90).contains(&c) || (97..=122).contains(&c));
        let pos = if is_data {
            -1
        } else {
            self.parse_endtag_base(md, i)?
        };
        if pos == -1 {
            let data = self.rawdata.slice(i, i + 2).to_string();
            self.handle_data(md, &data)?;
            return Ok((i + 2) as isize);
        }
        Ok(pos)
    }

    fn parse_starttag(&mut self, md: &mut Markdown, i: usize) -> PyResult<isize> {
        if self.rawdata.slice(i, i + 3) == "</>" {
            let data = self.rawdata.slice(i, i + 3).to_string();
            self.handle_data(md, &data)?;
            return Ok((i + 3) as isize);
        }
        self.starttag_text = None;
        let endpos = self.check_for_whole_start_tag(i);
        if endpos < 0 {
            let data = self.rawdata.slice(i, i + 1).to_string();
            self.handle_data(md, &data)?;
            return Ok((i + 1) as isize);
        }
        let endpos = endpos as usize;
        self.starttag_text = Some(self.rawdata.slice(i, endpos).to_string());
        let p = patterns();
        let mut attrs: Vec<(String, Option<String>)> = Vec::new();
        let (tag, mut k) = {
            let m = p
                .tagfind_tolerant
                .match_at(&self.rawdata.s, self.rawdata.byte(i + 1))
                .expect("a tag name");
            (
                crate::pystr::lower(m.str(1)),
                self.rawdata.char_index(m.end(0)),
            )
        };
        while k < endpos {
            let Some(m) = p
                .attrfind_tolerant
                .match_at(&self.rawdata.s, self.rawdata.byte(k))
            else {
                break;
            };
            let attrname = m.str(1).to_string();
            let rest = m.group(2).unwrap_or("");
            let mut attrvalue = m.group(3).map(str::to_string);
            if rest.is_empty() {
                attrvalue = None;
            } else if let Some(v) = &attrvalue {
                let first = v.chars().next();
                let last = v.chars().last();
                if (first == Some('\'') && last == Some('\''))
                    || (first == Some('"') && last == Some('"'))
                {
                    let chars: Vec<char> = v.chars().collect();
                    attrvalue = Some(if chars.len() >= 2 {
                        chars[1..chars.len() - 1].iter().collect()
                    } else {
                        String::new()
                    });
                }
            }
            if let Some(v) = &attrvalue {
                if !v.is_empty() {
                    attrvalue = Some(crate::entities::unescape(v));
                }
            }
            attrs.push((crate::pystr::lower(&attrname), attrvalue));
            k = self.rawdata.char_index(m.end(0));
        }
        let end = crate::pystr::strip(self.rawdata.slice(k, endpos)).to_string();
        if end != ">" && end != "/>" {
            let data = self.rawdata.slice(i, endpos).to_string();
            self.handle_data(md, &data)?;
            return Ok(endpos as isize);
        }
        if end.ends_with("/>") {
            self.handle_startendtag(md, &tag, &Attrs::List(attrs))?;
        } else {
            if CDATA_CONTENT_ELEMENTS.contains(&tag.as_str()) {
                self.set_cdata_mode(&tag);
            }
            self.handle_starttag(md, &tag, Attrs::List(attrs))?;
        }
        Ok(endpos as isize)
    }

    fn parse_comment(&mut self, md: &mut Markdown, i: usize) -> PyResult<isize> {
        match self.rawdata.search(&patterns().commentclose, i + 4) {
            None => {
                self.handle_data(md, "<")?;
                Ok((i + 1) as isize)
            }
            Some((start, end)) => {
                let data = self.rawdata.slice(i + 4, start).to_string();
                self.handle_comment(md, &data)?;
                Ok(end as isize)
            }
        }
    }

    fn parse_pi(&mut self, md: &mut Markdown, i: usize) -> PyResult<isize> {
        if self.at_line_start() || self.intail || (self.extra && !self.mdstack.is_empty()) {
            return self.parse_pi_base(md, i);
        }
        self.handle_data(md, "<?")?;
        Ok((i + 2) as isize)
    }

    fn parse_html_declaration(&mut self, md: &mut Markdown, i: usize) -> PyResult<isize> {
        if self.at_line_start() || self.intail || (self.extra && !self.mdstack.is_empty()) {
            if self.rawdata.slice(i, i + 3) == "<![" && self.rawdata.slice(i, i + 9) != "<![CDATA["
            {
                let result = self.parse_bogus_comment(md, i)?;
                if result == -1 {
                    let data = self.rawdata.slice(i, i + 1).to_string();
                    self.handle_data(md, &data)?;
                    return Ok((i + 1) as isize);
                }
                return Ok(result);
            }
            return self.parse_html_declaration_base(md, i);
        }
        self.handle_data(md, "<!")?;
        Ok((i + 2) as isize)
    }

    fn parse_bogus_comment(&mut self, md: &mut Markdown, i: usize) -> PyResult<isize> {
        let Some(pos) = self.rawdata.find(">", i + 2) else {
            return Ok(-1);
        };
        let pos = pos + 1;
        let data = self.rawdata.slice(i, pos).to_string();
        self.handle_empty_tag(md, &data, false)?;
        Ok(pos as isize)
    }

    fn line_offset(&mut self) -> usize {
        let from = self.lineno_start_cache.len() - 1;
        for ii in from..self.lineno.saturating_sub(1) {
            let last_line_start_pos = self.lineno_start_cache[ii];
            let lf_pos = self
                .rawdata
                .find("\n", last_line_start_pos)
                .unwrap_or(self.rawdata.len());
            self.lineno_start_cache.push(lf_pos + 1);
        }
        self.lineno_start_cache[self.lineno - 1]
    }

    fn at_line_start(&mut self) -> bool {
        if self.offset == 0 {
            return true;
        }
        if self.offset > 3 {
            return false;
        }
        let lo = self.line_offset();
        crate::pystr::strip(self.rawdata.slice(lo, lo + self.offset)).is_empty()
    }

    fn get_endtag_text(&mut self, tag: &str) -> String {
        let start = self.line_offset() + self.offset;
        match self.rawdata.search(&patterns().endendtag, start) {
            Some((_, end)) => self.rawdata.slice(start, end).to_string(),
            None => format!("</{tag}>"),
        }
    }

    /// Whether a blank line follows the `len` characters at the current position.
    fn blank_line_follows(&mut self, len: usize) -> bool {
        let pos = self.line_offset() + self.offset + len;
        patterns().blank_line.is_match(self.rawdata.from(pos))
    }

    fn get_starttag_text(&self) -> String {
        self.starttag_text.clone().unwrap_or_default()
    }

    fn handle_starttag(&mut self, md: &mut Markdown, tag: &str, attrs: Attrs) -> PyResult<()> {
        if self.extra {
            self.extra_handle_starttag(md, tag, attrs)
        } else {
            self.base_handle_starttag(md, tag, attrs)
        }
    }

    fn base_handle_starttag(&mut self, md: &mut Markdown, tag: &str, attrs: Attrs) -> PyResult<()> {
        if self.empty_tags.contains(tag) {
            return self.handle_startendtag(md, tag, &attrs);
        }
        if md.is_block_level(tag) && (self.intail || (self.at_line_start() && !self.inraw)) {
            self.inraw = true;
            self.cleandoc.push("\n".into());
        }
        let text = self.get_starttag_text();
        if self.inraw {
            self.stack.push(tag.to_string());
            self.cache.push(text);
        } else {
            self.cleandoc.push(text);
            self.open_inline_tag(md, tag);
            if CDATA_CONTENT_ELEMENTS.contains(&tag) {
                self.clear_cdata_mode();
            }
        }
        Ok(())
    }

    fn handle_endtag(&mut self, md: &mut Markdown, tag: &str) -> PyResult<()> {
        if self.extra {
            self.extra_handle_endtag(md, tag)
        } else {
            self.base_handle_endtag(md, tag)
        }
    }

    fn base_handle_endtag(&mut self, md: &mut Markdown, tag: &str) -> PyResult<()> {
        let text = self.get_endtag_text(tag);
        if self.inraw {
            self.cache.push(text.clone());
            if self.stack.iter().any(|t| t == tag) {
                while let Some(t) = self.stack.pop() {
                    if t == tag {
                        break;
                    }
                }
            }
            if self.stack.is_empty() {
                if self.blank_line_follows(crate::pystr::char_len(&text)) {
                    self.cache.push("\n".into());
                } else {
                    self.intail = true;
                }
                self.inraw = false;
                let placeholder = md.html_stash.store_str(self.cache.concat());
                self.cleandoc.push(placeholder);
                self.cleandoc.push("\n\n".into());
                self.cache.clear();
            }
        } else {
            self.cleandoc.push(text);
            self.close_inline_tag(tag);
        }
        Ok(())
    }

    fn open_inline_tag(&mut self, md: &Markdown, tag: &str) {
        if !md.is_block_level(tag) && !VOID_TAGS.contains(&tag) {
            self.inline_stack.push(tag.to_string());
        }
    }

    fn close_inline_tag(&mut self, tag: &str) {
        if self.inline_stack.iter().any(|t| t == tag) {
            while let Some(t) = self.inline_stack.pop() {
                if t == tag {
                    break;
                }
            }
        }
    }

    fn inline_close_follows(&mut self, text: &str) -> bool {
        if self.inline_stack.is_empty() {
            return false;
        }
        let pos = self.line_offset() + self.offset + crate::pystr::char_len(text);
        let after = self.rawdata.from(pos).to_string();
        let rest = patterns().inner_blank_line.split(&after, 1)[0]
            .unwrap_or("")
            .to_string();
        self.inline_stack.iter().any(|tag| {
            Regex::new(
                &format!(r"</\s*{}\s*>", crate::pyre::escape(tag)),
                crate::pyre::I,
            )
            .is_match(&rest)
        })
    }

    fn handle_data(&mut self, md: &mut Markdown, data: &str) -> PyResult<()> {
        if self.intail && data.contains('\n') {
            self.intail = false;
        }
        if self.extra && !self.inraw && !self.mdstack.is_empty() {
            if let Some(last) = self.mdstarted.last_mut() {
                *last = false;
            }
            self.treebuilder.data(data);
            return Ok(());
        }
        let _ = md;
        if self.inraw {
            self.cache.push(data.to_string());
        } else {
            if patterns().inner_blank_line.is_match(data) {
                self.inline_stack.clear();
            }
            self.cleandoc.push(data.to_string());
        }
        Ok(())
    }

    fn handle_empty_tag(&mut self, md: &mut Markdown, data: &str, is_block: bool) -> PyResult<()> {
        if self.extra && !self.inraw && !self.mdstack.is_empty() {
            if self.at_line_start() && is_block {
                let placeholder = md.html_stash.store_str(data);
                return self.handle_data(md, &format!("\n{placeholder}\n\n"));
            } else if self.mdstate.last() == Some(&Some("off")) {
                let placeholder = md.html_stash.store_str(data);
                return self.handle_data(md, &placeholder);
            } else {
                return self.handle_data(md, data);
            }
        }
        if self.inraw || self.intail {
            self.cache.push(data.to_string());
        } else if self.at_line_start() && is_block {
            let mut data = data.to_string();
            if self.blank_line_follows(crate::pystr::char_len(&data)) {
                data.push('\n');
            } else {
                self.intail = true;
            }
            let item = self.cleandoc.last().cloned().unwrap_or_default();
            if !item.ends_with("\n\n") && item.ends_with('\n') {
                self.cleandoc.push("\n".into());
            }
            let placeholder = md.html_stash.store_str(data);
            self.cleandoc.push(placeholder);
            self.cleandoc.push("\n\n".into());
        } else {
            self.cleandoc.push(data.to_string());
        }
        Ok(())
    }

    fn handle_startendtag(&mut self, md: &mut Markdown, tag: &str, attrs: &Attrs) -> PyResult<()> {
        if self.extra {
            let data = if self.empty_tags.contains(tag) {
                let mut dict = attrs_to_dict(attrs)?;
                if dict.iter().any(|(k, _)| k == "markdown") {
                    dict_pop(&mut dict, "markdown");
                    let el = md.tree.element_with(tag, dict);
                    crate::serializers::et_tostring_html(&md.tree, el)
                } else {
                    self.get_starttag_text()
                }
            } else {
                self.get_starttag_text()
            };
            let is_block = md.is_block_level(tag);
            return self.handle_empty_tag(md, &data, is_block);
        }
        let text = self.get_starttag_text();
        let is_block = md.is_block_level(tag);
        self.handle_empty_tag(md, &text, is_block)
    }

    fn handle_charref(&mut self, md: &mut Markdown, name: &str) -> PyResult<()> {
        self.handle_empty_tag(md, &format!("&#{name};"), false)
    }

    fn handle_entityref(&mut self, md: &mut Markdown, name: &str) -> PyResult<()> {
        self.handle_empty_tag(md, &format!("&{name};"), false)
    }

    fn handle_comment(&mut self, md: &mut Markdown, data: &str) -> PyResult<()> {
        let text = format!("<!--{data}-->");
        let is_block =
            patterns().inner_blank_line.is_match(data) || !self.inline_close_follows(&text);
        self.handle_empty_tag(md, &text, is_block)
    }

    fn handle_decl(&mut self, md: &mut Markdown, data: &str) -> PyResult<()> {
        self.handle_empty_tag(md, &format!("<!{data}>"), true)
    }

    fn handle_pi(&mut self, md: &mut Markdown, data: &str) -> PyResult<()> {
        self.handle_empty_tag(md, &format!("<?{data}?>"), true)
    }

    fn unknown_decl(&mut self, md: &mut Markdown, data: &str) -> PyResult<()> {
        let end = if data.starts_with("CDATA[") {
            "]]>"
        } else {
            "]>"
        };
        self.handle_empty_tag(md, &format!("<![{data}{end}"), true)
    }

    // ---- HTMLExtractorExtra (md_in_html) ----

    fn block_tags_contains(&self, md: &Markdown, tag: &str) -> bool {
        md.block_level_elements.iter().any(|t| t == tag)
            && !SPAN_TAGS.contains(&tag)
            && !RAW_TAGS.contains(&tag)
            && !self.empty_tags.contains(tag)
    }

    fn get_state(
        &self,
        md: &Markdown,
        tag: &str,
        attrs: &[(String, String)],
    ) -> Option<&'static str> {
        let mut md_attr = attrs
            .iter()
            .find(|(k, _)| k == "markdown")
            .map_or("0".to_string(), |(_, v)| v.clone());
        if md_attr == "markdown" {
            md_attr = "1".into();
        }
        let parent_state = self.mdstate.last().copied().flatten();
        if parent_state == Some("off") || (parent_state == Some("span") && md_attr != "0") {
            md_attr = parent_state.unwrap_or_default().to_string();
        }
        let in_block_tags = self.block_tags_contains(md, tag);
        let in_span_tags = SPAN_TAGS.contains(&tag);
        if (md_attr == "1" && in_block_tags)
            || (md_attr == "block" && (in_block_tags || in_span_tags))
        {
            Some("block")
        } else if (md_attr == "1" && in_span_tags)
            || (md_attr == "span" && (in_block_tags || in_span_tags))
        {
            Some("span")
        } else if md.block_level_elements.iter().any(|t| t == tag) {
            Some("off")
        } else {
            None
        }
    }

    fn extra_handle_starttag(
        &mut self,
        md: &mut Markdown,
        tag: &str,
        attrs: Attrs,
    ) -> PyResult<()> {
        let is_block_tag = md.block_level_elements.iter().any(|t| t == tag);
        if self.empty_tags.contains(tag) && (self.at_line_start() || self.intail) {
            let mut dict = attrs_to_dict(&attrs)?;
            let data = if dict.iter().any(|(k, _)| k == "markdown") {
                dict_pop(&mut dict, "markdown");
                let el = md.tree.element_with(tag, dict);
                crate::serializers::et_tostring_html(&md.tree, el)
            } else {
                self.get_starttag_text()
            };
            return self.handle_empty_tag(md, &data, true);
        }
        let started = self.mdstarted.last().copied().unwrap_or(false);
        if is_block_tag && (self.at_line_start() || self.intail || started) {
            let mut dict = attrs_to_dict(&attrs)?;
            let state = self.get_state(md, tag, &dict);
            if self.inraw || (matches!(state, None | Some("off")) && self.mdstack.is_empty()) {
                dict_pop(&mut dict, "markdown");
                self.base_handle_starttag(md, tag, Attrs::Dict(dict))?;
            } else {
                if self.mdstack.iter().any(|t| t == "p") && is_block_tag {
                    self.handle_endtag(md, "p")?;
                }
                self.mdstate.push(state);
                self.mdstack.push(tag.to_string());
                self.mdstarted.push(true);
                dict_set(&mut dict, "markdown", state.unwrap_or("None"));
                self.treebuilder.start(md, tag, dict);
            }
        } else if self.inraw {
            self.base_handle_starttag(md, tag, attrs)?;
        } else {
            let text = self.get_starttag_text();
            if self.mdstate.last() == Some(&Some("off")) {
                let placeholder = md.html_stash.store_str(text);
                self.handle_data(md, &placeholder)?;
            } else {
                self.handle_data(md, &text)?;
            }
            self.open_inline_tag(md, tag);
            if CDATA_CONTENT_ELEMENTS.contains(&tag) {
                self.clear_cdata_mode();
            }
        }
        Ok(())
    }

    fn extra_handle_endtag(&mut self, md: &mut Markdown, tag: &str) -> PyResult<()> {
        let is_block_tag = md.block_level_elements.iter().any(|t| t == tag);
        if is_block_tag {
            if self.inraw {
                return self.base_handle_endtag(md, tag);
            }
            if self.mdstack.iter().any(|t| t == tag) {
                while let Some(item) = self.mdstack.pop() {
                    self.mdstate.pop();
                    self.mdstarted.pop();
                    self.treebuilder.end(md);
                    if item == tag {
                        break;
                    }
                }
                if self.mdstack.is_empty() {
                    let element = self.get_element();
                    let item = self.cleandoc.last().cloned().unwrap_or_default();
                    if !item.ends_with("\n\n") && item.ends_with('\n') {
                        self.cleandoc.push("\n".into());
                    }
                    self.stash_children(md, element);
                    let placeholder = md.html_stash.store(Stashed::El(element));
                    self.cleandoc.push(placeholder);
                    self.cleandoc.push("\n\n".into());
                    let endtag_len = crate::pystr::char_len(&self.get_endtag_text(tag));
                    if !self.blank_line_follows(endtag_len) {
                        self.intail = true;
                    }
                }
            } else {
                let text = self.get_endtag_text(tag);
                if self.mdstate.last() == Some(&Some("off")) {
                    let placeholder = md.html_stash.store_str(text);
                    self.handle_data(md, &placeholder)?;
                } else {
                    self.handle_data(md, &text)?;
                }
            }
        } else if self.inraw {
            self.base_handle_endtag(md, tag)?;
        } else {
            let text = self.get_endtag_text(tag);
            if self.mdstate.last() == Some(&Some("off")) {
                let placeholder = md.html_stash.store_str(text);
                self.handle_data(md, &placeholder)?;
            } else {
                self.handle_data(md, &text)?;
            }
            self.close_inline_tag(tag);
        }
        Ok(())
    }

    fn get_element(&mut self) -> El {
        let root = self.treebuilder.root.expect("missing toplevel element");
        self.treebuilder = TreeBuilder::default();
        root
    }

    /// Replace the children of a finished `markdown` element (and of its nested markdown children) with
    /// stash placeholders in their parent's text.
    fn stash_children(&mut self, md: &mut Markdown, element: El) {
        let mut queue: std::collections::VecDeque<El> = std::collections::VecDeque::new();
        let mut current = Some(element);
        while let Some(cur) = current {
            for child in md.tree.children(cur).to_vec() {
                md.tree.remove(cur, child);
                let text = md.tree.text_str(cur).unwrap_or("").to_string();
                let mut tail = md.tree.tail_str(child).unwrap_or("").to_string();
                md.tree.set_tail(child, None);
                let state = md.tree.get(child, "markdown").unwrap_or("off").to_string();
                if tail != "\n" {
                    tail = format!("\n{}", crate::pystr::rstrip_chars(&tail, "\n"));
                }
                let text = if text.ends_with("\n\n") {
                    text
                } else {
                    format!("{}\n\n", crate::pystr::rstrip_chars(&text, "\n"))
                };
                if state == "span" || state == "block" {
                    let placeholder = md.html_stash.store(Stashed::El(child));
                    md.tree
                        .set_text_str(cur, format!("{text}{placeholder}{tail}"));
                    queue.push_back(child);
                } else {
                    md.tree.remove_attr(child, "markdown");
                    for c in md.tree.iter(child) {
                        md.tree.remove_attr(c, "markdown");
                    }
                    let placeholder = md.html_stash.store(Stashed::El(child));
                    md.tree
                        .set_text_str(cur, format!("{text}{placeholder}{tail}"));
                }
            }
            current = queue.pop_front();
        }
    }
}
