use std::{fs, path::Path, fmt};

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

enum ScalingGoverner {
    PowerSave,
    Performance,
}


pub struct SystemState {
    pub linux: bool,
    pub cpu_type: CpuType,
    pub acpi_type: ACPIType,
    pub scaling_governer: ScalingGoverner,
    pub scaling_min_freq: usize,
    pub scaling_max_freq: usize,
}

impl fmt::Display for SystemState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "system state:\n\trunning linux: {}\n\tcpu type: {:?}\n\tacpi type: {:?}",
            self.linux,
            self.cpu_type,
            self.acpi_type,
        )
    }
}

impl SystemState {
    pub fn init() -> Self {
        Self {
            linux: Self::detect_linux(),
            cpu_type: Self::detect_cpu_type(),
            acpi_type: Self::detect_acpi_type(),
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

        let has_os_release = Path::new("/etc/os-release").exists()
            || Path::new("/usr/lib/os-release").exists();

        compile_time || (runtime_uname && has_proc && has_sys) ||
            (has_proc && has_sys && has_etc && has_os_release)
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

    fn detect_acpi_type() -> ACPIType {
        return ACPIType::ThinkPad;
    }
}
