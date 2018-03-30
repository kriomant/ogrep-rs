use super::*;

use std::path::PathBuf;
use regex::Regex;

/// Returns default options for tests.
/// Note that this is not the same as options used by CLI by default.
/// CLI by default enabled some features which are useful for most users,
/// like handling proprocessor lines, while default options for tests
/// enables only minimal possible set of features.
///
/// Use
/// ```
/// Options { print_filename: true, ..default_options() }
/// ```
/// to alter options.
fn default_options() -> Options {
    Options {
        pattern: String::new(),
        input: InputSpec::Stdin,
        regex: false,
        case_insensitive: false,
        whole_word: false,
        use_colors: UseColors::Never,
        color_scheme: ColorSchemeSpec::Grey,
        use_pager: false,
        use_git_grep: false,
        breaks: false,
        ellipsis: false,
        print_filename: false,
        smart_branches: false,
        preprocessor: Preprocessor::Preserve,
        context_lines_before: 0,
        context_lines_after: 0,
    }
}

/// Tests which lines are dipslayed in ogrep result.
///
/// `pattern` is fixed string to search for,
/// `specification` should be written in special format which is used to
/// prepare both input text and expected result. Each line must start with
/// ". ", "o " or "~ ":
///   * ". " means that line must be ommitted from result,
///   * "o " means that line should be printed,
///   * "~ " means that line not present in source should be printed, also
///     note that line number won't be prepended.
fn test(options: &Options, pattern: &str, specification: &str) {
    let mut input = String::with_capacity(specification.len());
    let mut expected_output = String::with_capacity(specification.len());

    let mut line_number = 0usize;
    for line in specification.lines() {
        let line = line.trim_left();
        if line.is_empty() { continue }
        assert!(&[". ", "o ", "~ "].iter().any(|p| line.starts_with(p)));
        let (to_input, to_expected, with_line_number) = match line.chars().next().unwrap() {
            '.' => (true, false, false),
            'o' => (true, true, true),
            '~' => (false, true, false),
            _ => unreachable!()
        };

        if to_input {
            line_number += 1;
            write!(input, "{}\n", &line[2..]).unwrap();
        }
        if to_expected {
            if with_line_number {
                write!(expected_output, "{:4}: {}\n", line_number, &line[2..]).unwrap();
            } else {
                write!(expected_output, "   {}\n", &line[2..]).unwrap();
            }
        }
    }

    let regex = Regex::new(pattern).expect("invalid regexp");
    let filepath = PathBuf::new();
    let mut result = std::io::Cursor::new(Vec::new());
    {
        let mut printer = Printer {
            output: &mut result,
            options: AppearanceOptions {
                use_colors: false,
                color_scheme: ColorScheme {
                    filename:     ("".to_string(), "".to_string()),
                    matched_part: ("".to_string(), "".to_string()),
                    context_line: ("".to_string(), "".to_string()),
                },
                breaks: options.breaks,
                ellipsis: options.ellipsis,
                print_filename: options.print_filename,
            }
        };
        let mut input = std::io::BufReader::new(std::io::Cursor::new(input));
        process_input(&mut input, &regex, &options, Some(&filepath), &mut printer).expect("i/o error");
    }

    let mut result = result.into_inner();

    // Hack to fix assert_diff! output which incorrectly indents first line.
    result.insert(0, b'\n');
    expected_output.insert(0, '\n');

    assert_diff!(&expected_output,
                 std::str::from_utf8(result.as_slice()).expect("output is not valid utf-8"),
                 "\n", 0);
}

/// Tests most basic ogrep function: showing context based on indentation.
#[test]
fn test_simple_context() {
    test(&default_options(), "bla",
         "o foo
          .   bar
          .     baz
          o   qux
          o     bla");
}

/// Tests that all corresponding if-else branches are preserved if match
/// is found in one of branches.
#[test]
fn test_smart_branches() {
    test(&Options { smart_branches: true, ..default_options() },
         "bla",

         "o if a > 0 {
          .   bar
          .     baz
          o } else {
          o   qux
          o     bla
          . }");
}

/// Tests that smart-branches feature handles Python code.
#[test]
fn test_smart_branches_python() {
    test(&Options { smart_branches: true, ..default_options() },
         "bla",

         "o if a > 0:
          .   bar
          .     baz
          o else:
          o   qux
          o     bla");
}

/// Tests 'preserve' mode of handling preprocessor instructions,
/// they must be treated just like usual lines.
#[test]
fn test_preprocessor_preserve() {
    test(&Options { preprocessor: Preprocessor::Preserve, ..default_options() },
         "bla",

         ". foo
          . #if defined(yup)
          .   bar
          .     baz
          o #else
          o   qux
          o     bla
          . #endif");
}

/// Tests 'ignore' mode of handling preprocessor instructions,
/// they must be completely ignored.
#[test]
fn test_preprocessor_ignore() {
    test(&Options { preprocessor: Preprocessor::Ignore, ..default_options() },
         "bla",

         "o foo
          . #if defined(yup)
          .   bar
          .     baz
          . #else
          o   qux
          o     bla
          . #endif");
}

/// Tests 'context' mode of handling preprocessor instructions,
/// they must form parallel context.
#[test]
fn test_preprocessor_context() {
    test(&Options { preprocessor: Preprocessor::Context, ..default_options() },
         "bla",

         "o foo
          o #if defined(yup)
          .   bar
          .     baz
          o #else
          o   qux
          o     bla
          . #endif");
}

/// Tests printing textual context.
#[test]
fn test_context_before() {
    test(&Options { context_lines_before: 1, ..default_options() },
         "bla",

         "o foo
          .   bar
          .     baz
          o   qux
          .     bat
          o     boo
          o     bla
          .     pug");
}

/// Tests printing textual context.
#[test]
fn test_context_after() {
    test(&Options { context_lines_after: 1, ..default_options() },
         "bla",

         "o foo
          .   bar
          .     baz
          o   qux
          .     bat
          .     boo
          o     bla
          o     pug");
}

/// Tests that ellipsis may be printed when lines are skipped.
#[test]
fn test_ellipsis() {
    test(&Options { ellipsis: true, ..default_options() },
         "bla",

         "o foo
          .   bar
          .     baz
          ~ …
          o   qux
          .     boo
          ~ …
          o     bla");
}
