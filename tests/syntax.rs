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

//! The syntax tests: each case of `tests/fixtures/syntax/*.json` (source, options and expected HTML, ported from
//! the original test suite) must convert to exactly the expected HTML.

use std::path::Path;

use markdown::value::{parse_json, Value};

fn text(case: &Value, key: &str) -> String {
    case.get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

#[test]
fn syntax_cases() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/syntax");
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .expect("fixture directory")
        .flatten()
        .map(|e| e.path())
        .collect();
    files.sort();
    let mut failures = Vec::new();
    let mut count = 0;
    for file in files {
        let data = std::fs::read_to_string(&file).expect("fixture file");
        let Ok(Value::List(cases)) = parse_json(&data) else {
            panic!("{} is not a JSON list", file.display());
        };
        for case in &cases {
            let name = text(case, "name");
            let extensions: Vec<String> = match case.get("extensions") {
                Some(Value::List(items)) => items
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect(),
                _ => Vec::new(),
            };
            let configs = case
                .get("configs")
                .cloned()
                .unwrap_or(Value::Dict(Vec::new()));
            let format = text(case, "output_format");
            let expected = text(case, "expected");
            count += 1;
            match markdown::markdown(&text(case, "source"), &extensions, &configs, &format) {
                Ok(output) if output == expected => {}
                Ok(output) => failures.push(format!(
                    "{name}\n--- expected\n{expected}\n--- got\n{output}"
                )),
                Err(err) => failures.push(format!("{name}: {err}")),
            }
        }
    }
    assert!(count > 0, "no syntax cases");
    assert!(
        failures.is_empty(),
        "{} of {count} cases differ:\n{}",
        failures.len(),
        failures.join("\n\n")
    );
}
