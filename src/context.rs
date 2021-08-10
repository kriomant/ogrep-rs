use printer::Printer;

#[derive(Clone)]
pub struct Line {
    pub number: usize,
    pub text: String,
}

pub enum Action {
    Continue,
    Skip,
}

pub trait Context {
    /// Handle line before it is checked for matches. Context must update its state based
    /// on new line, but do not add this line into context yet. If line matches, `dump` will
    /// be called to get actual context. Otherwise, `post_line` will be called to put line into
    /// context.
    fn pre_line(&mut self, line: &Line, indentation: Option<usize>, printer: &mut Printer) -> Action;

    /// Put non-matching line into context, if needed.
    fn post_line(&mut self, line: &Line, indentation: Option<usize>);

    /// Returns current context lines.
    fn dump<'a>(&'a mut self) -> Box<dyn Iterator<Item=&'a Line> + 'a>;

    /// Clears context. Called after `dump`.
    fn clear(&mut self);

    /// Handle end of source text, flush all remaining lines, if needed.
    fn end(&mut self, printer: &mut Printer);
}
