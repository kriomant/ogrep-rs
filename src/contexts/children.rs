use std::path::Path;

use context::{Context, Line, Action};
use options::Options;
use printer::Printer;

pub struct ChildrenContext<'p> {
    filepath: Option<&'p Path>,

    last_line_indentation: usize,
    match_indentation: Option<usize>,
    context: Vec<Line>,
}

impl<'p> ChildrenContext<'p> {
    pub fn new<'o>(_options: &'o Options, filepath: Option<&'p Path>) -> Self {
        ChildrenContext {
            filepath: filepath,
            last_line_indentation: 0,
            match_indentation: None,
            context: Vec::new(),
        }
    }
}

impl<'p> Context for ChildrenContext<'p> {
    fn pre_line(&mut self, _line: &Line, indentation: Option<usize>, _printer: &mut Printer) -> Action {
        if let Some(indentation) = indentation {
            self.last_line_indentation = indentation;
        }
        Action::Continue
    }

    fn post_line(&mut self, line: &Line, indentation: Option<usize>) {
        let match_indentation = match self.match_indentation {
            Some(ind) => ind,
            None => return,
        };
        if indentation.map(|i| i > match_indentation).unwrap_or(true) {
            self.context.push(line.clone());
        } else {
            self.match_indentation = None;
        }
    }

    fn dump<'a>(&'a mut self) -> Box<Iterator<Item=&'a Line> + 'a> {
        Box::new(self.context.iter())
    }

    fn clear(&mut self) {
        if self.match_indentation.is_none() {
            self.match_indentation = Some(self.last_line_indentation);
        }
        self.context.clear();
    }

    fn end(&mut self, printer: &mut Printer) {
        for line in &self.context {
           printer.print_context(self.filepath, line.number, &line.text);
       }
    }
}
