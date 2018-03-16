extern crate atty;
#[macro_use] extern crate clap;
extern crate ansi_term;
extern crate regex;
extern crate itertools;

use std::ffi::{OsStr, OsString};
use std::path::PathBuf;
use std::borrow::Cow;
use regex::{Regex, RegexBuilder};

use std::io::BufRead;
use std::io::Write as IoWrite;
use std::os::unix::ffi::OsStrExt;
use std::fmt::Write as FmtWrite;

// This prefixes are used when "smart branches" feature
// is turned on. When line starts with given prefix, then retain
// lines with same indentation starting with given prefixes in context.
const SMART_BRANCH_PREFIXES: &[(&str, &[&str])] = &[
    ("} else ", &["if ", "} else if "]),
    ("case ", &["switch "]),
];

const LESS_ARGS: &[&str] = &["--quit-if-one-screen", "--RAW-CONTROL-CHARS",
                             "--quit-on-intr", "--no-init"];

#[derive(Debug)]
enum OgrepError {
    GitGrepWithStdinInput,
    GitGrepFailed,
    InvalidOgrepOptions,
}
impl std::error::Error for OgrepError {
    fn description(&self) -> &str {
        match *self {
            OgrepError::GitGrepWithStdinInput => "Don't use '-' input with --use-git-grep option",
            OgrepError::GitGrepFailed => "git grep failed",
            OgrepError::InvalidOgrepOptions => "OGREP_OPTIONS environment variable contains invalid UTF-8",
        }
    }
}
impl std::fmt::Display for OgrepError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> Result<(), std::fmt::Error> {
        use std::error::Error;
        write!(f, "{}", self.description())
    }
}

enum InputSpec {
    File(PathBuf),
    Stdin,
}

