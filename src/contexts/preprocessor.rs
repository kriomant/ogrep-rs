use context::{Context, Line, Action};
use util::starts_with_word;
use options::{Options, Preprocessor};
use printer::Printer;

struct Entry {
    line: Line,
    level: usize,
}

enum PreprocessorKind { If, Else, Endif, Other }
fn preprocessor_instruction_kind(s: &str) -> Option<PreprocessorKind> {
    match s {
        _ if starts_with_word(s, "#if") => Some(PreprocessorKind::If),
        _ if starts_with_word(s, "#else") => Some(PreprocessorKind::Else),
        _ if starts_with_word(s, "#endif") => Some(PreprocessorKind::Endif),
        _ if starts_with_word(s, "#") => Some(PreprocessorKind::Other),
        _ => None,
    }
}

pub struct PreprocessorContext<'o> {
    options: &'o Options,
    level: usize,
    context: Vec<Entry>,
}
impl<'o> PreprocessorContext<'o> {
    pub fn new(options: &'o Options) -> Self {
        PreprocessorContext {
            options: options,
            level: 0usize,
            context: Vec::new(),
        }
    }
}

impl<'o> Context for PreprocessorContext<'o> {
    /// Handle next line.
    fn pre_line(&mut self, line: &Line, indentation: Option<usize>, _printer: &mut Printer) -> Action {
        let indentation = match indentation {
            Some(i) => i,
            None => return Action::Continue,
        };

        // Ignore lines looking like C preprocessor instruction, because they
        // are often written without indentation and this breaks context.
        match self.options.preprocessor {
            Preprocessor::Preserve => Action::Continue, // Do nothing, handle line as usual
            Preprocessor::Ignore =>
                if preprocessor_instruction_kind(&line.text[indentation..]).is_some() {
                    Action::Skip
                } else {
                    Action::Continue
                },
            Preprocessor::Context =>
                match preprocessor_instruction_kind(&line.text[indentation..]) {
                    None => Action::Continue,
                    Some(PreprocessorKind::If) => {
                        self.level += 1;
                        self.context.push(Entry { line: line.clone(), level: self.level });
                        Action::Skip
                    },
                    Some(PreprocessorKind::Else) => {
                        self.context.push(Entry { line: line.clone(), level: self.level });
                        Action::Skip
                    },
                    Some(PreprocessorKind::Endif) => {
                        let top = self.context.iter().rposition(|e: &Entry| {
                            e.level < self.level
                        });
                        self.context.truncate(top.map(|t| t+1).unwrap_or(0));
                        self.level -= 1;
                        Action::Skip
                    },
                    Some(PreprocessorKind::Other) => Action::Skip
                }
        }
    }

    fn post_line(&mut self, _line: &Line, _indentation: Option<usize>) {}

    /// Returns current context lines.
    fn dump<'a>(&'a mut self) -> Box<Iterator<Item=&'a Line> + 'a> {
        Box::new(self.context.iter().map(|e| &e.line))
    }

    fn clear(&mut self) {
        self.context.clear();
    }

    /// Handle end of source text.
    fn end(&mut self, _printer: &mut Printer) {}
}
