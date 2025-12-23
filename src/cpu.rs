use crate::fds::{PersFd, PersFdError};
use crate::system_state::ChargingStatus;
use crate::system_state::{
    ScalingGoverner, BALANCE_PERFORMANCE, BALANCE_POWER, DEFAULT, EPP, PERFORMANCE, POWER,
    POWERSAVE,
};
use std::cell::RefCell;
use std::fmt;
use std::io;
use std::num;
use std::thread;
use std::time::Duration;

#[derive(Debug)]
pub enum CpuStatesError {
    PersFdErr(PersFdError),
    InvalidScalingGovVal,
    InvalidEPPVal,
    ParseIntErr(num::ParseIntError),
    GeneralIoErr(io::Error),
}

impl fmt::Display for CpuStatesError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CpuStatesError::PersFdErr(e) => write!(f, "{e}"),
            CpuStatesError::InvalidScalingGovVal => write!(f, "Unsupported scaling governer value"),
            CpuStatesError::InvalidEPPVal => write!(f, "Unsupported epp value"),
            CpuStatesError::ParseIntErr(e) => write!(f, "Failed parsing integer: {e}"),
            CpuStatesError::GeneralIoErr(e) => write!(f, "General io error: {e}"),
        }
    }
}

impl From<PersFdError> for CpuStatesError {
    fn from(error: PersFdError) -> Self {
        CpuStatesError::PersFdErr(error)
    }
}

impl From<num::ParseIntError> for CpuStatesError {
    fn from(error: num::ParseIntError) -> Self {
        CpuStatesError::ParseIntErr(error)
    }
}

impl From<io::Error> for CpuStatesError {
    fn from(error: io::Error) -> Self {
        CpuStatesError::GeneralIoErr(error)
    }
}

pub struct CpuStates {
    cpu_core_count: usize,
    scaling_governer: Vec<RefCell<PersFd>>,
    epp: Vec<RefCell<PersFd>>,
    min_cpu_freq: Vec<RefCell<PersFd>>,
    max_cpu_freq: Vec<RefCell<PersFd>>,
    cpu_freq: Vec<RefCell<PersFd>>,
    cpu_temp: RefCell<PersFd>,
    cpu_load: RefCell<PersFd>,
    cpu_power_draw: RefCell<PersFd>, // TODO: possibly wrong
                                     // TODO: /sys/devices/system/cpu/cpufreq/boost (0, 1)
}

impl fmt::Display for CpuStates {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "CpuStates
        scaling governer: {:?}
        epp: {:?}
        min/max cpu freq: {:.2}-{:.2} GHz
        cpu freq: {:.2} GHz
        cpu temp: {} C
        cpu load: {:?}%
        cpu power draw: {:.2} W",
            self.read_scaling_governer().unwrap(),
            self.read_epp().unwrap(),
            self.read_min_cpu_freq().unwrap(),
            self.read_max_cpu_freq().unwrap(),
            self.read_avg_cpu_freq().unwrap(),
            self.read_cpu_temp().unwrap(),
            self.read_cpu_load().unwrap(),
            self.read_cpu_power_draw().unwrap(),
        )
    }
}

