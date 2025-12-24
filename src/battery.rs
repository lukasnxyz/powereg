use crate::fds::{PersFd, PersFdError};
use std::cell::RefCell;
use std::fmt;
use std::fs;
use std::num;
use std::path::Path;

const CHARGING: &str = "Charging";
const DISCHARGING: &str = "Discharging";
const NOTCHARGING: &str = "Not charging";

#[derive(PartialEq, Debug)]
pub enum ChargingStatus {
    Charging,
    DisCharging,
    NotCharging,
    Unknown,
}

impl ChargingStatus {
    pub fn from_string(s: &str) -> Self {
        match s {
            CHARGING => Self::Charging,
            DISCHARGING => Self::DisCharging,
            NOTCHARGING => Self::NotCharging,
            _ => Self::Unknown,
        }
    }
}

#[derive(Debug)]
pub enum BatteryStatesError {
    PersFdErr(PersFdError),
    ParseIntErr(num::ParseIntError),
}

impl fmt::Display for BatteryStatesError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BatteryStatesError::PersFdErr(e) => write!(f, "{e}"),
            BatteryStatesError::ParseIntErr(e) => write!(f, "Failed parsing integer: {e}"),
        }
    }
}

impl From<PersFdError> for BatteryStatesError {
    fn from(error: PersFdError) -> Self {
        BatteryStatesError::PersFdErr(error)
    }
}

impl From<num::ParseIntError> for BatteryStatesError {
    fn from(error: num::ParseIntError) -> Self {
        BatteryStatesError::ParseIntErr(error)
    }
}

pub struct BatteryStates {
    battery_charging_status: RefCell<PersFd>,
    battery_capacity: RefCell<PersFd>,
    charge_start_threshold: RefCell<PersFd>,
    charge_stop_threshold: RefCell<PersFd>,
    total_power_draw: RefCell<PersFd>,
}

impl fmt::Display for BatteryStates {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "BatteryStates:
        charging status: {:?}
        battery capacity: {}%
        charge start threshold: {}
        charge stop threshold: {}
        total power draw: {:.2} W",
            self.read_charging_status()
                .unwrap_or(ChargingStatus::Unknown),
            self.read_battery_capacity().unwrap_or(0),
            self.read_charge_start_threshold().unwrap_or(0),
            self.read_charge_stop_threshold().unwrap_or(0),
            self.read_total_power_draw().unwrap_or(0.0),
        )
    }
}

impl BatteryStates {
    pub fn init() -> Result<Self, BatteryStatesError> {
        Ok(Self {
            battery_charging_status: Self::load_charging_status()?,
            battery_capacity: RefCell::new(PersFd::new(
                "/sys/class/power_supply/BAT0/capacity",
                false,
            )?),
            charge_start_threshold: RefCell::new(PersFd::new(
                "/sys/class/power_supply/BAT0/charge_start_threshold",
                true,
            )?),
            charge_stop_threshold: RefCell::new(PersFd::new(
                "/sys/class/power_supply/BAT0/charge_stop_threshold",
                true,
            )?),
            total_power_draw: RefCell::new(PersFd::new(
                "/sys/class/power_supply/BAT0/power_now",
                false,
            )?),
        })
    }

    fn load_charging_status() -> Result<RefCell<PersFd>, BatteryStatesError> {
        let power_supply_path = Path::new("/sys/class/power_supply");
        if let Ok(entries) = fs::read_dir(power_supply_path) {
            for entry in entries.flatten() {
                let name = entry.file_name();
                let name_str = name.to_string_lossy();
                if name_str.starts_with("AC") || name_str.starts_with("ACAD") {
                    let online_path = entry.path().join("online");
                    if online_path.exists() {
                        return Ok(RefCell::new(PersFd::new(
                            online_path.to_str().unwrap(),
                            false,
                        )?));
                    }
                }
            }
        }

        Ok(RefCell::new(PersFd::new("", false)?))
    }

    pub fn read_charging_status(&self) -> Result<ChargingStatus, BatteryStatesError> {
        Ok(ChargingStatus::from_string(
            &self.battery_charging_status.borrow_mut().read_value()?,
        ))
    }

    pub fn read_battery_capacity(&self) -> Result<usize, BatteryStatesError> {
        Ok(self.battery_capacity.borrow_mut().read_value()?.parse()?)
    }

    pub fn read_charge_start_threshold(&self) -> Result<usize, BatteryStatesError> {
        Ok(self
            .charge_start_threshold
            .borrow_mut()
            .read_value()?
            .parse()?)
    }

    pub fn set_charge_start_threshold(&self, start: usize) -> Result<(), BatteryStatesError> {
        Ok(self
            .charge_start_threshold
            .borrow_mut()
            .set_value(&start.to_string())?)
    }

    pub fn read_charge_stop_threshold(&self) -> Result<usize, BatteryStatesError> {
        Ok(self
            .charge_stop_threshold
            .borrow_mut()
            .read_value()?
            .parse()?)
    }

    pub fn set_charge_stop_threshold(&self, stop: usize) -> Result<(), BatteryStatesError> {
        Ok(self
            .charge_stop_threshold
            .borrow_mut()
            .set_value(&stop.to_string())?)
    }

    pub fn read_total_power_draw(&self) -> Result<f32, BatteryStatesError> {
        let power_uw: u64 = self.total_power_draw.borrow_mut().read_value()?.parse()?;

        let watts = power_uw as f32 / 1_000_000.0;
        Ok(watts)
    }
}
