use std;
use std::fs::File;
use std::io::BufReader;
use options::InputSpec;

pub enum Input {
    File(BufReader<File>),
    Stdin(std::io::Stdin),
}
pub enum InputLock<'a> {
    File(&'a mut std::io::BufReader<std::fs::File>),
    Stdin(std::io::StdinLock<'a>),
}
impl Input {
    pub fn open(spec: &InputSpec) -> std::io::Result<Self> {
        match *spec {
            InputSpec::File(ref path) => {
                let file = File::open(path)?;
                Ok(Input::File(BufReader::new(file)))
            },
            InputSpec::Stdin => Ok(Input::Stdin(std::io::stdin())),
        }
    }
    pub fn lock(&mut self) -> InputLock {
        match *self {
            Input::File(ref mut file) => InputLock::File(file),
            Input::Stdin(ref mut stdin) => InputLock::Stdin(stdin.lock()),
        }
    }
}
impl<'a> InputLock<'a> {
    pub fn as_buf_read(&mut self) -> &mut dyn std::io::BufRead {
        match self {
            &mut InputLock::File(ref mut reader) => reader,
            &mut InputLock::Stdin(ref mut lock) => lock,
        }
    }
}

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
