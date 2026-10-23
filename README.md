[Python-Markdown][]
===================

[![Build Status][build-button]][build]
[![BSD License][bsdlicense-button]][bsdlicense]
[![Code of Conduct][codeofconduct-button]][Code of Conduct]

[build-button]: https://github.com/Python-Markdown/markdown/actions/workflows/ci.yml/badge.svg
[build]: https://github.com/Python-Markdown/markdown/actions/workflows/ci.yml
[bsdlicense-button]: https://img.shields.io/badge/license-BSD-yellow.svg
[bsdlicense]: https://opensource.org/licenses/BSD-3-Clause
[codeofconduct-button]: https://img.shields.io/badge/code%20of%20conduct-contributor%20covenant-green.svg?style=flat-square
[Code of Conduct]: https://github.com/Python-Markdown/markdown/blob/master/CODE_OF_CONDUCT.md

This is a Rust implementation of John Gruber's [Markdown][] as Python-Markdown 3.11.2 implements it: the same
processing pipeline (preprocessors, block processors, inline patterns, tree processors, postprocessors and the
serializer), the same bundled extensions and the same `markdown` command line, with output identical to the
Python version's. See [Features][] for information on what exactly is supported and what is not. Additional
features are supported by the [Available Extensions][].

[Python-Markdown]: https://Python-Markdown.github.io/
[Markdown]: https://daringfireball.net/projects/markdown/
[Features]: https://Python-Markdown.github.io#Features
[Available Extensions]: https://Python-Markdown.github.io/extensions

Building
--------

```bash
cargo build --release
```

The command line is `target/release/markdown`; `cargo install --path .` installs it.

Command line
------------

```bash
markdown [options] [INPUTFILE]
```

Standard input is read when no input file is given and the HTML is written to standard output (`-f OUTPUT_FILE`
writes it to a file). The options are the Python version's: `-x EXTENSION` loads an extension, `-c CONFIG_FILE`
reads extension configurations from a JSON file, `-o html|xhtml` chooses the output format, `-e ENCODING` the
encoding of input and output files, `-q`, `-v` and `--noisy` the logging level; `--help` lists them all. Errors
are reported as the Python version reports them, and the program names itself `__main__.py`, as `python -m
markdown` does.

Extensions are named by their entry point name (`toc`), their module path (`markdown.extensions.toc`) or as
`module:Class` (`markdown.extensions.toc:TocExtension`). The bundled extensions are `extra` (`abbr`, `attr_list`,
`def_list`, `fenced_code`, `footnotes`, `md_in_html` and `tables`), `admonition`, `legacy_attrs`, `legacy_em`,
`meta`, `nl2br`, `sane_lists`, `smarty`, `toc` and `wikilinks`. Syntax highlighting (`codehilite`, which needs
Pygments) is not included.

Library
-------

```rust
use markdown::value::Value;

let html = markdown::markdown("*boo!*", &["toc".to_string()], &Value::Dict(Vec::new()), "html")?;
```

`markdown::Markdown` is the converter; extension configurations are JSON values (`markdown::value::Value`).

Python semantics
----------------

The output depends on details of Python 3.12 that Rust does not share, which the crate reproduces: the regular
expression dialect (`src/pyre.rs` translates Python patterns for the `fancy-regex` crate), Unicode string
methods such as `str.isspace()`, `str.lower()` and `str.expandtabs()` (`src/pystr.rs`), the HTML tokenizer of
`html.parser` that Python-Markdown patches (`src/htmlparser.rs`), JSON parsing and value formatting
(`src/value.rs`) and codec lookup and error handling (`src/encoding.rs`). The data these need, Unicode character
properties, the HTML entity tables and the codec aliases, is taken from Python 3.12 into
`src/unicode_tables.rs`, `src/entities_table.rs` and `src/encoding_tables.rs`.

Testing
-------

```bash
cargo test --release
```

runs the unit tests, the file based tests under `tests/basic`, `tests/misc` and `tests/extensions`, the syntax
cases of `tests/fixtures/syntax` and the command line tests.

Documentation
-------------

The `docs/` directory describes the Markdown syntax, the extensions and their options, and the command line.
See the change log at <https://python-markdown.github.io/changelog/>.

Support
-------

You may report bugs, ask for help, and discuss various other issues on the [bug tracker][].

[bug tracker]: https://github.com/Python-Markdown/markdown/issues

Code of Conduct
---------------

Everyone interacting in the Python-Markdown project's code bases, issue trackers,
and mailing lists is expected to follow the [Code of Conduct].
