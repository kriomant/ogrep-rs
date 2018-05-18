use std;
use regex;

use std::os::unix::ffi::OsStrExt;
use std::fmt::Write as FmtWrite;

pub struct ColorScheme {
    pub filename: (String, String),
    pub matched_part: (String, String),
    pub context_line: (String, String),
}

pub struct AppearanceOptions {
    pub color_scheme: ColorScheme,
    pub breaks: bool,
    pub ellipsis: bool,
    pub print_filename: bool,
}

pub struct Printer<'o> {
	pub output: &'o mut std::io::Write,
    pub options: AppearanceOptions,
    last_printed_lineno: usize,
    was_break: bool,
}

impl<'o> Printer<'o> {
    pub fn new(output: &'o mut std::io::Write, options: AppearanceOptions) -> Self {
        Printer {
            output: output,
            options: options,
            last_printed_lineno: 0,
            was_break: false,
        }
    }

    pub fn reset(&mut self) {
        self.last_printed_lineno = 0;
        self.was_break = false;
    }

    pub fn print_context(&mut self, line_number: usize, line: &str) {
        assert!(line_number > self.last_printed_lineno);
        self.maybe_print_ellipsis(line_number);
        writeln!(self.output, "{color}{:4}: {}{nocolor}", line_number, line,
                 color=self.options.color_scheme.context_line.0,
                 nocolor=self.options.color_scheme.context_line.1).unwrap();
        self.last_printed_lineno = line_number;
    }

    pub fn print_match<'m, M>(&mut self, line_number: usize, line: &str, matches: M)
            where M: Iterator<Item=regex::Match<'m>> {
        assert!(line_number > self.last_printed_lineno);
        self.maybe_print_ellipsis(line_number);
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
        self.last_printed_lineno = line_number;
    }

    pub fn print_break(&mut self) {
        if self.options.breaks {
            writeln!(self.output).unwrap();
            self.was_break = true;
        }
    }

    fn maybe_print_ellipsis(&mut self, line_number: usize) {
        if self.was_break {
            self.was_break = false;
            return;
        }
        if line_number > self.last_printed_lineno + 1 {
            self.print_ellipsis();
        }
    }

    pub fn print_ellipsis(&mut self) {
        if self.options.ellipsis {
            writeln!(self.output, "   {color}{}{nocolor}", "…",
                     color=self.options.color_scheme.context_line.0,
                     nocolor=self.options.color_scheme.context_line.1).unwrap();
        }
    }

    pub fn print_filename(&mut self, filename: &std::path::Path) {
        assert!(self.last_printed_lineno == 0);
        if self.options.print_filename {
            self.output.write(b"\n").unwrap();
            write!(&mut self.output, "{}", self.options.color_scheme.filename.0).unwrap();
            self.output.write(filename.as_os_str().as_bytes()).unwrap();
            write!(&mut self.output, "{}", self.options.color_scheme.filename.1).unwrap();
            self.output.write(b"\n\n").unwrap();
        }
    }
}
