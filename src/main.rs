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

//! The `markdown` command line, the same as `python -m markdown`: options are parsed the way Python's
//! `optparse` parses them, and an error the original reports with a Python traceback ends with that
//! traceback's last line and exit status 1.

use std::io::{Read, Write};
use std::process::ExitCode;

use markdown::core::{Markdown, PyError, CRITICAL, DEBUG, WARNING};
use markdown::encoding::{self, Codec};
use markdown::value::{self, Value};

/// The program name `optparse` derives from `python -m markdown`.
const PROG: &str = "__main__.py";

const USAGE: &str = "Usage: __main__.py [options] [INPUTFILE]\n       (STDIN is assumed if no INPUTFILE is given)\n";

const HELP: &str = "Usage: __main__.py [options] [INPUTFILE]
       (STDIN is assumed if no INPUTFILE is given)

A Python implementation of John Gruber's Markdown. https://python-
markdown.github.io/

Options:
  --version             show program's version number and exit
  -h, --help            show this help message and exit
  -f OUTPUT_FILE, --file=OUTPUT_FILE
                        Write output to OUTPUT_FILE. Defaults to STDOUT.
  -e ENCODING, --encoding=ENCODING
                        Encoding for input and output files.
  -o OUTPUT_FORMAT, --output_format=OUTPUT_FORMAT
                        Use output format 'xhtml' (default) or 'html'.
  -n, --no_lazy_ol      Observe number of first item of ordered lists.
  -x EXTENSION, --extension=EXTENSION
                        Load extension EXTENSION.
  -c CONFIG_FILE, --extension_configs=CONFIG_FILE
                        Read extension configurations from CONFIG_FILE.
                        CONFIG_FILE must be of JSON or YAML format. YAML
                        format requires that a python YAML library be
                        installed. The parsed JSON or YAML must result in a
                        python dictionary which would be accepted by the
                        'extension_configs' keyword on the markdown.Markdown
                        class. The extensions must also be loaded with the
                        `--extension` option.
  -q, --quiet           Suppress all warnings.
  -v, --verbose         Print all warnings.
  --noisy               Print debug messages.

WARNING: The Python-Markdown library does NOT sanitize its HTML output. If you
are processing Markdown input from an untrusted source, it is your
responsibility to ensure that it is properly sanitized. For more information
see <https://python-markdown.github.io/sanitization/>.
";

/// The options of the command line, in definition order: short and long names, and whether they take a value.
const OPTIONS: &[(Option<&str>, &str, bool)] = &[
    (None, "--version", false),
    (Some("-h"), "--help", false),
    (Some("-f"), "--file", true),
    (Some("-e"), "--encoding", true),
    (Some("-o"), "--output_format", true),
    (Some("-n"), "--no_lazy_ol", false),
    (Some("-x"), "--extension", true),
    (Some("-c"), "--extension_configs", true),
    (Some("-q"), "--quiet", false),
    (Some("-v"), "--verbose", false),
    (None, "--noisy", false),
];

/// The parsed command line.
struct Options {
    output_file: Option<String>,
    encoding: Option<String>,
    output_format: String,
    extensions: Vec<String>,
    config_file: Option<String>,
    log_level: u32,
    args: Vec<String>,
}

/// How the command ends before converting anything.
enum Exit {
    /// `--help` or `--version`: text for standard output.
    Print(String),
    /// A usage error: the message after `error: `.
    Usage(String),
}

impl Options {
    fn apply(&mut self, long: &str, value: Option<String>) -> Result<(), Exit> {
        match long {
            "--version" => return Err(Exit::Print(format!("{PROG} {}\n", markdown::VERSION))),
            "--help" => return Err(Exit::Print(HELP.to_string())),
            "--file" => self.output_file = value,
            "--encoding" => self.encoding = value,
            "--output_format" => self.output_format = value.unwrap_or_default(),
            // `lazy_ol` is accepted but no longer read by the converter.
            "--no_lazy_ol" => {}
            "--extension" => self.extensions.extend(value),
            "--extension_configs" => self.config_file = value,
            "--quiet" => self.log_level = CRITICAL + 10,
            "--verbose" => self.log_level = WARNING,
            "--noisy" => self.log_level = DEBUG,
            _ => unreachable!("unknown option {long}"),
        }
        Ok(())
    }
}

