extern crate clap;

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

struct Options {
    pattern: String,
    input: InputSpec,
}

fn parse_arguments() -> Options {
    use clap::{App, Arg};

    let matches = App::new("ogrep")
        .about("Outline grep")
        .author("Mikhail Trishchenkov <kriomant@gmail.com>")
        .arg(Arg::with_name("pattern")
            .help("Pattern to search for")
            .required(true))
        .arg(Arg::with_name("input")
            .help("File to search in"))
        .get_matches();

    Options {
        pattern: matches.value_of("pattern").expect("pattern").to_string(),
        input: match matches.value_of_os("input").unwrap_or(OsStr::new("-")) {
          path if path == "-" => InputSpec::Stdin,
          path => InputSpec::File(PathBuf::from(path)),
        }
    }
}

fn calculate_indentation(s: &str) -> Option<usize> {
    s.find(|c: char| !c.is_whitespace())
}

fn process_input(input: &mut BufRead, pattern: &str) -> std::io::Result<()> {
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
                println!("{}", context_line);
            }
            println!("{}", line);
            context.clear();
        } else {
            context.push((indentation, line))
        }
    }

    Ok(())
}

fn real_main() -> std::result::Result<i32, Box<std::error::Error>> {
    let options = parse_arguments();
    let mut input = Input::open(&options.input)?;
    let mut input_lock = input.lock();
    process_input(input_lock.as_buf_read(), &options.pattern)?;
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

