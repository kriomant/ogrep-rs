use std::ffi::{OsStr, OsString};
use std::path::PathBuf;
use clap;

pub enum InputSpec {
    File(PathBuf),
    Stdin,
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

arg_enum!{
    #[derive(Debug, PartialEq, Eq, Clone, Copy)]
    pub enum PrintFilename { No, PerFile, PerLine }
}

pub struct Options {
    pub pattern: String,
    pub input: InputSpec,
    pub regex: bool,
    pub case_insensitive: bool,
    pub whole_word: bool,
    pub use_colors: UseColors,
    pub color_scheme: ColorSchemeSpec,
    pub use_pager: bool,
    pub use_git_grep: bool,
    pub breaks: bool,
    pub ellipsis: bool,
    pub print_filename: PrintFilename,
    pub smart_branches: bool,
    pub preprocessor: Preprocessor,
    pub context_lines_before: usize,
    pub context_lines_after: usize,
    pub children: bool,
}

pub fn parse_arguments<'i, Iter: Iterator<Item=OsString>>(args: Iter)
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
        .arg(Arg::with_name("children")
            .long("children")
            .help("Show all lines with greater indentation (children) after matching line"))
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
            .takes_value(true)
            .possible_values(&PrintFilename::variants())
            .case_insensitive(true)
            .help("When to print filename"))
        .arg(Arg::with_name("print-filename-per-file")
            .short("f")
            .conflicts_with_all(&["print-filename", "F"])
            .help("Print filename before first match in file, shortcut for --print-filename=per-file"))
        .arg(Arg::with_name("print-filename-per-line")
            .short("F")
            .conflicts_with_all(&["print-filename", "f"])
            .help("Print filename on each line, shortcut for --print-filename=per-line"))
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
        breaks: !matches.is_present("no-breaks") && !matches.is_present("children"),
        ellipsis: matches.is_present("ellipsis"),
        print_filename:
            if matches.is_present("print-filename") {
                value_t!(matches, "print-filename", PrintFilename)?
            } else if matches.is_present("print-filename-per-file") {
                PrintFilename::PerFile
            } else if matches.is_present("print-filename-per-line") {
                PrintFilename::PerLine
            } else {
                PrintFilename::No
            },
        smart_branches: !matches.is_present("no-smart-branches"),
        preprocessor: value_t!(matches, "preprocessor", Preprocessor)?,
        context_lines_before: before_context,
        context_lines_after: after_context,
        children: matches.is_present("children"),
    })
}
