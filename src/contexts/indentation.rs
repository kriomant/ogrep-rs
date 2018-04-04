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

pub struct IndentationContext<'o> {
    options: &'o Options,
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
            None => return Action::Continue,
        };

        let top = self.context.iter().rposition(|e: &ContextEntry| {
            // Upper scopes are always preserved.
            if e.indentation < indentation { return true; }
            if e.indentation > indentation { return false; }

            if !self.options.smart_branches { return false; }

            let stripped_line = &line.text[indentation..];
            let stripped_context_line = &e.line.text[e.indentation..];
            for &(prefix, context_prefixes) in SMART_BRANCH_PREFIXES {
                if starts_with_word(stripped_line, prefix) {
                    return context_prefixes.iter().any(|p| starts_with_word(stripped_context_line, p));
                }
            }

            return false;
        });
        self.context.truncate(top.map(|t| t+1).unwrap_or(0));

        Action::Continue
    }

    fn post_line(&mut self, line: &Line, indentation: Option<usize>) {
        if let Some(indentation) = indentation {
            self.context.push(ContextEntry { line: line.clone(), indentation });
        }
    }

    /// Returns current context lines.
    fn dump<'a>(&'a mut self) -> Box<Iterator<Item=&'a Line> + 'a> {
        Box::new(self.context.iter().map(|e| &e.line))
    }

    fn clear(&mut self) {
        self.context.clear();
    }

    fn end(&mut self, _printer: &mut Printer) {}
}
