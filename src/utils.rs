use std::fmt;
use std::fs::{File, OpenOptions};
use std::io::{self, prelude::*, Seek, SeekFrom, Write};

#[derive(Debug)]
pub enum PersFdError {
    ReadErr(io::Error),
    WriteErr(io::Error),
}

impl fmt::Display for PersFdError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PersFdError::ReadErr(e) => write!(f, "Read error: {e}"),
            PersFdError::WriteErr(e) => write!(f, "Write error: {e}"),
        }
    }
}

pub struct PersFd {
    file: File,
    //path: String,
}

impl PersFd {
    pub fn new(path: &str, write: bool) -> Result<Self, PersFdError> {
        let file = OpenOptions::new()
            .read(true)
            .write(write)
            .open(path)
            .map_err(PersFdError::ReadErr)?;

        Ok(PersFd {
            file,
            //path: path.to_string(),
        })
    }

    pub fn read_value(&mut self) -> Result<String, PersFdError> {
        self.file
            .seek(SeekFrom::Start(0))
            .map_err(PersFdError::ReadErr)?;
        let mut contents = String::new();
        self.file
            .read_to_string(&mut contents)
            .map_err(PersFdError::ReadErr)?;
        Ok(contents.trim().to_string())
    }

    pub fn set_value(&mut self, value: &str) -> Result<(), PersFdError> {
        self.file
            .seek(io::SeekFrom::Start(0))
            .map_err(PersFdError::WriteErr)?;
        self.file.set_len(0).map_err(PersFdError::WriteErr)?;
        self.file
            .write_all(format!("{}\n", value).as_bytes())
            .map_err(PersFdError::WriteErr)?;
        self.file.flush().map_err(PersFdError::WriteErr)
    }
}

#[allow(dead_code)]
pub trait StyledString {
    fn black(&self) -> String;
    fn red(&self) -> String;
    fn green(&self) -> String;
    fn yellow(&self) -> String;
    fn blue(&self) -> String;
    fn magenta(&self) -> String;
    fn cyan(&self) -> String;
    fn white(&self) -> String;
    fn bold(&self) -> String;
    fn underline(&self) -> String;
    fn normal(&self) -> String;
    fn bright_red(&self) -> String;
}

impl StyledString for str {
    fn black(&self) -> String {
        format!("\x1b[30m{}\x1b[0m", self)
    }

    fn red(&self) -> String {
        format!("\x1b[31m{}\x1b[0m", self)
    }

    fn green(&self) -> String {
        format!("\x1b[32m{}\x1b[0m", self)
    }

    fn yellow(&self) -> String {
        format!("\x1b[33m{}\x1b[0m", self)
    }

    fn blue(&self) -> String {
        format!("\x1b[34m{}\x1b[0m", self)
    }

    fn magenta(&self) -> String {
        format!("\x1b[35m{}\x1b[0m", self)
    }

    fn cyan(&self) -> String {
        format!("\x1b[36m{}\x1b[0m", self)
    }

    fn white(&self) -> String {
        format!("\x1b[37m{}\x1b[0m", self)
    }

    fn bold(&self) -> String {
        format!("\x1b[1m{}\x1b[0m", self)
    }

    fn underline(&self) -> String {
        format!("\x1b[4m{}\x1b[0m", self)
    }

    fn normal(&self) -> String {
        format!("\x1b[0m{}\x1b[0m", self)
    }

    fn bright_red(&self) -> String {
        format!("\x1b[91m{}\x1b[0m", self)
    }
}
