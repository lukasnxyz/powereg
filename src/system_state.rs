use crate::battery::{BatteryStates, BatteryStatesError, ChargingStatus};
use crate::cpu::{CpuStates, CpuStatesError, ScalingGoverner, EPP};
use serde::Deserialize;
use std::env;
use std::fmt;
use std::fs;
use std::io::{self, Error, ErrorKind};
use std::path::Path;

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
            system_state
                .battery_states
                .set_charge_start_threshold(start_thresh.into())?;
        }

        if let Some(stop_thresh) = self.charge_stop_threshold {
            system_state
                .battery_states
                .set_charge_stop_threshold(stop_thresh.into())?;
        }

        Ok(())
    }

    pub fn get_config_path() -> Result<String, env::VarError> {
        let home = env::var("HOME")?;
        //Ok(format!("{}/.config/powereg/config.toml", home))
        Ok("/home/ln/.config/powereg/config.toml".to_string())
    }
}

#[derive(Debug)]
enum CpuType {
    AMD,
    Intel,
    Unknown,
}

#[derive(Debug, PartialEq)]
enum ACPIType {
    ThinkPad,
    //IdeaPad,
    Unknown,
}

#[derive(Debug)]
pub enum SystemStateError {
    ACPITypeErr(String),
    CpuStatesErr(CpuStatesError),
    BatteryStatesErr(BatteryStatesError),
    GeneralIoErr(io::Error),
}

impl fmt::Display for SystemStateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SystemStateError::ACPITypeErr(e) => write!(f, "{e}"),
            SystemStateError::CpuStatesErr(e) => write!(f, "{e}"),
            SystemStateError::BatteryStatesErr(e) => write!(f, "{e}"),
            SystemStateError::GeneralIoErr(e) => write!(f, "General io error: {e}"),
        }
    }
}

impl From<CpuStatesError> for SystemStateError {
    fn from(error: CpuStatesError) -> Self {
        SystemStateError::CpuStatesErr(error)
    }
}

impl From<BatteryStatesError> for SystemStateError {
    fn from(error: BatteryStatesError) -> Self {
        SystemStateError::BatteryStatesErr(error)
    }
}

impl From<io::Error> for SystemStateError {
    fn from(error: io::Error) -> Self {
        SystemStateError::GeneralIoErr(error)
    }
}

pub struct SystemState {
    pub linux: bool,
    cpu_type: CpuType,
    acpi_type: ACPIType,
    pub num_cpu_cores: usize,

    pub cpu_states: CpuStates,
    pub battery_states: BatteryStates,
}

impl fmt::Display for SystemState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "system state:\n\trunning linux: {}\n\tcpu type: {:?}\n\tacpi type: {:?}\n\tcpu core count: {}",
            self.linux, self.cpu_type, self.acpi_type, self.num_cpu_cores,
        )
    }
}

impl SystemState {
    pub fn init() -> Result<Self, SystemStateError> {
        let num_cpu_cores = Self::num_cpu_cores()?;
        Ok(Self {
            linux: Self::detect_linux(),
            cpu_type: Self::detect_cpu_type(),
            acpi_type: Self::detect_acpi_type(),
            num_cpu_cores,
            cpu_states: CpuStates::init(num_cpu_cores)?,
            battery_states: BatteryStates::init()?,
        })
    }

    pub fn post_init(&self) -> Result<(), SystemStateError> {
        match self.battery_states.read_charging_status()? {
            ChargingStatus::Charging => self.set_performance_mode(),
            ChargingStatus::DisCharging => self.set_powersave_mode(),
            //ChargingStatus::NotCharging => self.set_performance_mode(),
            ChargingStatus::Unknown => self.set_powersave_mode(),
        }
    }

    pub fn set_powersave_mode(&self) -> Result<(), SystemStateError> {
        self.cpu_states
            .set_scaling_governer(ScalingGoverner::Powersave)?;
        self.cpu_states.set_epp(EPP::BalancePower)?;
        Ok(())
    }

    pub fn set_performance_mode(&self) -> Result<(), SystemStateError> {
        if self.battery_states.read_charging_status()? == ChargingStatus::DisCharging {
            return Ok(());
        }

        self.cpu_states
            .set_scaling_governer(ScalingGoverner::Performance)?;
        self.cpu_states.set_epp(EPP::Performance)?;

        Ok(())
    }