/// Resolve a long option, allowing unambiguous prefixes.
fn match_long(opt: &str) -> Result<&'static str, Exit> {
    if let Some(&(_, long, _)) = OPTIONS.iter().find(|(_, l, _)| *l == opt) {
        return Ok(long);
    }
    let mut possibilities: Vec<&str> = OPTIONS
        .iter()
        .map(|(_, l, _)| *l)
        .filter(|l| l.starts_with(opt))
        .collect();
    match possibilities.len() {
        1 => Ok(possibilities[0]),
        0 => Err(Exit::Usage(format!("no such option: {opt}"))),
        _ => {
            possibilities.sort();
            Err(Exit::Usage(format!(
                "ambiguous option: {opt} ({}?)",
                possibilities.join(", ")
            )))
        }
    }
}

fn takes_value(long: &str) -> bool {
    OPTIONS.iter().any(|(_, l, v)| *l == long && *v)
}

/// Parse the arguments as `optparse` does, with interspersed positional arguments.
fn parse_args(argv: Vec<String>) -> Result<Options, Exit> {
    let mut options = Options {
        output_file: None,
        encoding: None,
        output_format: "xhtml".into(),
        extensions: Vec::new(),
        config_file: None,
        log_level: CRITICAL,
        args: Vec::new(),
    };
    let mut rargs: std::collections::VecDeque<String> = argv.into();
    while let Some(arg) = rargs.pop_front() {
        if arg == "--" {
            options.args.extend(rargs.drain(..));
            break;
        } else if arg.starts_with("--") {
            let (opt, explicit) = match arg.split_once('=') {
                Some((o, v)) => (o.to_string(), Some(v.to_string())),
                None => (arg.clone(), None),
            };
            let had_explicit = explicit.is_some();
            if let Some(v) = explicit {
                rargs.push_front(v);
            }
            let long = match_long(&opt)?;
            let value = if takes_value(long) {
                match rargs.pop_front() {
                    Some(v) => Some(v),
                    None => return Err(Exit::Usage(format!("{long} option requires 1 argument"))),
                }
            } else if had_explicit {
                return Err(Exit::Usage(format!("{long} option does not take a value")));
            } else {
                None
            };
            options.apply(long, value)?;
        } else if arg.starts_with('-') && arg.chars().count() > 1 {
            let chars: Vec<char> = arg.chars().collect();
            let mut i = 1;
            while i < chars.len() {
                let opt = format!("-{}", chars[i]);
                i += 1;
                let Some(&(_, long, takes)) =
                    OPTIONS.iter().find(|(s, _, _)| *s == Some(opt.as_str()))
                else {
                    return Err(Exit::Usage(format!("no such option: {opt}")));
                };
                let mut stop = false;
                let value = if takes {
                    if i < chars.len() {
                        rargs.push_front(chars[i..].iter().collect());
                        stop = true;
                    }
                    match rargs.pop_front() {
                        Some(v) => Some(v),
                        None => {
                            return Err(Exit::Usage(format!("{opt} option requires 1 argument")))
                        }
                    }
                } else {
                    None
                };
                options.apply(long, value)?;
                if stop {
                    break;
                }
            }
        } else {
            options.args.push(arg);
        }
    }
    Ok(options)
}

