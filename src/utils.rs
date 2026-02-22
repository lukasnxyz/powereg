use crate::battery::ACPIType;
use crate::system_state::{SystemState, SystemStateError};
use serde::Deserialize;
use std::env;
use std::fmt;
use std::fs;
use std::fs::{File, OpenOptions};
use std::io::{self, Error, ErrorKind, Seek, SeekFrom, Write, prelude::*};
use std::path::Path;

#[derive(Debug)]
pub enum PersFdError {
  InvalidFilePerms,
  ReadErr(io::Error),
  WriteErr(io::Error),
}

impl fmt::Display for PersFdError {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    match self {
      PersFdError::InvalidFilePerms => write!(f, "Invalid file permissions"),
      PersFdError::ReadErr(e) => write!(f, "Read error: {e}"),
      PersFdError::WriteErr(e) => write!(f, "Write error: {e}"),
    }
  }
}

pub struct PersFd {
  file: File,
  write: bool,
  //path: String,
}

impl PersFd {
  pub fn new(path: &str, write: bool) -> Result<Self, PersFdError> {
    let file = OpenOptions::new()
      .read(true)
      .write(write)
      .open(path)
      .map_err(PersFdError::ReadErr)?;

    Ok(PersFd { file, write })
  }

  pub fn read_value(&mut self) -> Result<String, PersFdError> {
    self
      .file
      .seek(SeekFrom::Start(0))
      .map_err(PersFdError::ReadErr)?;
    let mut contents = String::new();
    self
      .file
      .read_to_string(&mut contents)
      .map_err(PersFdError::ReadErr)?;
    Ok(contents.trim().to_string())
  }

  pub fn set_value(&mut self, value: &str) -> Result<(), PersFdError> {
    if !self.write {
      return Err(PersFdError::InvalidFilePerms);
    }

    self
      .file
      .seek(io::SeekFrom::Start(0))
      .map_err(PersFdError::WriteErr)?;
    self.file.set_len(0).map_err(PersFdError::WriteErr)?;
    self
      .file
      .write_all(format!("{}\n", value).as_bytes())
      .map_err(PersFdError::WriteErr)?;
    self.file.flush().map_err(PersFdError::WriteErr)
  }
}

#[allow(dead_code)]
pub trait StyledString {
  fn red(&self) -> String;
  fn green(&self) -> String;
  fn yellow(&self) -> String;
}

impl StyledString for str {
  fn red(&self) -> String {
    format!("\x1b[31m{}\x1b[0m", self)
  }

  fn green(&self) -> String {
    format!("\x1b[32m{}\x1b[0m", self)
  }

  fn yellow(&self) -> String {
    format!("\x1b[33m{}\x1b[0m", self)
  }
}

#[derive(Deserialize)]
struct ConfigFile {
  battery: BatteryConfig,
}

#[derive(Deserialize)]
struct BatteryConfig {
  start_threshold: u8,
  stop_threshold: u8,
}

pub struct Config {
  pub charge_start_threshold: Option<u8>,
  pub charge_stop_threshold: Option<u8>,
}

impl fmt::Display for Config {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    write!(f, "SystemFds Read:")
  }
}

impl Config {
  pub fn parse(config_path: &str) -> Result<Self, Box<dyn std::error::Error>> {
    if !Path::new(config_path).exists() {
      return Err(Box::new(Error::new(
        ErrorKind::NotFound,
        "No config file found",
      )));
    }

    let contents = fs::read_to_string(config_path)?;
    let config_file: ConfigFile = toml::from_str(&contents)?;
    Ok(Self {
      charge_start_threshold: Some(config_file.battery.start_threshold),
      charge_stop_threshold: Some(config_file.battery.stop_threshold),
    })
  }

  pub fn apply(&self, system_state: &SystemState) -> Result<(), SystemStateError> {
    if system_state.acpi_type != ACPIType::ThinkPad {
      return Err(SystemStateError::ACPITypeErr(
        "only thinkpad acpi supported for now".to_string(),
      ));
    }

    if let Some(start_thresh) = self.charge_start_threshold {
      println!("Setting charge start threshold to {}", start_thresh);
      system_state
        .battery_states
        .set_charge_start_threshold(start_thresh.into())?;
    }

    if let Some(stop_thresh) = self.charge_stop_threshold {
      println!("Setting charge stop threshold to {}", stop_thresh);
      system_state
        .battery_states
        .set_charge_stop_threshold(stop_thresh.into())?;
    }

    Ok(())
  }

  pub fn get_config_path() -> Result<String, env::VarError> {
    if let Ok(sudo_user) = env::var("SUDO_USER") {
      Ok(format!("/home/{}/.config/powereg/config.toml", sudo_user))
    } else {
      let home = env::var("HOME")?;
      Ok(format!("{}/.config/powereg/config.toml", home))
    }
  }

  pub fn setup_config(system_state: &SystemState) {
    if let Ok(config_path) = Config::get_config_path() {
      println!("Config path: {config_path}");
      match Config::parse(&config_path) {
        Ok(config) => match config.apply(&system_state) {
          Ok(_) => {}
          Err(e) => println!("{} {}", "Error while applying config:".red(), e),
        },
        Err(e) => eprintln!("{} {}", "Error loading config:".red(), e),
      };
    } else {
      println!("{}", "Error loading config".red());
    }
  }
}
