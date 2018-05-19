use std;
use regex;

use std::os::unix::ffi::OsStrExt;
use std::fmt::Write as FmtWrite;
use std::path::Path;

use options::PrintFilename;

pub struct ColorScheme {
    pub filename: (String, String),
    pub matched_part: (String, String),
    pub context_line: (String, String),
}

pub struct AppearanceOptions {
    pub color_scheme: ColorScheme,
    pub breaks: bool,
    pub ellipsis: bool,
    pub print_filename: PrintFilename,
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

    pub fn print_context(&mut self, filepath: Option<&Path>, line_number: usize, line: &str) {
        assert!(line_number > self.last_printed_lineno);
        self.maybe_print_ellipsis(line_number);

        match (self.options.print_filename, filepath) {
            (PrintFilename::PerLine, Some(path)) =>
                write!(self.output, "{color}{}:{:04}:{nocolor} ",
                       path.to_string_lossy(), line_number,
                       color=self.options.color_scheme.context_line.0,
                       nocolor=self.options.color_scheme.context_line.1).unwrap(),
            _ => write!(self.output, "{color}{:4}:{nocolor} ",
                        line_number,
                        color=self.options.color_scheme.context_line.0,
                        nocolor=self.options.color_scheme.context_line.1).unwrap(),
        }

        writeln!(self.output, "{color}{}{nocolor}", line,
                 color=self.options.color_scheme.context_line.0,
                 nocolor=self.options.color_scheme.context_line.1).unwrap();
        self.last_printed_lineno = line_number;
    }

    pub fn print_match<'m, M>(&mut self, filepath: Option<&Path>, line_number: usize,
                              line: &str, matches: M)
            where M: Iterator<Item=regex::Match<'m>> {
        assert!(line_number > self.last_printed_lineno);
        self.maybe_print_ellipsis(line_number);

        match (self.options.print_filename, filepath) {
            (PrintFilename::PerLine, Some(path)) =>
                write!(self.output, "{}:{:04}: ",
                       path.to_string_lossy(), line_number).unwrap(),
            _ => write!(self.output, "{:4}: ", line_number).unwrap(),
        }

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

        writeln!(self.output, "{}", buf).unwrap();
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

    pub fn print_heading_filename(&mut self, filename: &std::path::Path) {
        assert!(self.last_printed_lineno == 0);
        self.output.write(b"\n").unwrap();
        if self.options.print_filename == PrintFilename::PerFile {
            write!(&mut self.output, "{}", self.options.color_scheme.filename.0).unwrap();
            self.output.write(filename.as_os_str().as_bytes()).unwrap();
            write!(&mut self.output, "{}", self.options.color_scheme.filename.1).unwrap();
            self.output.write(b"\n\n").unwrap();
        }
    }
}
