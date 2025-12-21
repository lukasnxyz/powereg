use crate::fds::SystemFds;
use serde::Deserialize;
use std::{
    fmt, fs,
    io::{self, Error, ErrorKind},
    path::Path,
};

pub const POWERSAVE: &str = "powersave";
pub const POWER: &str = "power";
pub const BALANCE_POWER: &str = "balance_power";
pub const PERFORMANCE: &str = "performance";
pub const BALANCE_PERFORMANCE: &str = "balance_performance";
pub const CHARGING: &str = "Charging";
pub const DISCHARGING: &str = "Discharging";
pub const NOTCHARGING: &str = "Not charging";
pub const DEFAULT: &str = "default";

#[derive(PartialEq, Debug)]
pub enum ScalingGoverner {
    Powersave,
    Performance,
    Unknown,
}

impl ScalingGoverner {
    pub fn from_string(s: &str) -> Self {
        match s {
            PERFORMANCE => Self::Performance,
            POWERSAVE => Self::Powersave,
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
    pub fn from_string(s: &str) -> Self {
        match s {
            DEFAULT => EPP::EDefault,
            PERFORMANCE => EPP::Performance,
            BALANCE_PERFORMANCE => EPP::BalancePerformance,
            BALANCE_POWER => EPP::BalancePower,
            POWER => EPP::Power,
            _ => EPP::Unknown,
        }
    }
}

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

    pub fn apply(&self, system_fds: &SystemFds) -> io::Result<()> {
        if let Some(start_thresh) = self.charge_start_threshold {
            system_fds.set_charge_start_threshold(start_thresh.into())?;
        }

        if let Some(stop_thresh) = self.charge_stop_threshold {
            system_fds.set_charge_stop_threshold(stop_thresh.into())?;
        }

        Ok(())
    }
}

#[derive(Debug)]
enum CpuType {
    AMD,
    Intel,
    Unknown,
}

#[derive(Debug)]
enum ACPIType {
    ThinkPad,
    IdeaPad,
    Unknown,
}

pub struct SystemState {
    pub linux: bool,
    cpu_type: CpuType,
    acpi_type: ACPIType,
    pub num_cpu_cores: usize,
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
    pub fn init() -> Self {
        Self {
            linux: Self::detect_linux(),
            cpu_type: Self::detect_cpu_type(),
            acpi_type: Self::detect_acpi_type(),
            num_cpu_cores: Self::num_cpu_cores().unwrap(),
        }
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

    fn num_cpu_cores() -> io::Result<usize> {
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
        return ACPIType::ThinkPad;
    }
}

pub fn set_powersave_mode(system_fds: &SystemFds) -> io::Result<()> {
    system_fds.set_scaling_governer(ScalingGoverner::Powersave)?;
    system_fds.set_epp(EPP::BalancePower)?;
    Ok(())
}

pub fn set_performance_mode(system_fds: &SystemFds) -> io::Result<()> {
    if system_fds.read_battery_charging_status()? == ChargingStatus::DisCharging {
        return Ok(());
    }

    system_fds.set_scaling_governer(ScalingGoverner::Performance)?;
    system_fds.set_epp(EPP::Performance)?;

    Ok(())
}
