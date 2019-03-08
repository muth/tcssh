use std::borrow::Cow;
use std::fmt;
use std::result;

#[derive(Debug, PartialEq)]
pub struct Error {
    message: Cow<'static, str>,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> result::Result<(), fmt::Error> {
        self.message.fmt(f)
    }
}

pub type Result<T> = result::Result<T, Error>;

impl From<String> for Error {
    fn from(message: String) -> Error {
        Error {
            message: Cow::from(message),
        }
    }
}

impl From<&'static str> for Error {
    fn from(message: &'static str) -> Error {
        Error {
            message: Cow::from(message),
        }
    }
}

impl From<std::io::Error> for Error {
    fn from(error: std::io::Error) -> Self {
        Error {
            message: Cow::from(error.to_string()), // probably a better way to handle this.
                                                   // But if we have an error.. we're likely going to end the program
                                                   // so who cares about one more heap allocation.
        }
    }
}

impl From<regex::Error> for Error {
    fn from(error: regex::Error) -> Self {
        Error {
            message: Cow::from(error.to_string()),
        }
    }
}