enum Input {
    File(std::io::BufReader<std::fs::File>),
    Stdin(std::io::Stdin),
}
enum InputLock<'a> {
    File(&'a mut std::io::BufReader<std::fs::File>),
    Stdin(std::io::StdinLock<'a>),
}
impl Input {
    fn open(spec: &InputSpec) -> std::io::Result<Self> {
        match *spec {
            InputSpec::File(ref path) => {
                let file = std::fs::File::open(path)?;
                Ok(Input::File(std::io::BufReader::new(file)))
            },
            InputSpec::Stdin => Ok(Input::Stdin(std::io::stdin())),
        }
    }
    fn lock(&mut self) -> InputLock {
        match *self {
            Input::File(ref mut file) => InputLock::File(file),
            Input::Stdin(ref mut stdin) => InputLock::Stdin(stdin.lock()),
        }
    }
}
impl<'a> InputLock<'a> {
    fn as_buf_read(&mut self) -> &mut std::io::BufRead {
        match self {
            &mut InputLock::File(ref mut reader) => reader,
            &mut InputLock::Stdin(ref mut lock) => lock,
        }
    }
}

enum Output {
    Pager(std::process::Child),
    Stdout(std::io::Stdout),
}
enum OutputLock<'a> {
    Pager(&'a mut std::process::ChildStdin),
    Stdout(std::io::StdoutLock<'a>),
}
impl Output {
    fn lock(&mut self) -> OutputLock {
        match *self {
            Output::Pager(ref mut process) => OutputLock::Pager(process.stdin.as_mut().unwrap()),
            Output::Stdout(ref mut stdout) => OutputLock::Stdout(stdout.lock()),
        }
    }

    fn close(mut self) -> Result<(), Box<std::error::Error>> {
        self.close_impl()
    }

    fn close_impl(&mut self) -> Result<(), Box<std::error::Error>> {
        match self {
            &mut Output::Pager(ref mut process) => { process.wait()?; Ok(()) },
            &mut Output::Stdout(_) => Ok(()),
        }
    }
}
impl Drop for Output {
    fn drop(&mut self) {
        let _ = self.close_impl();
    }
}
impl<'a> OutputLock<'a> {
    fn as_write(&mut self) -> &mut std::io::Write {
        match self {
            &mut OutputLock::Pager(ref mut stdin) => stdin,
            &mut OutputLock::Stdout(ref mut lock) => lock,
        }
    }
}

arg_enum!{
    #[derive(Debug)]
    pub enum UseColors { Always, Auto, Never }
}

arg_enum!{
    #[derive(Debug)]
    pub enum Preprocessor { Context, Ignore, Preserve }
}

struct Options {
    pattern: String,
    input: InputSpec,
    regex: bool,
    case_insensitive: bool,
    whole_word: bool,
    use_colors: UseColors,
    use_pager: bool,
    use_git_grep: bool,
    breaks: bool,
    ellipsis: bool,
    print_filename: bool,
    smart_branches: bool,
    preprocessor: Preprocessor,
}

struct AppearanceOptions {
    use_colors: bool,
    breaks: bool,
    ellipsis: bool,
    print_filename: bool,
}

struct Printer<'o> {
	output: &'o mut std::io::Write,
    options: AppearanceOptions,
}
impl<'o> Printer<'o> {
    fn print_context(&mut self, line_number: usize, line: &str) {
        if self.options.use_colors {
            let text = format!("{:4}: {}", line_number, line);
            writeln!(self.output, "{}", ansi_term::Style::new().dimmed().paint(text)).unwrap();
        } else {
            writeln!(self.output, "{:4}: {}", line_number, line).unwrap();
        }
    }

    fn print_match<'m, M>(&mut self, line_number: usize, line: &str, matches: M)
            where M: Iterator<Item=regex::Match<'m>> {
        if self.options.use_colors {
            let match_style = ansi_term::Style::new().bold();

            let mut buf = String::new();
            let mut pos = 0usize;
            for m in matches {
                buf.push_str(&line[pos..m.start()]);
                write!(&mut buf, "{}", match_style.paint(m.as_str())).unwrap();
                pos = m.end();
            }
            buf.push_str(&line[pos..]);

            writeln!(self.output, "{:4}: {}", line_number, buf).unwrap();

        } else {
            writeln!(self.output, "{:4}: {}", line_number, line).unwrap();
        }
    }

    fn print_break(&mut self) {
        if self.options.breaks {
            writeln!(self.output).unwrap();
        }
    }

    fn print_ellipsis(&mut self) {
        if self.options.ellipsis {
            writeln!(self.output, "   {}", ansi_term::Style::new().dimmed().paint("…")).unwrap();
        }
    }

    fn print_filename(&self, filename: &std::path::Path) {
        if self.options.print_filename {
            let mut stdout = std::io::stdout();
            stdout.write(b"\n").unwrap();
            if self.options.use_colors {
                let style = ansi_term::Style::new().underline();
                style.paint(filename.as_os_str().as_bytes()).write_to(&mut stdout).unwrap();
            } else {
                stdout.write_all(filename.as_os_str().as_bytes()).unwrap();
            }
            stdout.write(b"\n\n").unwrap();
        }
    }
}

