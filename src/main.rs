#[macro_use] extern crate clap;
extern crate regex;
extern crate itertools;
extern crate termion;

use std::ffi::{OsStr, OsString};
use std::path::PathBuf;
use std::borrow::Cow;
use regex::{Regex, RegexBuilder};

use std::io::BufRead;
use std::io::Write as IoWrite;
use std::os::unix::ffi::OsStrExt;
use std::fmt::Write as FmtWrite;
use itertools::Itertools;

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
    ClapError(clap::Error),
    GitGrepWithStdinInput,
    GitGrepFailed,
    InvalidOgrepOptions,
}
impl std::error::Error for OgrepError {
    fn description(&self) -> &str {
        match *self {
            OgrepError::ClapError(ref e) => e.description(),
            OgrepError::GitGrepWithStdinInput => "Don't use '-' input with --use-git-grep option",
            OgrepError::GitGrepFailed => "git grep failed",
            OgrepError::InvalidOgrepOptions => "OGREP_OPTIONS environment variable contains invalid UTF-8",
        }
    }
}
impl std::fmt::Display for OgrepError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> Result<(), std::fmt::Error> {
        use std::error::Error;
        match *self {
            OgrepError::ClapError(ref e) => write!(f, "{}", e),
            _ => write!(f, "{}", self.description())
        }
    }
}
impl From<clap::Error> for OgrepError {
    fn from(e: clap::Error) -> OgrepError {
        OgrepError::ClapError(e)
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

arg_enum!{
    #[derive(Debug)]
    pub enum ColorSchemeSpec { Grey, Colored }
}

struct Options {
    pattern: String,
    input: InputSpec,
    regex: bool,
    case_insensitive: bool,
    whole_word: bool,
    use_colors: UseColors,
    color_scheme: ColorSchemeSpec,
    use_pager: bool,
    use_git_grep: bool,
    breaks: bool,
    ellipsis: bool,
    print_filename: bool,
    smart_branches: bool,
    preprocessor: Preprocessor,
    context_lines_before: usize,
    context_lines_after: usize,
}

struct ColorScheme {
    filename: (String, String),
    matched_part: (String, String),
    context_line: (String, String),
}

struct AppearanceOptions {
    use_colors: bool,
    color_scheme: ColorScheme,
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
            writeln!(self.output, "{color}{:4}: {}{nocolor}", line_number, line,
                     color=self.options.color_scheme.context_line.0,
                     nocolor=self.options.color_scheme.context_line.1).unwrap();
        } else {
            writeln!(self.output, "{:4}: {}", line_number, line).unwrap();
        }
    }

    fn print_match<'m, M>(&mut self, line_number: usize, line: &str, matches: M)
            where M: Iterator<Item=regex::Match<'m>> {
        if self.options.use_colors {
            let mut buf = String::new();
            let mut pos = 0usize;
            for m in matches {
                buf.push_str(&line[pos..m.start()]);
                write!(&mut buf, "{color}{}{nocolor}", m.as_str(),
                       color=self.options.color_scheme.matched_part.0,
                       nocolor=self.options.color_scheme.matched_part.1).unwrap();
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
            writeln!(self.output, "   {dim}{}{nodim}", "…",
                     dim=termion::style::Faint,
                     nodim=termion::style::NoFaint).unwrap();
        }
    }

    fn print_filename(&mut self, filename: &std::path::Path) {
        if self.options.print_filename {
            self.output.write(b"\n").unwrap();
            if self.options.use_colors {
                write!(&mut self.output, "{}", self.options.color_scheme.filename.0).unwrap();
                self.output.write(filename.as_os_str().as_bytes()).unwrap();
                write!(&mut self.output, "{}", self.options.color_scheme.filename.1).unwrap();
            } else {
                self.output.write_all(filename.as_os_str().as_bytes()).unwrap();
            }
            self.output.write(b"\n\n").unwrap();
        }
    }
}

