use crate::utils::{PersFd, PersFdError};
use std::cell::RefCell;
use std::fmt;
use std::io;
use std::num;
use std::thread;
use std::time::Duration;

#[derive(Debug, PartialEq, Clone)]
pub enum CpuType {
    AMD,
    Intel,
    Unknown,
}

#[derive(PartialEq, Debug)]
pub enum ScalingGoverner {
    Performance,
    Powersave,
    Unknown,
}

impl ScalingGoverner {
    const PERFORMANCE: &str = "performance";
    const POWERSAVE: &str = "powersave";

    pub fn from_string(s: &str) -> Self {
        match s {
            ScalingGoverner::PERFORMANCE => Self::Performance,
            ScalingGoverner::POWERSAVE => Self::Powersave,
            _ => Self::Unknown,
        }
    }
}

#[derive(PartialEq, Debug)]
pub enum EPP {
    EDefault,
    Performance,
    BalancePerformance,
    BalancePower,
    Power,
    Unknown,
}

impl EPP {
    const DEFAULT: &str = "default";
    const PERFORMANCE: &str = "performance";
    const BALANCE_PERFORMANCE: &str = "balance_performance";
    const BALANCE_POWER: &str = "balance_power";
    const POWER: &str = "power";

    pub fn from_string(s: &str) -> Self {
        match s {
            EPP::DEFAULT => EPP::EDefault,
            EPP::PERFORMANCE => EPP::Performance,
            EPP::BALANCE_PERFORMANCE => EPP::BalancePerformance,
            EPP::BALANCE_POWER => EPP::BalancePower,
            EPP::POWER => EPP::Power,
            _ => EPP::Unknown,
        }
    }
}

#[derive(Debug)]
pub enum CpuStatesError {
    InvalidScalingGovVal,
    InvalidEPPVal,
    InvalidAMDPstate,
    UnsupportedCpuType,
    EmptyProcStat,
    InvalidProcStat,
    GenericError(String),
    PersFdErr(PersFdError),
    ParseIntErr(num::ParseIntError),
    GeneralIoErr(io::Error),
}

impl fmt::Display for CpuStatesError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CpuStatesError::InvalidScalingGovVal => write!(f, "Unsupported scaling governer value"),
            CpuStatesError::InvalidEPPVal => write!(f, "Unsupported epp value"),
            CpuStatesError::InvalidAMDPstate => write!(f, "AMD pstate probably isn't set to active"),
            CpuStatesError::UnsupportedCpuType => write!(f, "Detected and unsupported cpu type"),
            CpuStatesError::EmptyProcStat => write!(f, "Empty /proc/stat"),
            CpuStatesError::InvalidProcStat => write!(f, "Invalid /proc/stat"),
            CpuStatesError::GenericError(e) => write!(f, "Generic error: {e}"),
            CpuStatesError::PersFdErr(e) => write!(f, "{e}"),
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
    cpu_type: CpuType,

    scaling_governer: Vec<RefCell<PersFd>>,
    min_cpu_freq: Vec<RefCell<PersFd>>,
    max_cpu_freq: Vec<RefCell<PersFd>>,
    cpu_freq: Vec<RefCell<PersFd>>, // TODO: possibly wrong (not same as btop)
    cpu_temp: RefCell<PersFd>,
    cpu_load: RefCell<PersFd>, // TODO: possibly wrong
    cpu_boost: RefCell<PersFd>,
    epp: Vec<RefCell<PersFd>>,

    cpu_power_draw: Option<RefCell<PersFd>>, // TODO: possibly wrong
}

impl fmt::Display for CpuStates {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "CPU:
    cpu type: {:?}
    scaling governer: {:?}
    epp: {:?}
    cpu boost: {}
    min/max cpu freq: {:.2}-{:.2} GHz
    cpu freq: {:.2} GHz
    cpu temp: {}°C
    cpu load: {:.2}%
    cpu power draw: {:.2} W",
            self.cpu_type,
            self.read_scaling_governer()
                .unwrap_or(ScalingGoverner::Unknown),
            self.read_epp().unwrap_or(EPP::Unknown),
            self.read_cpu_boost().unwrap_or(false),
            self.read_min_cpu_freq().unwrap_or(0.0),
            self.read_max_cpu_freq().unwrap_or(0.0),
            self.read_avg_cpu_freq().unwrap_or(0.0),
            self.read_cpu_temp().unwrap_or(0),
            self.read_cpu_load().unwrap_or(0.0),
            self.read_cpu_power_draw().unwrap_or(0.0),
        )
    }
}