    fn detect_linux() -> bool {
        #[cfg(target_os = "linux")]
        let compile_time = true;
        #[cfg(not(target_os = "linux"))]
        let compile_time = false;

        let runtime_uname = std::process::Command::new("uname")
            .arg("-s")
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .map(|s| s.trim().eq_ignore_ascii_case("linux"))
            .unwrap_or(false);

        let has_proc = Path::new("/proc").exists();
        let has_sys = Path::new("/sys").exists();
        let has_etc = Path::new("/etc").exists();

        let has_os_release =
            Path::new("/etc/os-release").exists() || Path::new("/usr/lib/os-release").exists();

        compile_time
            || (runtime_uname && has_proc && has_sys)
            || (has_proc && has_sys && has_etc && has_os_release)
    }

    fn detect_cpu_type() -> CpuType {
        if let Ok(cpuinfo) = fs::read_to_string("/proc/cpuinfo") {
            for line in cpuinfo.lines() {
                if line.starts_with("vendor_id") {
                    if line.contains("GenuineIntel") {
                        return CpuType::Intel;
                    } else if line.contains("AuthenticAMD") {
                        return CpuType::AMD;
                    }
                }
            }
        }

        #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
        {
            if let Some(cpu_type) = Self::detect_cpu_via_cpuid() {
                return cpu_type;
            }
        }

        if let Ok(output) = std::process::Command::new("lscpu").output() {
            if let Ok(text) = String::from_utf8(output.stdout) {
                let lower = text.to_lowercase();
                if lower.contains("genuineintel") || lower.contains("intel") {
                    return CpuType::Intel;
                } else if lower.contains("authenticamd") || lower.contains("amd") {
                    return CpuType::AMD;
                }
            }
        }

        CpuType::Unknown
    }

    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    fn detect_cpu_via_cpuid() -> Option<CpuType> {
        #[cfg(target_arch = "x86")]
        use core::arch::x86::__cpuid;
        #[cfg(target_arch = "x86_64")]
        use core::arch::x86_64::__cpuid;

        unsafe {
            // CPUID leaf 0 returns vendor string in EBX, EDX, ECX
            let result = __cpuid(0);

            // convert registers to bytes
            let mut vendor = [0u8; 12];
            vendor[0..4].copy_from_slice(&result.ebx.to_le_bytes());
            vendor[4..8].copy_from_slice(&result.edx.to_le_bytes());
            vendor[8..12].copy_from_slice(&result.ecx.to_le_bytes());

            match &vendor {
                b"GenuineIntel" => Some(CpuType::Intel),
                b"AuthenticAMD" => Some(CpuType::AMD),
                _ => None,
            }
        }
    }

    fn num_cpu_cores() -> Result<usize, SystemStateError> {
        let cpu_dir = "/sys/devices/system/cpu/";
        let mut count = 0;

        for entry in fs::read_dir(cpu_dir)? {
            let entry = entry?;
            let path = entry.path();

            if path.is_dir() {
                if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                    if name.starts_with("cpu") && name != "cpufreq" {
                        // Check if the remaining part of the name is a number
                        if name[3..].parse::<u32>().is_ok() {
                            count += 1;
                        }
                    }
                }
            }
        }
        Ok(count)
    }

    fn detect_acpi_type() -> ACPIType {
        if let Ok(product_version) = fs::read_to_string("/sys/class/dmi/id/product_version") {
            let product_version = product_version.trim().to_lowercase();
            if product_version.contains("thinkpad") {
                return ACPIType::ThinkPad;
            }
            //if product_version.contains("ideapad") {
            //    return ACPIType::IdeaPad;
            //}
        }

        if let Ok(product_name) = fs::read_to_string("/sys/class/dmi/id/product_name") {
            let product_name = product_name.trim().to_lowercase();
            if product_name.contains("thinkpad") {
                return ACPIType::ThinkPad;
            }
            //if product_name.contains("ideapad") {
            //    return ACPIType::IdeaPad;
            //}
        }

        if Path::new("/proc/acpi/ibm").exists() {
            return ACPIType::ThinkPad;
        }

        ACPIType::Unknown
    }
}
