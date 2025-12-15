use crate::system_state::{ChargingStatus, ScalingGoverner, PERFORMANCE, POWERSAVE};
use std::{
    cell::RefCell,
    fmt,
    fs::{File, OpenOptions},
    io::{self, prelude::*, ErrorKind, Seek, SeekFrom, Write},
};

pub struct PersFd {
    file: File,
    path: String,
}

impl PersFd {
    pub fn new(path: &str, write: bool) -> io::Result<Self> {
        let file = OpenOptions::new().read(true).write(write).open(path)?;
        Ok(PersFd {
            file,
            path: path.to_string(),
        })
    }

    pub fn read_value(&mut self) -> io::Result<String> {
        self.file.seek(SeekFrom::Start(0))?;
        let mut contents = String::new();
        self.file.read_to_string(&mut contents)?;
        Ok(contents.trim().to_string())
    }

    pub fn set_value(&mut self, value: &str) -> io::Result<()> {
        self.file.seek(io::SeekFrom::Start(0))?;
        self.file.set_len(0)?;
        self.file.write_all(format!("{}\n", value).as_bytes())?;
        self.file.flush()?;
        Ok(())
    }
}

// SystemFdErr
//  io::Error
//  parsing error

/*
/sys/devices/system/cpu/cpu* /cpufreq/scaling_governor
/sys/devices/system/cpu/cpu* /cpufreq/scaling_min_freq
/sys/devices/system/cpu/cpu* /cpufreq/scaling_max_freq
/sys/devices/system/cpu/cpu* /cpufreq/energy_performance_preference
/sys/devices/system/cpu/intel_pstate/no_turbo
/sys/devices/system/cpu/cpufreq/boost
/sys/firmware/acpi/platform_profile
*/

pub struct SystemFds {
    cpu_core_count: usize,

    scaling_governer: Vec<RefCell<PersFd>>,
    min_cpu_freq: Vec<RefCell<PersFd>>,
    max_cpu_freq: Vec<RefCell<PersFd>>,
    cpu_freq: Vec<RefCell<PersFd>>,
    cpu_temp: RefCell<PersFd>,
    battery_charging_status: RefCell<PersFd>,
    battery_capacity: RefCell<PersFd>,
    load_avg: RefCell<PersFd>,
    // TODO: /sys/devices/system/cpu/cpufreq/boost (0, 1)
    // TODO: /sys/devices/system/cpu/cpu*/cpufreq/energy_performance_preference (power, balance_power, balance_performance, performance)
    //      intel_pstate and amd_pstate
    // TODO: /sys/firmware/acpi/platform_profile (low-power, balanced, performance)
    // battery thresholds on thinkpad
}

impl fmt::Display for SystemFds {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "SystemFds Read:
        scaling governer: {:?}
        min/max cpu freq: {:.2}-{:.2} GHz
        cpu freq: {:.2} GHz
        cpu temp: {}
        load avg: {:?}
        charging status: {:?}
        battery capacity: {}",
            self.read_scaling_governer().unwrap(),
            self.read_min_cpu_freq().unwrap(),
            self.read_max_cpu_freq().unwrap(),
            self.read_avg_cpu_freq().unwrap(),
            self.read_cpu_temp().unwrap(),
            self.read_load_avg().unwrap(),
            self.read_battery_charging_status().unwrap(),
            self.read_battery_capacity().unwrap(),
        )
    }
}

impl SystemFds {
    pub fn init(n: usize) -> io::Result<Self> {
        let mut available_scaling_governers = PersFd::new(
            "/sys/devices/system/cpu/cpu0/cpufreq/scaling_available_governors",
            false,
        )?;
        assert_eq!(
            "performance powersave",
            available_scaling_governers.read_value()?
        );

        let mut scaling_governer: Vec<RefCell<PersFd>> = vec![];
        let mut cpu_freq: Vec<RefCell<PersFd>> = vec![];
        let mut max_cpu_freq: Vec<RefCell<PersFd>> = vec![];
        let mut min_cpu_freq: Vec<RefCell<PersFd>> = vec![];
        for i in 0..n {
            let scaling_gov_path =
                format!("/sys/devices/system/cpu/cpu{}/cpufreq/scaling_governor", i);
            let cpu_freq_path =
                format!("/sys/devices/system/cpu/cpu{}/cpufreq/scaling_cur_freq", i);
            let min_cpu_freq_path =
                format!("/sys/devices/system/cpu/cpu{}/cpufreq/scaling_min_freq", i);
            let max_cpu_freq_path =
                format!("/sys/devices/system/cpu/cpu{}/cpufreq/scaling_max_freq", i);

            scaling_governer.push(RefCell::new(PersFd::new(&scaling_gov_path, true)?));
            cpu_freq.push(RefCell::new(PersFd::new(&cpu_freq_path, false)?));
            min_cpu_freq.push(RefCell::new(PersFd::new(&min_cpu_freq_path, true)?));
            max_cpu_freq.push(RefCell::new(PersFd::new(&max_cpu_freq_path, true)?));
        }

        Ok(Self {
            cpu_core_count: n,
            scaling_governer,
            cpu_freq,
            min_cpu_freq,
            max_cpu_freq,
            cpu_temp: RefCell::new(PersFd::new("/sys/class/thermal/thermal_zone0/temp", false)?),
            // TODO: do with /sys/class/power_supply/AC/online and check
            //   if name_str.starts_with("AC") || name_str.starts_with("ACAD") {
            battery_charging_status: RefCell::new(PersFd::new(
                "/sys/class/power_supply/BAT0/status",
                false,
            )?),
            battery_capacity: RefCell::new(PersFd::new(
                "/sys/class/power_supply/BAT0/capacity",
                false,
            )?),
            load_avg: RefCell::new(PersFd::new("/proc/loadavg", false)?),
        })
    }