impl CpuStates {
    pub fn init(n: usize, cpu_type: &CpuType) -> Result<Self, CpuStatesError> {
        let mut available_asgr = PersFd::new("/sys/devices/system/cpu/cpu0/cpufreq/scaling_available_governors", false)?;
        let asgr = available_asgr.read_value()?;

        if !asgr.contains("performance") || !asgr.contains("powersave") {
            eprintln!("Incorrect available scaling governor options!");
            return Err(CpuStatesError::InvalidScalingGovVal);
        }

        let mut scaling_governer: Vec<RefCell<PersFd>> = vec![];
        let mut cpu_freq: Vec<RefCell<PersFd>> = vec![];
        let mut max_cpu_freq: Vec<RefCell<PersFd>> = vec![];
        let mut min_cpu_freq: Vec<RefCell<PersFd>> = vec![];
        let mut epp: Vec<RefCell<PersFd>> = vec![];
        for i in 0..n {
            let scaling_gov_path =
                format!("/sys/devices/system/cpu/cpu{}/cpufreq/scaling_governor", i);
            let cpu_freq_path =
                format!("/sys/devices/system/cpu/cpu{}/cpufreq/scaling_cur_freq", i);
            let min_cpu_freq_path =
                format!("/sys/devices/system/cpu/cpu{}/cpufreq/scaling_min_freq", i);
            let max_cpu_freq_path =
                format!("/sys/devices/system/cpu/cpu{}/cpufreq/scaling_max_freq", i);

            let amd_epp_path =
                format!("/sys/devices/system/cpu/cpu{}/cpufreq/energy_performance_preference", i);

            scaling_governer.push(RefCell::new(PersFd::new(&scaling_gov_path, true)?));
            cpu_freq.push(RefCell::new(PersFd::new(&cpu_freq_path, false)?));
            min_cpu_freq.push(RefCell::new(PersFd::new(&min_cpu_freq_path, true)?));
            max_cpu_freq.push(RefCell::new(PersFd::new(&max_cpu_freq_path, true)?));
            epp.push(RefCell::new(PersFd::new(&amd_epp_path, true)?));
        }

        let mut cpu_power_draw: Option<RefCell<PersFd>> = None;
        if *cpu_type == CpuType::AMD {
            let mut amd_pstate = PersFd::new("/sys/devices/system/cpu/amd_pstate/status", true)?;
            let r_amd_pstate = amd_pstate.read_value()?;

            if !r_amd_pstate.contains("active") {
                println!("amd_pstate is not active!");
                println!("Attempting to set amd_pstate to 'active'");

                if let Err(e) = amd_pstate.set_value("active") {
                    eprintln!("Failed setting amd_pstate to 'active': {:?}", e);
                    return Err(CpuStatesError::InvalidAMDPstate);
                }
            }

            cpu_power_draw = Some(RefCell::new(PersFd::new("/sys/class/powercap/intel-rapl:0/energy_uj", false)?));
        } else if *cpu_type == CpuType::Intel {
        } else {
            return Err(CpuStatesError::UnsupportedCpuType);
        }

        Ok(Self {
            cpu_core_count: n,
            cpu_type: cpu_type.clone(),

            scaling_governer,
            min_cpu_freq,
            max_cpu_freq,
            cpu_freq,
            cpu_temp: RefCell::new(PersFd::new("/sys/class/thermal/thermal_zone0/temp", false)?),
            cpu_load: RefCell::new(PersFd::new("/proc/stat", false)?),
            cpu_boost: RefCell::new(PersFd::new(
                "/sys/devices/system/cpu/cpufreq/boost",
                true,
            )?),
            epp,

            cpu_power_draw,
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
            ScalingGoverner::Powersave => ScalingGoverner::POWERSAVE,
            ScalingGoverner::Performance => ScalingGoverner::PERFORMANCE,
            _ => return Err(CpuStatesError::InvalidScalingGovVal),
        };

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
            EPP::EDefault => EPP::DEFAULT,
            EPP::Performance => EPP::PERFORMANCE,
            EPP::BalancePerformance => EPP::BALANCE_PERFORMANCE,
            EPP::BalancePower => EPP::BALANCE_POWER,
            EPP::Power => EPP::POWER,
            _ => return Err(CpuStatesError::InvalidEPPVal),
        };

        for fd in &self.epp {
            fd.borrow_mut().set_value(write)?;
        }

        Ok(())
    }

