use std;
use clap;

#[derive(Debug)]
pub enum OgrepError {
    ClapError(clap::Error),
    GitGrepWithStdinInput,
    GitGrepFailed,
    InvalidOgrepOptions,
}
impl std::error::Error for OgrepError {
    fn description(&self) -> &str {
        match *self {
            OgrepError::ClapError(ref e) => e.description(),
            OgrepError::GitGrepWithStdinInput => "Don't use '-' input with --use-git-grep option",
            OgrepError::GitGrepFailed => "git grep failed",
            OgrepError::InvalidOgrepOptions => "OGREP_OPTIONS environment variable contains invalid UTF-8",
        }
    }
}
impl std::fmt::Display for OgrepError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> Result<(), std::fmt::Error> {
        use std::error::Error;
        match *self {
            OgrepError::ClapError(ref e) => write!(f, "{}", e),
            _ => write!(f, "{}", self.description())
        }
    }
}
impl From<clap::Error> for OgrepError {
    fn from(e: clap::Error) -> OgrepError {
        OgrepError::ClapError(e)
    }
}