fn parse_arguments<'i, Iter: Iterator<Item=OsString>>(args: Iter)
        -> Result<Options, clap::Error> {
    use clap::{App, Arg};

    let colors_default = UseColors::Auto.to_string();
    let color_scheme_default = ColorSchemeSpec::Grey.to_string();
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
        .arg(Arg::with_name("before_context")
            .short("B")
            .long("before-context")
            .takes_value(true)
            .help("Show specified number of leading lines before matched one"))
        .arg(Arg::with_name("after_context")
            .short("A")
            .long("after-context")
            .takes_value(true)
            .help("Show specified number of trailing lines after matched one"))
        .arg(Arg::with_name("both_contexts")
            .short("C")
            .long("context")
            .takes_value(true)
            .conflicts_with_all(&["before_context", "after_context"])
            .help("Show specified number of leading and trailing lines before/after matched one"))
        .arg(Arg::with_name("color")
            .long("color")
            .takes_value(true)
            .default_value(&colors_default)
            .possible_values(&UseColors::variants())
            .case_insensitive(true)
            .help("Whether to use colors"))
        .arg(Arg::with_name("color-scheme")
            .long("color-scheme")
            .takes_value(true)
            .default_value(&color_scheme_default)
            .possible_values(&ColorSchemeSpec::variants())
            .case_insensitive(true)
            .help("Color scheme to use"))
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

    let (before_context, after_context) =
        if matches.is_present("both_contexts") {
            let c: usize = value_t!(matches.value_of("both_contexts"), usize)?;
            (c, c)
        } else {
            let before =
                if matches.is_present("before_context") {
                    value_t!(matches.value_of("before_context"), usize)?
                } else {
                    0
                };
            let after =
                if matches.is_present("after_context") {
                    value_t!(matches.value_of("after_context"), usize)?
                } else {
                    0
                };
            (before, after)
        };

    Ok(Options {
        pattern: matches.value_of("pattern").expect("pattern").to_string(),
        input: match matches.value_of_os("input").unwrap_or(OsStr::new("-")) {
          path if path == "-" => InputSpec::Stdin,
          path => InputSpec::File(PathBuf::from(path)),
        },
        regex: matches.is_present("regex"),
        case_insensitive: matches.is_present("case-insensitive"),
        whole_word: matches.is_present("whole-word"),
        use_colors: value_t!(matches, "color", UseColors)?,
        color_scheme: value_t!(matches, "color-scheme", ColorSchemeSpec)?,
        use_pager: !matches.is_present("no-pager"),
        use_git_grep: matches.is_present("use-git-grep"),
        breaks: !matches.is_present("no-breaks"),
        ellipsis: matches.is_present("ellipsis"),
        print_filename: matches.is_present("print-filename"),
        smart_branches: !matches.is_present("no-smart-branches"),
        preprocessor: value_t!(matches, "preprocessor", Preprocessor)?,
        context_lines_before: before_context,
        context_lines_after: after_context,
    })
}

fn calculate_indentation(s: &str) -> Option<usize> {
    s.find(|c: char| !c.is_whitespace())
}

struct Line {
    number: usize,
    text: String,
}

struct ContextEntry {
    line: Line,
    indentation: usize,
}

struct PreprocessorContextEntry {
    line: Line,
    level: usize,
}

struct SurroundContextEntry {
    line: Line,
    print_on_discard: bool,
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

    // Context as it is understood by usual 'grep' - fixed number of
    // lines before and after matched one.
    let mut surrounding_context =
        std::collections::VecDeque::<SurroundContextEntry>::with_capacity(
            options.context_lines_before + options.context_lines_after);

    // Whether at least one match was already found.
    let mut match_found = false;

    // Whether empty line was met since last match.
    let mut was_empty_line = false;

    let mut last_printed_lineno = 0usize;

    // How many trailing lines after match left to print.
    let mut trailing_lines_left = 0usize;