    pub fn read_cpu_boost(&self) -> Result<bool, CpuStatesError> {
        let val = self
            .cpu_boost
            .borrow_mut()
            .read_value()?
            .parse::<u8>()?;
        Ok(val == 1)
    }

    pub fn set_cpu_boost(&self, cpu_boost: bool) -> Result<(), CpuStatesError> {
        let val_str = (cpu_boost as u8).to_string();
        self.cpu_boost
            .borrow_mut()
            .set_value(&val_str)?;
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

    /// GHz
    pub fn read_max_cpu_freq(&self) -> Result<f32, CpuStatesError> {
        let prev: usize = self.max_cpu_freq[0].borrow_mut().read_value()?.parse()?;

        for fd in &self.max_cpu_freq[1..] {
            let val: usize = fd.borrow_mut().read_value()?.clone().parse()?;
            assert_eq!(prev, val, "max_cpu_freq is the same for all cpu cores");
        }

        Ok((prev as f32) / 1_000_000.0)
    }

    /// celcius
    pub fn read_cpu_temp(&self) -> Result<usize, CpuStatesError> {
        let temp: usize = self.cpu_temp.borrow_mut().read_value()?.parse()?;
        Ok(temp / 1000)
    }

    // TODO: a better way to do this?
    pub fn read_cpu_load(&self) -> Result<f64, CpuStatesError> {
        let proc_stat = self.cpu_load.borrow_mut().read_value()?;
        let line = proc_stat.lines().next().ok_or(CpuStatesError::EmptyProcStat)?;

        let prev: Vec<u64> = line.split_whitespace()
            .skip(1)
            .map(|s| s.parse::<u64>().map_err(|e| CpuStatesError::ParseIntErr(e)))
            .collect::<Result<Vec<_>, _>>()?;

        if prev.len() < 4 { return Err(CpuStatesError::InvalidProcStat); }
        if prev.len() > 10 { return Err(CpuStatesError::GenericError("to many fields".to_string())); }

        let prev_total: u64 = prev.iter().sum();
        let prev_idle = prev[3] + prev.get(4).unwrap_or(&0);

        thread::sleep(Duration::from_millis(200));

        let proc_stat2 = self.cpu_load.borrow_mut().read_value()?;
        let line2 = proc_stat2.lines().next().ok_or(CpuStatesError::EmptyProcStat)?;

        let now: Vec<u64> = line2.split_whitespace()
            .skip(1)
            .map(|s| s.parse::<u64>().map_err(|e| CpuStatesError::ParseIntErr(e)))
            .collect::<Result<Vec<_>, _>>()?;

        if now.len() < 4 { return Err(CpuStatesError::InvalidProcStat); }
        if now.len() > 10 { return Err(CpuStatesError::GenericError("to many fields".to_string())); }

        let now_total: u64 = now.iter().sum();
        let now_idle = now[3] + now.get(4).unwrap_or(&0);

        let total_delta = (now_total as i64 - prev_total as i64).max(1);
        let idle_delta = now_idle as i64 - prev_idle as i64;

        let busy_delta = (total_delta - idle_delta).max(0);
        let load_percent = (busy_delta as f64 / total_delta as f64) * 100.0;

        Ok(load_percent)
    }

    pub fn read_cpu_power_draw(&self) -> Result<f32, CpuStatesError> {
        if let Some(power_draw) = &self.cpu_power_draw {
            let start: u64 = power_draw.borrow_mut().read_value()?.trim().parse()?;
            let start_time = std::time::Instant::now();

            std::thread::sleep(std::time::Duration::from_millis(500));

            let end: u64 = power_draw.borrow_mut().read_value()?.trim().parse()?;
            let end_time = std::time::Instant::now();

            if end < start {
                return Ok(0.0);
            }

            let energy_delta_uj = end - start;
            let time_delta_ms = end_time.duration_since(start_time).as_secs_f32() * 1000.0;

            if time_delta_ms <= 0.0 {
                return Ok(0.0);
            }

            let watts = (energy_delta_uj as f32 / time_delta_ms) / 1000.0;

            Ok(watts)
        } else {
            Ok(0.0)
        }    }
}
