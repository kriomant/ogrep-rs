use std;
use std::path::Path;

use context::{Context, Line, Action};
use options::Options;
use printer::Printer;

struct Entry {
    line: Line,
    print_on_discard: bool,
}

pub struct TextualContext<'p, 'o> {
    filepath: Option<&'p Path>,
    options: &'o Options,
    context: std::collections::VecDeque<Entry>,

    // How many trailing lines after match left to print.
    trailing_lines_left: usize,
}

impl<'p, 'o> TextualContext<'p, 'o> {
    pub fn new(options: &'o Options, filepath: Option<&'p Path>) -> Self {
        TextualContext {
            filepath: filepath,
            options: options,
            context: std::collections::VecDeque::with_capacity(
                options.context_lines_before + options.context_lines_after),
            trailing_lines_left: 0,
        }
    }
}

impl<'p, 'o> Context for TextualContext<'p, 'o> {
    fn pre_line(&mut self, line: &Line, _indentation: Option<usize>, printer: &mut Printer) -> Action {
        while !self.context.is_empty() &&
              self.context[0].line.number <
                  line.number - self.options.context_lines_before {
           let entry = self.context.pop_front().unwrap();
           if entry.print_on_discard {
               printer.print_context(self.filepath, entry.line.number, &entry.line.text);
           }
        }
        Action::Continue
    }

    fn post_line(&mut self, line: &Line, _indentation: Option<usize>) {
        self.context.push_back(
            Entry { line: line.clone(), print_on_discard: self.trailing_lines_left > 0 });
        if self.trailing_lines_left > 0 { self.trailing_lines_left -= 1; }
    }

    fn dump<'a>(&'a mut self) -> Box<dyn Iterator<Item=&'a Line> + 'a> {
        Box::new(self.context.iter().map(|e| &e.line))
    }

    fn clear(&mut self) {
        // Start counting trailing lines after match.
        self.trailing_lines_left = self.options.context_lines_after;
    }

    fn end(&mut self, printer: &mut Printer) {
        while let Some(&Entry { print_on_discard: true, ..}) = self.context.front() {
           let entry = self.context.pop_front().unwrap();
           printer.print_context(self.filepath, entry.line.number, &entry.line.text);
       }
    }
}
