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

//! The file based tests: every `.txt` file under `tests/` converted with the options its directory (and the
//! file's own entry) gives, compared with the `.html` file next to it. The `php` and `pl` directories need
//! HTML normalization (tidy) and are not run, as without tidylib in the original suite.

use std::path::Path;

use markdown::value::{parse_json, Value};

/// Convert every input of `dir` and report the files whose output differs.
fn check_dir(
    dir: &str,
    default_extensions: &[&str],
    per_file: &[(&str, &[&str], &str)],
    exclude: &[&str],
) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join(dir);
    let mut entries: Vec<_> = std::fs::read_dir(&root)
        .expect("test directory")
        .flatten()
        .map(|e| e.path())
        .collect();
    entries.sort();
    let mut failures = Vec::new();
    let mut count = 0;
    for input in entries
        .iter()
        .filter(|p| p.extension().is_some_and(|e| e == "txt"))
    {
        let stem = input.file_stem().unwrap().to_string_lossy().to_string();
        if exclude.contains(&stem.as_str()) {
            continue;
        }
        let (extensions, configs) = match per_file.iter().find(|(name, _, _)| *name == stem) {
            Some((_, exts, configs)) => (exts.to_vec(), *configs),
            None => (default_extensions.to_vec(), "{}"),
        };
        let extensions: Vec<String> = extensions.iter().map(|s| s.to_string()).collect();
        let configs: Value = parse_json(configs).expect("valid JSON configs");
        let text = std::fs::read_to_string(input).expect("input file");
        let expected = std::fs::read_to_string(input.with_extension("html"))
            .expect("expected file")
            .replace("\r\n", "\n");
        let output = markdown::markdown(&text, &extensions, &configs, "xhtml").expect("conversion");
        count += 1;
        if output != expected {
            failures.push(format!(
                "{dir}/{stem}.txt\n--- expected\n{expected}\n--- got\n{output}"
            ));
        }
    }
    assert!(count > 0, "no test files in {dir}");
    assert!(
        failures.is_empty(),
        "{} of {count} files differ:\n{}",
        failures.len(),
        failures.join("\n\n")
    );
}

#[test]
fn basic() {
    check_dir("basic", &[], &[], &[]);
}

#[test]
fn misc() {
    check_dir("misc", &[], &[], &[]);
}

#[test]
fn extensions() {
    check_dir(
        "extensions",
        &[],
        &[
            ("attr_list", &["attr_list", "def_list", "smarty"], "{}"),
            ("toc", &["toc"], "{}"),
            ("toc_invalid", &["toc"], "{}"),
            ("toc_out_of_order", &["toc"], "{}"),
            ("toc_nested", &["toc"], r#"{"toc": {"permalink": true}}"#),
            (
                "toc_nested2",
                &["toc"],
                r#"{"toc": {"permalink": "[link]"}}"#,
            ),
            ("toc_nested_list", &["toc"], "{}"),
            ("wikilinks", &["wikilinks"], "{}"),
            ("github_flavored", &["fenced_code"], "{}"),
            ("sane_lists", &["sane_lists"], "{}"),
            ("nl2br_w_attr_list", &["nl2br", "attr_list"], "{}"),
            ("admonition", &["admonition"], "{}"),
        ],
        &["codehilite"],
    );
}

#[test]
fn extensions_extra() {
    check_dir(
        "extensions/extra",
        &["extra"],
        &[
            ("loose_def_list", &["def_list"], "{}"),
            ("simple_def-lists", &["def_list"], "{}"),
            ("abbr", &["abbr"], "{}"),
            ("footnotes", &["footnotes"], "{}"),
            (
                "extra_config",
                &["extra"],
                r#"{"extra": {"footnotes": {"PLACE_MARKER": "~~~placemarker~~~"}}}"#,
            ),
        ],
        &[],
    );
}
