use crate::system_state::{
    ChargingStatus, ScalingGoverner, BALANCE_PERFORMANCE, BALANCE_POWER, DEFAULT, EPP, PERFORMANCE,
    POWER, POWERSAVE,
};
use std::{
    cell::RefCell,
    fmt,
    fs::{File, OpenOptions},
    io::{self, prelude::*, ErrorKind, Seek, SeekFrom, Write},
    thread,
    time::Duration,
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

pub struct SystemFds {
    cpu_core_count: usize,
    scaling_governer: Vec<RefCell<PersFd>>,
    epp: Vec<RefCell<PersFd>>,
    min_cpu_freq: Vec<RefCell<PersFd>>,
    max_cpu_freq: Vec<RefCell<PersFd>>,
    cpu_freq: Vec<RefCell<PersFd>>,
    cpu_temp: RefCell<PersFd>,
    cpu_load: RefCell<PersFd>,
    battery_charging_status: RefCell<PersFd>,
    battery_capacity: RefCell<PersFd>,
    charge_start_threshold: RefCell<PersFd>,
    charge_stop_threshold: RefCell<PersFd>,
    cpu_power_draw: RefCell<PersFd>, // TODO: possibly wrong
    total_power_draw: RefCell<PersFd>,
    // TODO: /sys/devices/system/cpu/cpufreq/boost (0, 1)
    // TODO: battery thresholds on thinkpad
}

impl fmt::Display for SystemFds {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "SystemFds Read:
        scaling governer: {:?}
        epp: {:?}
        min/max cpu freq: {:.2}-{:.2} GHz
        cpu freq: {:.2} GHz
        cpu temp: {} C
        cpu load: {:?}%
        cpu power draw: {:.2} W
        charging status: {:?}
        battery capacity: {}%
        charge start threshold: {}
        charge stop threshold: {}
        total power draw: {} W",
            self.read_scaling_governer().unwrap(),
            self.read_epp().unwrap(),
            self.read_min_cpu_freq().unwrap(),
            self.read_max_cpu_freq().unwrap(),
            self.read_avg_cpu_freq().unwrap(),
            self.read_cpu_temp().unwrap(),
            self.read_cpu_load().unwrap(),
            self.read_cpu_power_draw().unwrap(),
            self.read_battery_charging_status().unwrap(),
            self.read_battery_capacity().unwrap(),
            self.read_charge_start_threshold().unwrap(),
            self.read_charge_stop_threshold().unwrap(),
            self.read_total_power_draw().unwrap(),
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
            available_scaling_governers.read_value()?,
            "performance powersave",
            "correct options for scaling governers",
        );
        let mut available_epps = PersFd::new(
            "/sys/devices/system/cpu/cpu0/cpufreq/energy_performance_available_preferences",
            false,
        )?;

        let battery_charging_status =
            RefCell::new(PersFd::new("/sys/class/power_supply/BAT0/status", false)?);
        let c_status =
            ChargingStatus::from_string(&battery_charging_status.borrow_mut().read_value()?);
        if c_status == ChargingStatus::Charging || c_status == ChargingStatus::NotCharging {
            assert_eq!(
                available_epps.read_value()?,
                "performance",
                "correct options for epp",
            );
        } else {
            assert_eq!(
                available_epps.read_value()?,
                "default performance balance_performance balance_power power",
                "correct options for epp",
            );
        }

        let mut scaling_governer: Vec<RefCell<PersFd>> = vec![];
        let mut epp: Vec<RefCell<PersFd>> = vec![];
        let mut cpu_freq: Vec<RefCell<PersFd>> = vec![];
        let mut max_cpu_freq: Vec<RefCell<PersFd>> = vec![];
        let mut min_cpu_freq: Vec<RefCell<PersFd>> = vec![];
        for i in 0..n {
            let scaling_gov_path =
                format!("/sys/devices/system/cpu/cpu{}/cpufreq/scaling_governor", i);
            let epp_path = format!(
                "/sys/devices/system/cpu/cpu{}/cpufreq/energy_performance_preference",
                i
            );
            let cpu_freq_path =
                format!("/sys/devices/system/cpu/cpu{}/cpufreq/scaling_cur_freq", i);
            let min_cpu_freq_path =
                format!("/sys/devices/system/cpu/cpu{}/cpufreq/scaling_min_freq", i);
            let max_cpu_freq_path =
                format!("/sys/devices/system/cpu/cpu{}/cpufreq/scaling_max_freq", i);

            scaling_governer.push(RefCell::new(PersFd::new(&scaling_gov_path, true)?));
            epp.push(RefCell::new(PersFd::new(&epp_path, true)?));
            cpu_freq.push(RefCell::new(PersFd::new(&cpu_freq_path, false)?));
            min_cpu_freq.push(RefCell::new(PersFd::new(&min_cpu_freq_path, true)?));
            max_cpu_freq.push(RefCell::new(PersFd::new(&max_cpu_freq_path, true)?));
        }

        let mut amd_pstate_status =
            PersFd::new("/sys/devices/system/cpu/amd_pstate/status", false)?;
        assert_eq!(
            amd_pstate_status.read_value()?,
            "active",
            "amd_pstate is active"
        );

        Ok(Self {
            cpu_core_count: n,
            scaling_governer,
            epp,
            cpu_freq,
            min_cpu_freq,
            max_cpu_freq,
            cpu_temp: RefCell::new(PersFd::new("/sys/class/thermal/thermal_zone0/temp", false)?),
            cpu_load: RefCell::new(PersFd::new("/proc/stat", false)?),
            // TODO: do with /sys/class/power_supply/AC/online and check
            //   if name_str.starts_with("AC") || name_str.starts_with("ACAD") {
            battery_charging_status,
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
            cpu_power_draw: RefCell::new(PersFd::new(
                "/sys/class/powercap/intel-rapl:0/energy_uj",
                false,
            )?),
            total_power_draw: RefCell::new(PersFd::new(
                "/sys/class/power_supply/BAT0/power_now",
                false,
            )?),
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

    pub fn read_charge_start_threshold(&self) -> io::Result<usize> {
        Ok(self
            .charge_start_threshold
            .borrow_mut()
            .read_value()?
            .parse()
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "Failed to parse usize"))?)
    }

    pub fn set_charge_start_threshold(&self, start: usize) -> io::Result<()> {
        self.charge_start_threshold
            .borrow_mut()
            .set_value(&start.to_string())
    }

    pub fn read_charge_stop_threshold(&self) -> io::Result<usize> {
        Ok(self
            .charge_stop_threshold
            .borrow_mut()
            .read_value()?
            .parse()
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "Failed to parse usize"))?)
    }

    pub fn set_charge_stop_threshold(&self, stop: usize) -> io::Result<()> {
        self.charge_stop_threshold
            .borrow_mut()
            .set_value(&stop.to_string())
    }

    pub fn read_scaling_governer(&self) -> io::Result<ScalingGoverner> {
        let gov =
            ScalingGoverner::from_string(&self.scaling_governer[0].borrow_mut().read_value()?);
        assert_ne!(
            gov,
            ScalingGoverner::Unknown,
            "Scaling governer is not unknown"
        );

        for fd in &self.scaling_governer[1..] {
            let val = ScalingGoverner::from_string(&fd.borrow_mut().read_value()?);
            assert_eq!(gov, val, "Scaling governer is the same for all cpu cores");
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
                    "Unsupported scaling governer value",
                ))
            }
        };

        println!("Setting cpu performance preference to: {}", write);

        for fd in &self.scaling_governer {
            fd.borrow_mut().set_value(write)?;
        }

        Ok(())
    }

    pub fn read_epp(&self) -> io::Result<EPP> {
        let gov = EPP::from_string(&self.epp[0].borrow_mut().read_value()?);
        assert_ne!(gov, EPP::Unknown, "EPP is not unknown");

        for fd in &self.epp[1..] {
            let val = EPP::from_string(&fd.borrow_mut().read_value()?);
            assert_eq!(gov, val, "EPP is the same for all cpu cores");
        }

        Ok(gov)
    }

    pub fn set_epp(&self, epp: EPP) -> io::Result<()> {
        let write = match epp {
            EPP::EDefault => DEFAULT,
            EPP::Performance => PERFORMANCE,
            EPP::BalancePerformance => BALANCE_PERFORMANCE,
            EPP::BalancePower => BALANCE_POWER,
            EPP::Power => POWER,
            _ => {
                return Err(io::Error::new(
                    ErrorKind::InvalidInput,
                    "Unsupported epp value",
                ))
            }
        };

        println!("Setting CPU epp to: {}", write);

        for fd in &self.epp {
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
            assert_eq!(prev, val, "min_cpu_freq is the same for all cpu cores");
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
            assert_eq!(prev, val, "max_cpu_freq is the same for all cpu cores");
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

    pub fn read_cpu_load(&self) -> io::Result<f64> {
        let proc_stat = self.cpu_load.borrow_mut().read_value()?;
        let line = proc_stat
            .lines()
            .next()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "Empty /proc/stat"))?;

        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 5 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Invalid /proc/stat format",
            ));
        }

        let prev: Vec<u64> = parts[1..]
            .iter()
            .map(|s| {
                s.parse()
                    .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "Parse error"))
            })
            .collect::<io::Result<Vec<_>>>()?;
        let prev_total: u64 = prev.iter().sum();
        let prev_idle = prev[3] + if prev.len() > 4 { prev[4] } else { 0 };

        thread::sleep(Duration::from_millis(250));

        let proc_stat = self.cpu_load.borrow_mut().read_value()?;
        let line = proc_stat
            .lines()
            .next()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "Empty /proc/stat"))?;

        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 5 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Invalid /proc/stat format",
            ));
        }

        let now: Vec<u64> = parts[1..]
            .iter()
            .map(|s| {
                s.parse()
                    .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "Parse error"))
            })
            .collect::<io::Result<Vec<_>>>()?;
        let now_total: u64 = now.iter().sum();
        let now_idle = now[3] + if now.len() > 4 { now[4] } else { 0 };

        let total_delta = (now_total as i64 - prev_total as i64).max(1) as u64;
        let idle_delta = now_idle as i64 - prev_idle as i64;

        let load_percent = if total_delta > 0 {
            let busy_delta = total_delta as i64 - idle_delta;
            (busy_delta.max(0) as f64 / total_delta as f64) * 100.0
        } else {
            0.0
        };

        Ok(load_percent)
    }

    pub fn read_cpu_power_draw(&self) -> io::Result<f32> {
        let start: u64 = self
            .cpu_power_draw
            .borrow_mut()
            .read_value()?
            .parse()
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "Failed to parse u64"))?;

        std::thread::sleep(std::time::Duration::from_secs_f32(0.5));

        let end: u64 = self
            .cpu_power_draw
            .borrow_mut()
            .read_value()?
            .parse()
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "Failed to parse u64"))?;

        let watts = (end - start) as f32 / 1_000_000.0;
        Ok(watts)
    }

    pub fn read_total_power_draw(&self) -> io::Result<f32> {
        let power_uw: u64 = self
            .total_power_draw
            .borrow_mut()
            .read_value()?
            .parse()
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "Failed to parse u64"))?;

        let watts = power_uw as f32 / 1_000_000.0;
        Ok(watts)
    }
}