fn parse_arguments<'i, Iter: Iterator<Item=OsString>>(args: Iter) -> Options {
    use clap::{App, Arg};

    let colors_default = UseColors::Auto.to_string();
    let preprocessor_default = Preprocessor::Context.to_string();

    let matches = App::new(crate_name!())
        .about(crate_description!())
        .author(crate_authors!("\n"))
		.version(crate_version!())
        .setting(clap::AppSettings::NoBinaryName)
        .after_help("\
ENVIRONMENT VARIABLES:
    OGREP_OPTIONS  Default options

EXIT STATUS:
    0              Some matches found
    1              No matches found
    2              An error occurred")
        .arg(Arg::with_name("pattern")
            .help("Pattern to search for")
            .required(true))
        .arg(Arg::with_name("input")
            .help("File to search in"))
        .arg(Arg::with_name("regex")
            .short("e")
            .long("regex")
            .help("Treat pattern as regular expression"))
        .arg(Arg::with_name("case-insensitive")
            .short("i")
            .long("case-insensitive")
            .help("Perform case-insensitive matching"))
        .arg(Arg::with_name("whole-word")
            .short("w")
            .long("word")
            .help("Search for whole words matching pattern"))
        .arg(Arg::with_name("color")
            .long("color")
            .takes_value(true)
            .default_value(&colors_default)
            .possible_values(&UseColors::variants())
            .case_insensitive(true)
            .help("File to search in"))
        .arg(Arg::with_name("no-pager")
            .long("no-pager")
            .help("Don't use pager even when output is terminal"))
        .arg(Arg::with_name("use-git-grep")
            .long("use-git-grep")
            .short("g")
            .help("Use git grep for prior search"))
        .arg(Arg::with_name("no-breaks")
            .long("no-breaks")
            .help("Don't preserve line breaks"))
        .arg(Arg::with_name("ellipsis")
            .long("ellipsis")
            .help("Print ellipsis when lines were skipped"))
        .arg(Arg::with_name("print-filename")
            .long("print-filename")
            .help("Print filename on match"))
        .arg(Arg::with_name("no-smart-branches")
            .long("no-smart-branches")
            .help("Don't handle if/if-else/else conditionals specially"))
        .arg(Arg::with_name("preprocessor")
            .long("preprocessor")
            .takes_value(true)
            .default_value(&preprocessor_default)
            .possible_values(&Preprocessor::variants())
            .case_insensitive(true)
            .help("How to handle C preprocessor instructions"))
        .get_matches_from(args);

    Options {
        pattern: matches.value_of("pattern").expect("pattern").to_string(),
        input: match matches.value_of_os("input").unwrap_or(OsStr::new("-")) {
          path if path == "-" => InputSpec::Stdin,
          path => InputSpec::File(PathBuf::from(path)),
        },
        regex: matches.is_present("regex"),
        case_insensitive: matches.is_present("case-insensitive"),
        whole_word: matches.is_present("whole-word"),
        use_colors: value_t!(matches, "color", UseColors).unwrap_or_else(|e| e.exit()),
        use_pager: !matches.is_present("no-pager"),
        use_git_grep: matches.is_present("use-git-grep"),
        breaks: !matches.is_present("no-breaks"),
        ellipsis: matches.is_present("ellipsis"),
        print_filename: matches.is_present("print-filename"),
        smart_branches: !matches.is_present("no-smart-branches"),
        preprocessor: value_t!(matches, "preprocessor", Preprocessor).unwrap_or_else(|e| e.exit()),
    }
}

fn calculate_indentation(s: &str) -> Option<usize> {
    s.find(|c: char| !c.is_whitespace())
}

struct ContextEntry {
    line_number: usize,
    indentation: usize,
    line: String,
}

enum PreprocessorKind { If, Else, Endif, Other }
fn preprocessor_instruction_kind(s: &str) -> Option<PreprocessorKind> {
    match s {
        _ if s.starts_with("#if ") => Some(PreprocessorKind::If),
        _ if s.starts_with("#else") => Some(PreprocessorKind::Else),
        _ if s.starts_with("#endif") => Some(PreprocessorKind::Endif),
        _ if s.starts_with("#") => Some(PreprocessorKind::Other),
        _ => None,
    }
}