impl CpuStates {
    pub fn init(n: usize) -> Result<Self, CpuStatesError> {
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
            cpu_power_draw: RefCell::new(PersFd::new(
                "/sys/class/powercap/intel-rapl:0/energy_uj",
                false,
            )?),
        })
    }

    pub fn read_scaling_governer(&self) -> Result<ScalingGoverner, CpuStatesError> {
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

    pub fn set_scaling_governer(
        &self,
        scaling_governer: ScalingGoverner,
    ) -> Result<(), CpuStatesError> {
        let write = match scaling_governer {
            ScalingGoverner::Powersave => POWERSAVE,
            ScalingGoverner::Performance => PERFORMANCE,
            _ => return Err(CpuStatesError::InvalidScalingGovVal),
        };

        println!("Setting cpu performance preference to: {}", write);

        for fd in &self.scaling_governer {
            fd.borrow_mut().set_value(write)?;
        }

        Ok(())
    }

    pub fn read_epp(&self) -> Result<EPP, CpuStatesError> {
        let gov = EPP::from_string(&self.epp[0].borrow_mut().read_value()?);
        assert_ne!(gov, EPP::Unknown, "EPP is not unknown");

        for fd in &self.epp[1..] {
            let val = EPP::from_string(&fd.borrow_mut().read_value()?);
            assert_eq!(gov, val, "EPP is the same for all cpu cores");
        }

        Ok(gov)
    }

    pub fn set_epp(&self, epp: EPP) -> Result<(), CpuStatesError> {
        let write = match epp {
            EPP::EDefault => DEFAULT,
            EPP::Performance => PERFORMANCE,
            EPP::BalancePerformance => BALANCE_PERFORMANCE,
            EPP::BalancePower => BALANCE_POWER,
            EPP::Power => POWER,
            _ => return Err(CpuStatesError::InvalidEPPVal),
        };

        println!("Setting CPU epp to: {}", write);

        for fd in &self.epp {
            fd.borrow_mut().set_value(write)?;
        }

        Ok(())
    }

    /// GHz
    pub fn read_avg_cpu_freq(&self) -> Result<f32, CpuStatesError> {
        let mut total: usize = 0;

        for fd in &self.cpu_freq {
            let val: String = fd.borrow_mut().read_value()?;
            total += val.parse::<usize>()?;
        }

        Ok(((total / self.cpu_core_count) as f32) / 1_000_000.0)
    }

    /// GHz
    pub fn read_min_cpu_freq(&self) -> Result<f32, CpuStatesError> {
        let prev: usize = self.min_cpu_freq[0].borrow_mut().read_value()?.parse()?;

        for fd in &self.min_cpu_freq[1..] {
            let val = fd.borrow_mut().read_value()?.clone().parse()?;
            assert_eq!(prev, val, "min_cpu_freq is the same for all cpu cores");
        }

        Ok((prev as f32) / 1_000_000.0)
    }

    //pub fn set_min_cpu_freq(&self) -> io::Result<usize> {}

    /// GHz
    pub fn read_max_cpu_freq(&self) -> Result<f32, CpuStatesError> {
        let prev: usize = self.max_cpu_freq[0].borrow_mut().read_value()?.parse()?;

        for fd in &self.max_cpu_freq[1..] {
            let val: usize = fd.borrow_mut().read_value()?.clone().parse()?;
            assert_eq!(prev, val, "max_cpu_freq is the same for all cpu cores");
        }

        Ok((prev as f32) / 1_000_000.0)
    }

    //pub fn set_max_cpu_freq(&mut self) -> io::Result<usize> {}

    /// celcius
    pub fn read_cpu_temp(&self) -> Result<usize, CpuStatesError> {
        let temp: usize = self.cpu_temp.borrow_mut().read_value()?.parse()?;
        Ok(temp / 1000)
    }

    pub fn read_cpu_load(&self) -> Result<f64, CpuStatesError> {
        let proc_stat = self.cpu_load.borrow_mut().read_value()?;
        let line = proc_stat
            .lines()
            .next()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "Empty /proc/stat"))?;

        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 5 {
            return Err(CpuStatesError::GeneralIoErr(io::Error::new(
                io::ErrorKind::InvalidData,
                "Invalid /proc/stat format",
            )));
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
            return Err(CpuStatesError::GeneralIoErr(io::Error::new(
                io::ErrorKind::InvalidData,
                "Invalid /proc/stat format",
            )));
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

    pub fn read_cpu_power_draw(&self) -> Result<f32, CpuStatesError> {
        let start: u64 = self.cpu_power_draw.borrow_mut().read_value()?.parse()?;

        std::thread::sleep(std::time::Duration::from_secs_f32(0.5));

        let end: u64 = self.cpu_power_draw.borrow_mut().read_value()?.parse()?;

        let watts = (end - start) as f32 / 1_000_000.0;
        Ok(watts)
    }
}
