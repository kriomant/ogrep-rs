use std;

pub enum Output {
    Pager(std::process::Child),
    Stdout(std::io::Stdout),
}
pub enum OutputLock<'a> {
    Pager(&'a mut std::process::ChildStdin),
    Stdout(std::io::StdoutLock<'a>),
}
impl Output {
    pub fn lock(&mut self) -> OutputLock {
        match *self {
            Output::Pager(ref mut process) => OutputLock::Pager(process.stdin.as_mut().unwrap()),
            Output::Stdout(ref mut stdout) => OutputLock::Stdout(stdout.lock()),
        }
    }

    pub fn close(mut self) -> Result<(), Box<dyn std::error::Error>> {
        self.close_impl()
    }

    pub fn close_impl(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        match self {
            &mut Output::Pager(ref mut process) => { process.wait()?; Ok(()) },
            &mut Output::Stdout(_) => Ok(()),
        }
    }
}
impl Drop for Output {
    fn drop(&mut self) {
        let _ = self.close_impl();
    }
}
impl<'a> OutputLock<'a> {
    pub fn as_write(&mut self) -> &mut dyn std::io::Write {
        match self {
            &mut OutputLock::Pager(ref mut stdin) => stdin,
            &mut OutputLock::Stdout(ref mut lock) => lock,
        }
    }
}
