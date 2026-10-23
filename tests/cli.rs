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

//! The command line: options, standard streams, files, encodings and errors.

use std::io::Write;
use std::process::{Command, Output, Stdio};

fn run(args: &[&str], stdin: &[u8], dir: &std::path::Path) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_markdown"))
        .args(args)
        .current_dir(dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the markdown binary runs");
    // The program may exit (on an error) before it reads its input.
    if let Err(err) = child.stdin.take().expect("stdin").write_all(stdin) {
        assert_eq!(
            err.kind(),
            std::io::ErrorKind::BrokenPipe,
            "writing stdin: {err}"
        );
    }
    child.wait_with_output().expect("output")
}

fn scratch(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("markdown-cli-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch directory");
    dir
}

#[test]
fn converts_stdin_without_trailing_newline() {
    let out = run(&[], b"*boo!*\n", &scratch("stdin"));
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(out.stdout, b"<p><em>boo!</em></p>");
    assert!(out.stderr.is_empty());
}

#[test]
fn version_and_usage_errors() {
    let dir = scratch("usage");
    let out = run(&["--version"], b"", &dir);
    assert_eq!(out.stdout, b"__main__.py 3.11.2\n");
    let out = run(&["--ext", "toc"], b"", &dir);
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(
        stderr.ends_with(
            "__main__.py: error: ambiguous option: --ext (--extension, --extension_configs?)\n"
        ),
        "{stderr}"
    );
}

#[test]
fn extensions_and_configs() {
    let dir = scratch("configs");
    std::fs::write(dir.join("cfg.json"), r#"{"toc": {"permalink": true}}"#).unwrap();
    let out = run(&["-x", "toc", "-c", "cfg.json"], b"# Title\n", &dir);
    assert_eq!(
        String::from_utf8(out.stdout).unwrap(),
        "<h1 id=\"title\">Title<a class=\"headerlink\" href=\"#title\" title=\"Permanent link\">&para;</a></h1>"
    );
    std::fs::write(dir.join("bad.json"), "{").unwrap();
    let out = run(&["-c", "bad.json"], b"x", &dir);
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stderr
        .trim_end()
        .ends_with("json.decoder.JSONDecodeError: Failed parsing extension config file: bad.json"));
}

#[test]
fn unknown_extension_is_a_traceback() {
    let out = run(&["-x", "nosuch"], b"x", &scratch("ext"));
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(
        stderr
            .trim_end()
            .ends_with("ModuleNotFoundError: No module named 'nosuch'"),
        "{stderr}"
    );
}

#[test]
fn files_and_encodings() {
    let dir = scratch("files");
    std::fs::write(dir.join("in.md"), b"caf\xe9\r\n").unwrap();
    let out = run(&["-e", "latin-1", "-f", "out.html", "in.md"], b"", &dir);
    assert_eq!(out.status.code(), Some(0));
    assert!(out.stdout.is_empty());
    assert_eq!(
        std::fs::read(dir.join("out.html")).unwrap(),
        b"<p>caf\xe9</p>"
    );
    let out = run(&["-e", "ascii"], "\u{20ac}".as_bytes(), &dir);
    assert_eq!(out.stdout, b"<p>&#8364;</p>");
    let out = run(&[], b"a\xffb", &dir);
    assert_eq!(out.stdout, b"<p>a&#56575;b</p>");
}
