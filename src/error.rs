use std;
use clap;

#[derive(Debug)]
pub enum OgrepError {
    ClapError(clap::Error),
    GitGrepFailed,
    InvalidOgrepOptions,
}
impl std::error::Error for OgrepError {
    /*fn description(&self) -> &str {
        match *self {
            OgrepError::ClapError(ref e) => e.to_string(),
            OgrepError::GitGrepFailed => "git grep failed",
            OgrepError::InvalidOgrepOptions => "OGREP_OPTIONS environment variable contains invalid UTF-8",
        }
    }*/
}
impl std::fmt::Display for OgrepError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> Result<(), std::fmt::Error> {
        match *self {
            OgrepError::ClapError(ref e) => write!(f, "{}", e),
            OgrepError::GitGrepFailed => write!(f, "git grep failed"),
            OgrepError::InvalidOgrepOptions => write!(f, "OGREP_OPTIONS environment variable contains invalid UTF-8"),
        }
    }
}
impl From<clap::Error> for OgrepError {
    fn from(e: clap::Error) -> OgrepError {
        OgrepError::ClapError(e)
    }
}
