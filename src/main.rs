extern crate atty;
#[macro_use] extern crate clap;
extern crate ansi_term;
extern crate itertools;

use std::ffi::OsStr;
use std::path::PathBuf;

use std::io::BufRead;
use std::io::Write;
use itertools::Itertools;
use std::os::unix::ffi::OsStrExt;

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

arg_enum!{
    #[derive(Debug)]
    pub enum UseColors { Always, Auto, Never }
}

struct Options {
    pattern: String,
    input: InputSpec,
    use_colors: UseColors,
    breaks: bool,
    ellipsis: bool,
    print_filename: bool,
}

struct AppearanceOptions {
    use_colors: bool,
    breaks: bool,
    ellipsis: bool,
    print_filename: bool,
}

struct Printer {
    options: AppearanceOptions,
}
impl Printer {
    fn print_context(&self, line_number: usize, line: &str) {
        if self.options.use_colors {
            let text = format!("{:4}: {}", line_number, line);
            println!("{}", ansi_term::Style::new().dimmed().paint(text));
        } else {
            println!("{:4}: {}", line_number, line);
        }
    }

    fn print_match(&self, line_number: usize, line: &str, pattern: &str) {
        if self.options.use_colors {
            let dflt_style = ansi_term::Style::new();
            let match_style = ansi_term::Style::new().bold();
            let match_str = match_style.paint(pattern);

            print!("{:4}: ", line_number);
            line.split(pattern).map(|p| dflt_style.paint(p)).intersperse(match_str).for_each(|p| {
                print!("{}", p);
            });
            print!("\n");

        } else {
            println!("{:4}: {}", line_number, line);
        }
    }

    fn print_break(&self) {
        if self.options.breaks {
            println!();
        }
    }

    fn print_ellipsis(&self) {
        if self.options.ellipsis {
            println!("   {}", ansi_term::Style::new().dimmed().paint("…"));
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

fn parse_arguments() -> Options {
    use clap::{App, Arg};

    let colors_default = UseColors::Auto.to_string();

    let matches = App::new("ogrep")
        .about("Outline grep")
        .author("Mikhail Trishchenkov <kriomant@gmail.com>")
        .arg(Arg::with_name("pattern")
            .help("Pattern to search for")
            .required(true))
        .arg(Arg::with_name("input")
            .help("File to search in"))
        .arg(Arg::with_name("color")
            .long("color")
            .takes_value(true)
            .default_value(&colors_default)
            .possible_values(&UseColors::variants())
            .case_insensitive(true)
            .help("File to search in"))
        .arg(Arg::with_name("no-breaks")
            .long("no-breaks")
            .help("Don't preserve line breaks"))
        .arg(Arg::with_name("ellipsis")
            .long("ellipsis")
            .help("Print ellipsis when lines were skipped"))
        .arg(Arg::with_name("print-filename")
            .long("print-filename")
            .help("Print filename on match"))
        .get_matches();

    Options {
        pattern: matches.value_of("pattern").expect("pattern").to_string(),
        input: match matches.value_of_os("input").unwrap_or(OsStr::new("-")) {
          path if path == "-" => InputSpec::Stdin,
          path => InputSpec::File(PathBuf::from(path)),
        },
        use_colors: value_t!(matches, "color", UseColors).unwrap_or_else(|e| e.exit()),
        breaks: !matches.is_present("no-breaks"),
        ellipsis: matches.is_present("ellipsis"),
        print_filename: matches.is_present("print-filename"),
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

fn process_input(input: &mut BufRead, pattern: &str, input_spec: &InputSpec, printer: &Printer) -> std::io::Result<()> {
    let mut context = Vec::new();

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

        let top = context.iter().rposition(|e: &ContextEntry| e.indentation < indentation);
        context.truncate(top.map(|t| t+1).unwrap_or(0));

        if line.contains(pattern) {
            // `match_found` is checked to avoid extra line break before first match.
            if !match_found {
                if let &InputSpec::File(ref path) = input_spec {
                    printer.print_filename(path)
                }
            }
            if was_empty_line && match_found {
                printer.print_break();
            }

            for entry in &context {
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
            printer.print_match(line_number, &line, pattern);
            last_printed_lineno = line_number;

            context.clear();
            was_empty_line = false;
            match_found = true;

        } else {
            context.push(ContextEntry { line_number, indentation, line })
        }
    }

    Ok(())
}

fn real_main() -> std::result::Result<i32, Box<std::error::Error>> {
    let options = parse_arguments();

    let appearance = AppearanceOptions {
        use_colors: match options.use_colors {
            UseColors::Always => true,
            UseColors::Never => false,
            UseColors::Auto => atty::is(atty::Stream::Stdout),
        },
        breaks: options.breaks,
        ellipsis: options.ellipsis,
        print_filename: options.print_filename,
    };

    let printer = Printer { options: appearance };

    let mut input = Input::open(&options.input)?;
    let mut input_lock = input.lock();
    process_input(input_lock.as_buf_read(), &options.pattern, &options.input, &printer)?;
    Ok(0)
}

fn main() {
    match real_main() {
        Ok(code) => std::process::exit(code),
        Err(err) => {
            writeln!(std::io::stderr(), "{}", err.description()).unwrap();
            std::process::exit(127);
        },
    }
}

