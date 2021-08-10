use super::*;
use super::options::*;

use regex::Regex;

use std::fmt::Write as FmtWrite;

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
        input: Some(InputSpec::Stdin),
        regex: false,
        case_insensitive: false,
        whole_word: false,
        use_colors: UseColors::Never,
        color_scheme: ColorSchemeSpec::Grey,
        use_pager: false,
        use_git_grep: false,
        breaks: false,
        ellipsis: false,
        print_filename: PrintFilename::No,
        smart_branches: false,
        preprocessor: Preprocessor::Preserve,
        context_lines_before: 0,
        context_lines_after: 0,
        children: false,
    }
}

/// Tests which lines are dipslayed in ogrep result.
///
/// `pattern` is fixed string to search for,
/// `specification` should be written in special format which is used to
/// prepare both input text and expected result. Each line must start with
/// one of:
///   * line starting with ". " means that line must be ommitted from result,
///   * line starting with "o " means that line must be printed,
///   * "~ …" means that ellipsis should be printed
///   * "~" means that break should be printed
fn test(options: &Options, pattern: &str, specification: &str) {
    let mut input = String::with_capacity(specification.len());
    let mut expected_output = String::with_capacity(specification.len());

    fn rest_of_line(line: &str) -> &str {
        assert!(!line.is_empty());
        if line.len() == 1 {
            return "";
        } else {
            assert_eq!(line.chars().skip(1).next(), Some(' '));
            return &line[2..];
        }
    }

    let mut line_number = 0usize;
    for line in specification.lines() {
        let line = line.trim_start();

        if line.is_empty() { continue }
        let (to_input, to_expected) = match line {
            l if l.starts_with(".") => (Some(rest_of_line(line)), None),
            l if l.starts_with("o") => (Some(rest_of_line(line)),
                                        Some((rest_of_line(line), true))),
            "~ …" => (None, Some(("   …", false))),
            "~" => (None, Some(("", false))),
            _ => panic!("unexpected specification line: {}", line),
        };

        if let Some(to_input) = to_input {
            line_number += 1;
            write!(input, "{}\n", to_input).unwrap();
        }
        if let Some((expected, with_line_number)) = to_expected {
            if with_line_number {
                write!(expected_output, "{:4}: {}\n", line_number, expected).unwrap();
            } else {
                write!(expected_output, "{}\n", expected).unwrap();
            }
        }
    }

    let regex = Regex::new(pattern).expect("invalid regexp");
    let mut result = std::io::Cursor::new(Vec::new());
    {
        let mut printer = Printer::new(
            &mut result,
            AppearanceOptions {
                color_scheme: ColorScheme {
                    filename:     ("".to_string(), "".to_string()),
                    matched_part: ("".to_string(), "".to_string()),
                    context_line: ("".to_string(), "".to_string()),
                },
                breaks: options.breaks,
                ellipsis: options.ellipsis,
                print_filename: options.print_filename,
            });
        let mut input = std::io::BufReader::new(std::io::Cursor::new(input));
        process_input(&mut input, &regex, &options, None, &mut printer).expect("i/o error");
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

/// Tests that branches are recognized even when there are no
/// space after 'if'.
#[test]
fn test_smart_branches_no_space() {
    test(&Options { smart_branches: true, ..default_options() },
         "bla",

         "o if(a > 0) {
          .   bar
          .     baz
          o } else {
          o   qux
          o     bla
          . }");
}

/// Tests that branches are NOT recognized when first words
/// has 'if' prefix.
#[test]
fn test_smart_branches_if_prefix() {
    test(&Options { smart_branches: true, ..default_options() },
         "bla",

         ". ifere(a > 0) {
          .   bar
          .     baz
          o } else {
          o   qux
          o     bla
          . }");
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

/// Tests that breaks are printed instead of ellipsis when there
/// was empty line in source text.
#[test]
fn test_breaks() {
    test(&Options { breaks: true, ..default_options() },
         "bla",

         "o foo
          .   bar
          .     baz
          .
          o   qux
          o     bla
          .
          ~
          o     bla");
}

/// Tests that breaks are printed instead of ellipsis when there
/// was empty line in source text.
#[test]
fn test_breaks_incorrect() {
    test(&Options { breaks: true, ..default_options() },
         "bla",

         "o foo
          .   bar
          .     baz
          .
          o   qux
          o     bla
          ~
          o   fux
          .
          o     bla");
}

/// Tests printing all children of matched line.
#[test]
fn test_children() {
    test(&Options { children: true, ..default_options() },
         "foo",

         "o foo
          o   bar
          o     baz");
}

/// Tests printing all children of matched line when there is
/// another match inside children.
#[test]
fn test_nested_children() {
    test(&Options { children: true, ..default_options() },
         "foo",

         "o foo
          o   bar
          o     foo
          o   baz");
}

/// Tests printing breaks together with children context.
/// It doesn't work right now and current workaround is to disable breaks
/// when children option is used.
// #[test]
#[allow(dead_code)]
fn test_children_breaks() {
    test(&Options { breaks: true, children: true, ..default_options() },
         "foo",

         "o foo
          o   bar
          o
          o   baz
          ~
          o foo
          o   bar");
}