fn process_input(input: &mut BufRead,
                 pattern: &Regex,
                 options: &Options,
                 filepath: Option<&std::path::Path>,
                 printer: &mut Printer) -> std::io::Result<bool> {
    // Context of current line. Last context item contains closest line above current
    // whose indentation is lower than one of a current line. One before last
    // item contains closest line above last context line with lower indentation and
    // so on. Once line is printed, it is removed from context.
    // Context may contain lines with identical identation due to smart if-else branches
    // handling.
    let mut context = Vec::new();

    // Secondary stack for preprocessor instructions.
    let mut preprocessor_level = 0usize;
    let mut preprocessor_context = Vec::new();

    // Whether at least one match was already found.
    let mut match_found = false;

    // Whether empty line was met since last match.
    let mut was_empty_line = false;

    let mut last_printed_lineno = 0usize;

    for (line_number, line) in input.lines().enumerate().map(|(n, l)| (n+1, l)) {
        let line = line?;
        let indentation = match calculate_indentation(&line) {
            Some(ind) => ind,
            None => {
                was_empty_line = true;
                continue;
            }
        };

        // Ignore lines looking like C preprocessor instruction, because they
        // are often written without indentation and this breaks context.
        match options.preprocessor {
            Preprocessor::Preserve => (), // Do nothing, handle line as usual
            Preprocessor::Ignore =>
                if preprocessor_instruction_kind(&line[indentation..]).is_some() {
                    continue;
                },
            Preprocessor::Context =>
                match preprocessor_instruction_kind(&line[indentation..]) {
                    None => (),
                    Some(PreprocessorKind::If) => {
                        preprocessor_level += 1;
                        preprocessor_context.push(ContextEntry { line_number, indentation: preprocessor_level, line });
                        continue;
                    },
                    Some(PreprocessorKind::Else) => {
                        preprocessor_context.push(ContextEntry { line_number, indentation: preprocessor_level, line });
                        continue;
                    },
                    Some(PreprocessorKind::Endif) => {
                        let top = preprocessor_context.iter().rposition(|e: &ContextEntry| {
                            e.indentation < preprocessor_level
                        });
                        preprocessor_context.truncate(top.map(|t| t+1).unwrap_or(0));
                        preprocessor_level -= 1;
                        continue;
                    },
                    Some(PreprocessorKind::Other) => continue,
                },
        }

        let top = context.iter().rposition(|e: &ContextEntry| {
            // Upper scopes are always preserved.
            if e.indentation < indentation { return true; }
            if e.indentation > indentation { return false; }

            if !options.smart_branches { return false; }

            let stripped_line = &line[indentation..];
            let stripped_context_line = &e.line[e.indentation..];
            for &(prefix, context_prefixes) in SMART_BRANCH_PREFIXES {
                if stripped_line.starts_with(prefix) {
                    return context_prefixes.iter().any(|p| stripped_context_line.starts_with(p));
                }
            }

            return false;
        });
        context.truncate(top.map(|t| t+1).unwrap_or(0));

        let matched = {
            let mut matches = pattern.find_iter(&line).peekable();
            if matches.peek().is_some() {
                // `match_found` is checked to avoid extra line break before first match.
                if !match_found {
                    if let Some(ref path) = filepath {
                        printer.print_filename(path)
                    }
                }
                if was_empty_line && match_found {
                    printer.print_break();
                }

                {
                    let combined_context = itertools::merge_join_by(&context, &preprocessor_context, |ci, pci|
                        ci.line_number.cmp(&pci.line_number)
                    ).map(|either| {
                        use itertools::EitherOrBoth::{Left, Right, Both};
                        match either {
                            Left(l) => l,
                            Right(l) => l,
                            Both(_, _) => unreachable!(),
                        }
                    });

                    for entry in combined_context {
                        if (!was_empty_line || !printer.options.breaks) &&
                           entry.line_number > last_printed_lineno + 1 {
                            printer.print_ellipsis();
                        }
                        printer.print_context(entry.line_number, &entry.line);
                        last_printed_lineno = entry.line_number;
                    }

                    if (!was_empty_line || !printer.options.breaks) &&
                       line_number > last_printed_lineno + 1 {
                        printer.print_ellipsis();
                    }
                    printer.print_match(line_number, &line, matches);
                    last_printed_lineno = line_number;
                }

                context.clear();
                preprocessor_context.clear();
                was_empty_line = false;
                match_found = true;

                true
            } else {
                false
            }
        };

        if !matched {
            context.push(ContextEntry { line_number, indentation, line });
        }
    }

    Ok(match_found)
}

