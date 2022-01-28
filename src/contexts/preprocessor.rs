use context::{Context, Line, Action};
use options::{Options, Preprocessor};
use printer::Printer;
use regex::Regex;

struct Entry {
    line: Line,
    level: usize,
}

enum PreprocessorKind { If, Else, Endif, Other }

pub struct PreprocessorContext<'o> {
    options: &'o Options,
    level: usize,
    context: Vec<Entry>,

    if_regex: Regex,
    else_regex: Regex,
    endif_regex: Regex,
    other_regex: Regex,
}
impl<'o> PreprocessorContext<'o> {
    pub fn new(options: &'o Options) -> Self {
        PreprocessorContext {
            options: options,
            level: 0usize,
            context: Vec::new(),

            if_regex: Regex::new(r"^\s*(?:#|\{%-?)\s*if\b").unwrap(),
            else_regex: Regex::new(r"^\s*(?:#|\{%-?)\s*else\b").unwrap(),
            endif_regex: Regex::new(r"^\s*(?:#|\{%-?)\s*endif\b").unwrap(),
            other_regex: Regex::new(r"^\s*(?:#|\{%-?)").unwrap(),
        }
    }

    fn preprocessor_instruction_kind(&self, s: &str) -> Option<PreprocessorKind> {
        if self.if_regex.is_match(s) { return Some(PreprocessorKind::If) }
        if self.else_regex.is_match(s) { return Some(PreprocessorKind::Else) }
        if self.endif_regex.is_match(s) { return Some(PreprocessorKind::Endif) }
        if self.other_regex.is_match(s) { return Some(PreprocessorKind::Other) }
        return None;
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
                if self.preprocessor_instruction_kind(&line.text[indentation..]).is_some() {
                    Action::Skip
                } else {
                    Action::Continue
                },
            Preprocessor::Context =>
                match self.preprocessor_instruction_kind(&line.text[indentation..]) {
                    None => Action::Continue,
                    Some(PreprocessorKind::If) => {
                        self.level += 1;
                        Action::Skip
                    },
                    Some(PreprocessorKind::Else) => {
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

    fn post_line(&mut self, line: &Line, indentation: Option<usize>) {
        let indentation = match indentation {
            Some(i) => i,
            None => return,
        };

        // Ignore lines looking like C preprocessor instruction, because they
        // are often written without indentation and this breaks context.
        match self.options.preprocessor {
            Preprocessor::Preserve => (),
            Preprocessor::Ignore => (),
            Preprocessor::Context =>
                match self.preprocessor_instruction_kind(&line.text[indentation..]) {
                    None => (),
                    Some(PreprocessorKind::If) => {
                        self.context.push(Entry { line: line.clone(), level: self.level });
                    },
                    Some(PreprocessorKind::Else) => {
                        self.context.push(Entry { line: line.clone(), level: self.level });
                    },
                    Some(PreprocessorKind::Endif) => (),
                    Some(PreprocessorKind::Other) => (),
                }
        }
    }

    /// Returns current context lines.
    fn dump<'a>(&'a mut self) -> Box<dyn Iterator<Item=&'a Line> + 'a> {
        Box::new(self.context.iter().map(|e| &e.line))
    }

    fn clear(&mut self) {
        self.context.clear();
    }

    /// Handle end of source text.
    fn end(&mut self, _printer: &mut Printer) {}
}