    for (line_number, line) in input.lines().enumerate().map(|(n, l)| (n+1, l)) {
        let line = line?;

        let indentation = calculate_indentation(&line);
        if let Some(indentation) = indentation {
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
                            preprocessor_context.push(PreprocessorContextEntry {
                                line: Line { number: line_number, text: line},
                                level: preprocessor_level });
                            continue;
                        },
                        Some(PreprocessorKind::Else) => {
                            preprocessor_context.push(PreprocessorContextEntry {
                                line: Line { number: line_number, text: line },
                                level: preprocessor_level });
                            continue;
                        },
                        Some(PreprocessorKind::Endif) => {
                            let top = preprocessor_context.iter().rposition(|e: &PreprocessorContextEntry| {
                                e.level < preprocessor_level
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
                let stripped_context_line = &e.line.text[e.indentation..];
                for &(prefix, context_prefixes) in SMART_BRANCH_PREFIXES {
                    if stripped_line.starts_with(prefix) {
                        return context_prefixes.iter().any(|p| stripped_context_line.starts_with(p));
                    }
                }

                return false;
            });
            context.truncate(top.map(|t| t+1).unwrap_or(0));
        } else {
            was_empty_line = true;
        }

        while !surrounding_context.is_empty() &&
               surrounding_context[0].line.number <
                    line_number - options.context_lines_before {
           let entry = surrounding_context.pop_front().unwrap();
           if entry.print_on_discard {
               if entry.line.number > last_printed_lineno + 1 {
                   printer.print_ellipsis();
               }
               printer.print_context(entry.line.number, &entry.line.text);
               last_printed_lineno = entry.line.number;
           }
        }

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
                    // Merge all contexts.
                    let mut context_iter = context.iter().map(|e| &e.line);
                    let mut preprocessor_context_iter = preprocessor_context.iter().map(|e| &e.line);
                    let mut surrounding_context_iter = surrounding_context.iter().map(|e| &e.line);
                    let mut context_iters: [&mut Iterator<Item=&Line>; 3] = [
                        &mut context_iter,
                        &mut preprocessor_context_iter,
                        &mut surrounding_context_iter];
                    let combined_context = context_iters
                        .iter_mut()
                        .kmerge_by(|first, second| first.number < second.number)
                        .coalesce(|first, second| {
                            if first.number == second.number {
                                Ok(first)
                            } else {
                                Err((first, second))
                            }
                        })
                        .enumerate();

                    for (i, line) in combined_context {
                        if (!was_empty_line || !printer.options.breaks || i != 0) &&
                           line.number > last_printed_lineno + 1 {
                            printer.print_ellipsis();
                        }
                        printer.print_context(line.number, &line.text);
                        last_printed_lineno = line.number;
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
                surrounding_context.clear();
                was_empty_line = false;
                match_found = true;

                true
            } else {
                false
            }
        };

        if matched {
            // Start counting trailing lines after match.
            trailing_lines_left = options.context_lines_after;
        } else {
            if let Some(indentation) = indentation {
                context.push(ContextEntry { line: Line { number: line_number, text: line.clone() },
                                            indentation: indentation });
            }
            surrounding_context.push_back(
                SurroundContextEntry { line: Line { number: line_number, text: line },
                                       print_on_discard: trailing_lines_left > 0 });
            if trailing_lines_left > 0 { trailing_lines_left -= 1; }
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
    let options = parse_arguments(args)?;

    let appearance = AppearanceOptions {
        use_colors: match options.use_colors {
            UseColors::Always => true,
            UseColors::Never => false,
            UseColors::Auto => termion::is_tty(&std::io::stdout()),
        },
        color_scheme: {
            use termion::style::{Faint, NoFaint, Bold, Underline, NoUnderline, Reset as ResetStyle};
            use termion::color::{Fg, Blue, Red, Reset};
            match options.color_scheme {
                ColorSchemeSpec::Grey => ColorScheme {
                    filename:     (format!("{}", Underline), format!("{}", NoUnderline)),
                    // I wish to use `NoBold` here, but it doesn't work, at least on
                    // Mac with iTerm2. So use `Reset`.
                    matched_part: (format!("{}", Bold),      format!("{}", ResetStyle)),
                    context_line: (format!("{}", Faint),     format!("{}", NoFaint)),
                },
                ColorSchemeSpec::Colored => ColorScheme {
                    filename: (format!("{}", Fg(Blue)), format!("{}", Fg(Reset))),
                    matched_part: (format!("{}", Fg(Red)), format!("{}", Fg(Reset))),
                    context_line: (format!("{}", Faint), format!("{}", ResetStyle)),
                },
            }
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
            writeln!(std::io::stderr(), "{}", err).unwrap();
            std::process::exit(2);
        },
    }
}
