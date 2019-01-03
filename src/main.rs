#[macro_use] extern crate clap;
extern crate regex;
extern crate itertools;
extern crate termion;

mod error;
mod options;
mod printer;
mod io;
mod context;
mod contexts;
mod util;

#[cfg(test)] #[macro_use(assert_diff)] extern crate difference;
#[cfg(test)] mod tests;

use std::ffi::{OsStr, OsString};
use std::borrow::Cow;
use regex::{Regex, RegexBuilder};

use std::io::BufRead;
use std::io::Write as IoWrite;
use itertools::Itertools;

use error::OgrepError;
use options::{InputSpec, ColorSchemeSpec, Options, UseColors, PrintFilename,
              parse_arguments};
use printer::{AppearanceOptions, ColorScheme, Printer};
use io::{Input, Output};
use context::{Context, Line, Action};
use contexts::indentation::IndentationContext;
use contexts::preprocessor::PreprocessorContext;
use contexts::textual::TextualContext;
use contexts::children::ChildrenContext;

const LESS_ARGS: &[&str] = &["--quit-if-one-screen", "--RAW-CONTROL-CHARS",
                             "--quit-on-intr", "--no-init"];

fn calculate_indentation(s: &str) -> Option<usize> {
    s.find(|c: char| !c.is_whitespace())
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
    let mut indentation_context = IndentationContext::new(&options);

    // Secondary stack for preprocessor instructions.
    let mut preprocessor_context = PreprocessorContext::new(&options);

    // Context as it is understood by usual 'grep' - fixed number of
    // lines before and after matched one.
    let mut textual_context = TextualContext::new(&options, filepath);

    // Children context prints all children of matched lines.
    let mut children_context = ChildrenContext::new(&options, filepath);

    let mut contexts = vec![
        &mut textual_context as &mut Context,
        &mut preprocessor_context,
        &mut indentation_context,
    ];

    if options.children {
        contexts.push(&mut children_context);
    }

    // Whether at least one match was already found.
    let mut match_found = false;

    // Whether empty line was met since last match.
    let mut was_empty_line = false;

    'lines: for (line_number, line) in input.lines().enumerate().map(|(n, l)| (n+1, l)) {
        let line = line?;

        let indentation = calculate_indentation(&line);
        if indentation.is_none() {
            was_empty_line = true;
        }

        for mut context in &mut contexts {
            match context.pre_line(&Line { text: line.clone(), number: line_number },
                                   indentation, printer) {
                Action::Skip => continue 'lines,
                Action::Continue => (),
            }
        }

        let matched = {
            let mut matches = pattern.find_iter(&line).peekable();
            if matches.peek().is_some() {
                // `match_found` is checked to avoid extra line break before first match.
                if !match_found {
                    if let Some(ref path) = filepath {
                        printer.print_heading_filename(path)
                    }
                }
                if was_empty_line && match_found {
                    printer.print_break();
                }

                {
                    // Merge all contexts.
                    let combined_context = contexts.iter_mut()
                        .map(|c| c.dump())
                        .kmerge_by(|first, second| first.number < second.number)
                        .coalesce(|first, second| {
                            if first.number == second.number {
                                Ok(first)
                            } else {
                                Err((first, second))
                            }
                        })
                        .enumerate();

                    for (_, line) in combined_context {
                        printer.print_context(filepath, line.number, &line.text);
                    }

                    printer.print_match(filepath, line_number, &line, matches);
                }

                for mut context in &mut contexts {
                    context.clear();
                }
                was_empty_line = false;
                match_found = true;

                true
            } else {
                false
            }
        };

        if !matched {
            for mut context in &mut contexts {
                context.post_line(&Line { number: line_number, text: line.clone() },
                                  indentation);
            }
        }
    }

    for mut context in &mut contexts {
        context.end(printer);
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
        color_scheme: {
            use termion::style::{Faint, NoFaint, Bold, Underline, NoUnderline, Reset as ResetStyle};
            use termion::color::{Fg, Blue, Red, Reset};
            let use_colors = match options.use_colors {
                UseColors::Always => true,
                UseColors::Never => false,
                UseColors::Auto => termion::is_tty(&std::io::stdout()),
            };
            if use_colors {
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
            } else {
                ColorScheme {
                    filename: (String::new(), String::new()),
                    matched_part: (String::new(), String::new()),
                    context_line: (String::new(), String::new()),
                }
            }
        },
        breaks: options.breaks,
        ellipsis: options.ellipsis,
        print_filename: match options.print_filename {
            PrintFilename::No if options.use_git_grep => PrintFilename::PerFile,
            value => value
        }
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

        let mut printer = Printer::new(output_lock.as_write(), appearance);

        let mut pattern: Cow<str> =
            if options.regex {
                Cow::from(options.pattern.as_str())
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

                    {
                        let filepath = std::path::Path::new(line.trim_end_matches('\n'));

                        let mut file = std::fs::File::open(&filepath)?;
                        let mut input = std::io::BufReader::new(file);

                        printer.reset();
                        match_found |= process_input(&mut input, &re, &options, Some(filepath), &mut printer)?;
                    }

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
