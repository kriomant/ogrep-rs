extern crate atty;
#[macro_use] extern crate clap;
extern crate ansi_term;

use std::ffi::OsStr;
use std::path::PathBuf;

use std::io::BufRead;
use std::io::Write;

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
}

struct AppearanceOptions {
    use_colors: bool,
}

struct Printer {
    options: AppearanceOptions,
}
impl Printer {
    fn print_context(&self, line: &str) {
        if self.options.use_colors {
            println!("{}", ansi_term::Style::new().dimmed().paint(line));
        } else {
            println!("{}", line);
        }
    }

    fn print_match(&self, line: &str) {
        if self.options.use_colors {
            println!("{}", ansi_term::Style::new().bold().paint(line));
        } else {
            println!("{}", line);
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
        .get_matches();

    Options {
        pattern: matches.value_of("pattern").expect("pattern").to_string(),
        input: match matches.value_of_os("input").unwrap_or(OsStr::new("-")) {
          path if path == "-" => InputSpec::Stdin,
          path => InputSpec::File(PathBuf::from(path)),
        },
        use_colors: value_t!(matches, "color", UseColors).unwrap_or_else(|e| e.exit()),
    }
}

fn calculate_indentation(s: &str) -> Option<usize> {
    s.find(|c: char| !c.is_whitespace())
}

fn process_input(input: &mut BufRead, pattern: &str, printer: &Printer) -> std::io::Result<()> {
    let mut context = Vec::new();

    for line in input.lines() {
        let line = line?;
        let indentation = match calculate_indentation(&line) {
            Some(ind) => ind,
            None => continue,
        };

        let top = context.iter().rposition(|&(ind, _): &(usize, String)| ind < indentation);
        context.truncate(top.map(|t| t+1).unwrap_or(0));

        if line.contains(pattern) {
            for &(_, ref context_line) in &context {
                printer.print_context(context_line);
            }
            printer.print_match(&line);
            context.clear();
        } else {
            context.push((indentation, line))
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
    };

    let printer = Printer { options: appearance };

    let mut input = Input::open(&options.input)?;
    let mut input_lock = input.lock();
    process_input(input_lock.as_buf_read(), &options.pattern, &printer)?;
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