    /*
    fn check_initial_ac_status() -> io::Result<bool> {
    use std::fs;
    use std::path::Path;

    let power_supply_path = Path::new("/sys/class/power_supply");

    if let Ok(entries) = fs::read_dir(power_supply_path) {
    for entry in entries.flatten() {
    let name = entry.file_name();
    let name_str = name.to_string_lossy();

    if name_str.starts_with("AC") || name_str.starts_with("ACAD") {
    let online_path = entry.path().join("online");
    if let Ok(content) = fs::read_to_string(online_path) {
    return Ok(content.trim() == "1");
    }
    }
    }
    }

    Ok(false)
    }
    */

    pub fn read_battery_charging_status(&self) -> io::Result<ChargingStatus> {
        Ok(ChargingStatus::from_string(
            &self.battery_charging_status.borrow_mut().read_value()?,
        ))
    }

    pub fn read_battery_capacity(&self) -> io::Result<usize> {
        Ok(self
            .battery_capacity
            .borrow_mut()
            .read_value()?
            .parse()
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "Failed to parse usize"))?)
    }

    pub fn read_scaling_governer(&self) -> io::Result<ScalingGoverner> {
        let gov =
            ScalingGoverner::from_string(&self.scaling_governer[0].borrow_mut().read_value()?);
        assert_ne!(gov, ScalingGoverner::Unknown);

        for fd in &self.scaling_governer[1..] {
            let val = ScalingGoverner::from_string(&fd.borrow_mut().read_value()?);
            assert_eq!(gov, val);
        }

        Ok(gov)
    }

    pub fn set_scaling_governer(&self, scaling_governer: ScalingGoverner) -> io::Result<()> {
        let write = match scaling_governer {
            ScalingGoverner::Powersave => POWERSAVE,
            ScalingGoverner::Performance => PERFORMANCE,
            _ => {
                return Err(io::Error::new(
                    ErrorKind::InvalidInput,
                    "unsupported performance preference value",
                ))
            }
        };

        println!("setting cpu performance preference to: {}", write);

        for fd in &self.scaling_governer {
            fd.borrow_mut().set_value(write)?;
        }

        Ok(())
    }

    /// GHz
    pub fn read_avg_cpu_freq(&self) -> io::Result<f32> {
        let mut total: usize = 0;

        for fd in &self.cpu_freq {
            let val: String = fd.borrow_mut().read_value()?;
            total += val
                .parse::<usize>()
                .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "Failed to parse usize"))?;
        }

        Ok(((total / self.cpu_core_count) as f32) / 1_000_000.0)
    }

    /// GHz
    pub fn read_min_cpu_freq(&self) -> io::Result<f32> {
        let prev: usize = self.min_cpu_freq[0]
            .borrow_mut()
            .read_value()?
            .parse()
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "Failed to parse usize"))?;

        for fd in &self.min_cpu_freq[1..] {
            let val =
                fd.borrow_mut().read_value()?.clone().parse().map_err(|_| {
                    io::Error::new(io::ErrorKind::InvalidData, "Failed to parse usize")
                })?;
            assert_eq!(prev, val);
        }

        Ok((prev as f32) / 1_000_000.0)
    }

    //pub fn set_min_cpu_freq(&self) -> io::Result<usize> {}

    /// GHz
    pub fn read_max_cpu_freq(&self) -> io::Result<f32> {
        let prev: usize = self.max_cpu_freq[0]
            .borrow_mut()
            .read_value()?
            .parse()
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "Failed to parse usize"))?;

        for fd in &self.max_cpu_freq[1..] {
            let val: usize =
                fd.borrow_mut().read_value()?.clone().parse().map_err(|_| {
                    io::Error::new(io::ErrorKind::InvalidData, "Failed to parse usize")
                })?;
            assert_eq!(prev, val);
        }

        Ok((prev as f32) / 1_000_000.0)
    }

    //pub fn set_max_cpu_freq(&mut self) -> io::Result<usize> {}

    /// celcius
    pub fn read_cpu_temp(&self) -> io::Result<usize> {
        let temp: usize = self
            .cpu_temp
            .borrow_mut()
            .read_value()?
            .parse()
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "Failed to parse usize"))?;
        Ok(temp / 1000)
    }

    /// 1min, 5min, 15min
    pub fn read_load_avg(&self) -> io::Result<(f32, f32, f32)> {
        let contents = self.load_avg.borrow_mut().read_value()?;
        let parts: Vec<&str> = contents.split_whitespace().collect();
        let load_1min: f32 = parts[0]
            .parse()
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "Failed to parse f32"))?;
        let load_5min: f32 = parts[1]
            .parse()
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "Failed to parse f32"))?;
        let load_15min: f32 = parts[2]
            .parse()
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "Failed to parse f32"))?;

        Ok((load_1min, load_5min, load_15min))
    }
}
