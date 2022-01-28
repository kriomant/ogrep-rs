use options::Options;
use context::{Context, Line, Action};
use printer::Printer;
use util::starts_with_word;

// This prefixes are used when "smart branches" feature
// is turned on. When line starts with given prefix, then retain
// lines with same indentation starting with given prefixes in context.
const SMART_BRANCH_PREFIXES: &[(&str, &[&str])] = &[
    ("} else ", &["if", "} else if"]),
    ("else:", &["if", "else if"]),
    ("case", &["switch"]),
];

struct ContextEntry {
    line: Line,
    indentation: usize,
}

/// Indentation context — the heart of ogrep.
pub struct IndentationContext<'o> {
    options: &'o Options,

    /// Indentation context: last-processed line and it's parents (in terms of indentaion).
    ///
    /// ```text
    /// a      ← context[0], indentation=0
    ///   b
    ///     c
    ///   d    ← context[1], indentation=1
    ///     e  ← context[2], indentation=2
    /// ```
    ///
    /// Invariant: lines in context are sorted by indentation (ascending).
    /// Formal:
    /// ```ignore
    /// context.iter().tuple_windows().all(|(a,b)| {
    ///     if options.smart_branches {
    ///         a.indentation <= b.indentation
    ///     } else {
    ///         a.indentation < b.indentation
    ///     }
    /// })
    /// ```
    context: Vec<ContextEntry>,
}
impl<'o> IndentationContext<'o> {
    pub fn new(options: &'o Options) -> Self {
        IndentationContext {
            options: options,
            context: Vec::new(),
        }
    }
}
impl<'o> Context for IndentationContext<'o> {
    /// Handle next line.
    fn pre_line(&mut self, line: &Line, indentation: Option<usize>, _printer: &mut Printer) -> Action {
        let indentation = match indentation {
            Some(i) => i,
            // Empty lines shouldn't reset indentation, skip them
            None => return Action::Continue,
        };

        // Drop lines with indentation less than current line's one.
        //     a      ↑
        //       b    | context ← drop this line
        //         c  ↓         ← and this
        //     ------
        //       d              ← current line
        //
        // `top` is an index of last context line to leave, or None
        // if whole context must be dropped.
        let top = self.context.iter().rposition(|e: &ContextEntry| {
            // Upper scopes are always preserved.
            if e.indentation < indentation { return true; }
            if e.indentation > indentation { return false; }

            // Indentation is the same as of current line, push it out
            // when `smart_branches` option is off.
            if !self.options.smart_branches { return false; }

            // When `smart_branches` option is on, things are little harder.
            // We still pushes lines with greater or equal indentation out of
            // context, but we want to leave e.g. line with 'if' corresponding to
            // 'else' in current line.
            let stripped_line = &line.text[indentation..];
            let stripped_context_line = &e.line.text[e.indentation..];
            for &(prefix, context_prefixes) in SMART_BRANCH_PREFIXES {
                if starts_with_word(stripped_line, prefix) {
                    return context_prefixes.iter().any(|p| starts_with_word(stripped_context_line, p));
                }
            }

            // Current line is not part of branch statement, push it out.
            return false;
        });

        // Drop all context lines after one with `top` index.
        self.context.truncate(top.map(|t| t+1).unwrap_or(0));

        Action::Continue
    }

    fn post_line(&mut self, line: &Line, indentation: Option<usize>) {
        if let Some(indentation) = indentation {
            // We already pushed out all lines with greater or equal indentation
            // out of context in `pre_line`, …
            assert!(match self.context.last() {
                Some(last) =>
                    if self.options.smart_branches {
                        last.indentation <= indentation
                    } else {
                        last.indentation < indentation
                    }
                None => true
            });

            // … so just put new line into context.
            self.context.push(ContextEntry { line: line.clone(), indentation });
        }
    }

    /// Returns current context lines.
    fn dump<'a>(&'a mut self) -> Box<dyn Iterator<Item=&'a Line> + 'a> {
        Box::new(self.context.iter().map(|e| &e.line))
    }

    fn clear(&mut self) {
        self.context.clear();
    }

    fn end(&mut self, _printer: &mut Printer) {}
}
