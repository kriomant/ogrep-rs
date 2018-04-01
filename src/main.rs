#[macro_use] extern crate clap;
extern crate regex;
extern crate itertools;
extern crate termion;

mod error;
mod options;
mod printer;
mod io;

#[cfg(test)] #[macro_use(assert_diff)] extern crate difference;
#[cfg(test)] mod tests;

use std::ffi::{OsStr, OsString};
use std::borrow::Cow;
use regex::{Regex, RegexBuilder};

use std::io::BufRead;
use std::io::Write as IoWrite;
use itertools::Itertools;

use error::OgrepError;
use options::{InputSpec, ColorSchemeSpec, Options, Preprocessor, UseColors, parse_arguments};
use printer::{AppearanceOptions, ColorScheme, Printer};
use io::{Input, Output};

// This prefixes are used when "smart branches" feature
// is turned on. When line starts with given prefix, then retain
// lines with same indentation starting with given prefixes in context.
const SMART_BRANCH_PREFIXES: &[(&str, &[&str])] = &[
    ("} else ", &["if", "} else if"]),
    ("else:", &["if", "else if"]),
    ("case", &["switch"]),
];

const LESS_ARGS: &[&str] = &["--quit-if-one-screen", "--RAW-CONTROL-CHARS",
                             "--quit-on-intr", "--no-init"];

/// Checks whether `text` starts with `prefix` and there is word boundary
/// right after prefix, i.e. either `text` ends there or next character
/// is not alphanumberic.
fn starts_with_word(text: &str, prefix: &str) -> bool {
    text.starts_with(prefix) &&
        text[prefix.len()..].chars().next().map(|c| !c.is_ascii_alphanumeric()).unwrap_or(true)
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
        _ if starts_with_word(s, "#if") => Some(PreprocessorKind::If),
        _ if starts_with_word(s, "#else") => Some(PreprocessorKind::Else),
        _ if starts_with_word(s, "#endif") => Some(PreprocessorKind::Endif),
        _ if starts_with_word(s, "#") => Some(PreprocessorKind::Other),
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
                    if starts_with_word(stripped_line, prefix) {
                        return context_prefixes.iter().any(|p| starts_with_word(stripped_context_line, p));
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

    while let Some(&SurroundContextEntry { print_on_discard: true, ..}) =
            surrounding_context.front() {
       let entry = surrounding_context.pop_front().unwrap();
       if entry.line.number > last_printed_lineno + 1 {
           printer.print_ellipsis();
       }
       printer.print_context(entry.line.number, &entry.line.text);
       last_printed_lineno = entry.line.number;
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
                    line.clear();
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