/// An `OSError` for a file, as Python words it.
fn os_error(err: &std::io::Error, path: &str) -> PyError {
    let quoted = markdown::pystr::repr_str(path);
    match err.kind() {
        std::io::ErrorKind::NotFound => PyError::new(
            "FileNotFoundError",
            format!("[Errno 2] No such file or directory: {quoted}"),
        ),
        std::io::ErrorKind::IsADirectory => PyError::new(
            "IsADirectoryError",
            format!("[Errno 21] Is a directory: {quoted}"),
        ),
        std::io::ErrorKind::PermissionDenied => PyError::new(
            "PermissionError",
            format!("[Errno 13] Permission denied: {quoted}"),
        ),
        std::io::ErrorKind::NotADirectory => PyError::new(
            "NotADirectoryError",
            format!("[Errno 20] Not a directory: {quoted}"),
        ),
        _ => PyError::new("OSError", format!("{err}: {quoted}")),
    }
}

/// Python's universal newlines: `\r\n` and `\r` read as `\n`.
fn universal_newlines(text: String) -> String {
    if text.contains('\r') {
        text.replace("\r\n", "\n").replace('\r', "\n")
    } else {
        text
    }
}

/// `open(path, encoding=...).read()`.
fn read_text(path: &str, encoding: &str) -> Result<String, PyError> {
    let data = std::fs::read(path).map_err(|e| os_error(&e, path))?;
    let codec = encoding::lookup(encoding)?;
    Ok(universal_newlines(encoding::decode(&data, codec)?))
}

/// Everything after option parsing: load the configuration, build the converter and convert.
fn run(options: &Options) -> Result<(), PyError> {
    let mut configs = Value::Dict(Vec::new());
    if let Some(path) = &options.config_file {
        let text = read_text(path, options.encoding.as_deref().unwrap_or("utf-8"))?;
        configs = value::parse_json(&text).map_err(|_| {
            PyError::new(
                "json.decoder.JSONDecodeError",
                format!("Failed parsing extension config file: {path}"),
            )
        })?;
    }
    let mut md = Markdown::new(
        &options.extensions,
        &configs,
        &options.output_format,
        options.log_level,
    )?;
    let encoding = options
        .encoding
        .clone()
        .filter(|e| !e.is_empty())
        .unwrap_or_else(|| "utf-8".into());
    let text = match options.args.first().filter(|a| !a.is_empty()) {
        Some(path) => read_text(path, &encoding)?,
        None => {
            let mut data = Vec::new();
            std::io::stdin()
                .read_to_end(&mut data)
                .map_err(|e| PyError::new("OSError", e.to_string()))?;
            universal_newlines(encoding::decode_stdin(&data))
        }
    };
    let text = text.trim_start_matches('\u{feff}');
    let html = md.convert(text)?;
    match options.output_file.as_deref().filter(|f| !f.is_empty()) {
        Some(path) => {
            let mut file = std::fs::File::create(path).map_err(|e| os_error(&e, path))?;
            let codec: Codec = encoding::lookup(&encoding)?;
            file.write_all(&encoding::encode(&html, codec))
                .map_err(|e| os_error(&e, path))?;
        }
        None => {
            let codec = encoding::lookup(&encoding)?;
            let mut stdout = std::io::stdout().lock();
            stdout
                .write_all(&encoding::encode(&html, codec))
                .map_err(|e| PyError::new("OSError", e.to_string()))?;
            stdout
                .flush()
                .map_err(|e| PyError::new("OSError", e.to_string()))?;
        }
    }
    Ok(())
}

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let options = match parse_args(argv) {
        Ok(options) => options,
        Err(Exit::Print(text)) => {
            print!("{text}");
            return ExitCode::SUCCESS;
        }
        Err(Exit::Usage(message)) => {
            eprint!("{USAGE}\n{PROG}: error: {message}\n");
            return ExitCode::from(2);
        }
    };
    match run(&options) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!(
                "Traceback (most recent call last):\n  File \"{PROG}\", line 1, in <module>\n{err}"
            );
            ExitCode::from(1)
        }
    }
}