fn real_main() -> std::result::Result<i32, Box<std::error::Error>> {
    // Read default options from OGREP_OPTIONS environment variable.
    let env_var = std::env::var("OGREP_OPTIONS");
    let env_var_ref = match env_var {
        Ok(ref opts) => opts.as_str(),
        Err(std::env::VarError::NotPresent) => "",
        Err(std::env::VarError::NotUnicode(_)) =>
            return Err(Box::new(OgrepError::InvalidOgrepOptions)),
    };
    let env_args = env_var_ref
        .split_whitespace()
        .map(|b| OsString::from(b));
    let cmdline_args = std::env::args_os();
    let args = env_args.chain(cmdline_args.skip(1));
    let options = parse_arguments(args);

    let appearance = AppearanceOptions {
        use_colors: match options.use_colors {
            UseColors::Always => true,
            UseColors::Never => false,
            UseColors::Auto => atty::is(atty::Stream::Stdout),
        },
        breaks: options.breaks,
        ellipsis: options.ellipsis,
        print_filename: options.print_filename || options.use_git_grep,
    };

    let mut output= if options.use_pager {
        let pager_process = match std::env::var_os("PAGER") {
            Some(pager_cmdline) => {
                // User configured custom pager via environment variable.
                // Since pager can contain parameters, not only command name,
                // it is needed to start it using shell. Find which shell to use.
                let shell_var = std::env::var_os("SHELL");
                let shell_path = shell_var.as_ref().map(|v| v.as_os_str()).unwrap_or(OsStr::new("/bin/sh"));
                std::process::Command::new(shell_path)
                    .args(&[OsStr::new("-c"), &pager_cmdline])
                    .stdin(std::process::Stdio::piped())
                    .spawn()?
            },
            None => std::process::Command::new("less")
                        .args(LESS_ARGS)
                        .stdin(std::process::Stdio::piped())
                        .spawn()?
        };
        Output::Pager(pager_process)
    } else {
        Output::Stdout(std::io::stdout())
    };

    let mut match_found = false;
    {
        let mut output_lock = output.lock();

        let mut printer = Printer { output: output_lock.as_write(), options: appearance };

        let mut pattern: Cow<str> =
            if options.regex {
                Cow::from(options.pattern.as_ref())
            } else {
                Cow::from(regex::escape(&options.pattern))
            };
        if options.whole_word {
            let p = pattern.to_mut();
            p.insert_str(0, r"\b");
            p.push_str(r"\b");
        }
        let re = RegexBuilder::new(&pattern).case_insensitive(options.case_insensitive).build()?;

        if options.use_git_grep {
            let pathspec = match options.input {
                InputSpec::File(ref path) => path,
                InputSpec::Stdin => return Err(Box::new(OgrepError::GitGrepWithStdinInput)),
            };
            let mut git_grep_args = vec![OsStr::new("grep"), OsStr::new("--files-with-matches")];
            if options.case_insensitive {
                git_grep_args.push(OsStr::new("--ignore-case"))
            }
            if !options.regex {
                git_grep_args.push(OsStr::new("--fixed-strings"))
            }
            if options.whole_word {
                git_grep_args.push(OsStr::new("--word-regexp"))
            }
            git_grep_args.push(OsStr::new("-e"));
            git_grep_args.push(OsStr::new(&options.pattern));
            git_grep_args.push(OsStr::new("--"));
            git_grep_args.push(pathspec.as_os_str());
            let mut git_grep_process = std::process::Command::new("git")
                .args(&git_grep_args)
                .stdout(std::process::Stdio::piped())
                .spawn()?;

            {
                let out = git_grep_process.stdout.as_mut().unwrap();
                let mut reader = std::io::BufReader::new(out);
                let mut line = String::new();
                while let Ok(bytes_count) = reader.read_line(&mut line) {
                    if bytes_count == 0 { break }

                    let filepath = std::path::Path::new(line.trim_right_matches('\n'));

                    let mut file = std::fs::File::open(&filepath)?;
                    let mut input = std::io::BufReader::new(file);
                    match_found |= process_input(&mut input, &re, &options, Some(filepath), &mut printer)?;
                }
            };

            match git_grep_process.wait()?.code() {
                Some(0) | Some(1) => (),
                _ => return Err(Box::new(OgrepError::GitGrepFailed)),
            }

        } else {
            let mut input = Input::open(&options.input)?;
            let mut input_lock = input.lock();
            let filename: Option<&std::path::Path> = match options.input {
                InputSpec::File(ref path) => Some(path),
                InputSpec::Stdin => None,
            };
            match_found = process_input(input_lock.as_buf_read(), &re, &options, filename, &mut printer)?
        };
    }

    output.close()?;
    Ok(if match_found { 0 } else { 1 })
}

fn main() {
    match real_main() {
        Ok(code) => std::process::exit(code),
        Err(err) => {
            writeln!(std::io::stderr(), "{}", err.description()).unwrap();
            std::process::exit(2);
        },
    }
}

