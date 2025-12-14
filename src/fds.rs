use std::fs::{File, OpenOptions};
use std::io::{self, prelude::*, ErrorKind, Seek, SeekFrom, Write};
//use std::os::unix::io::AsRawFd;

use crate::system_state::{ScalingGoverner, PERFORMANCE, POWERSAVE};

pub struct PersFd {
    file: File,
    path: String,
}

impl PersFd {
    /// Creates a new persistent setting by opening the specified file path.
    /// It requires CAP_SYS_ADMIN (root) permissions to write to most sysfs files.
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
    cpu_core_count: usize, // TODO: go this with generic N
    scaling_governer: Vec<PersFd>,
    max_cpu_freq: Vec<PersFd>,
    min_cpu_freq: Vec<PersFd>,
    cpu_freq: Vec<PersFd>,
    cpu_temp: PersFd,
    // TODO: /sys/devices/system/cpu/cpufreq/boost (0, 1)
    // TODO: /sys/devices/system/cpu/cpu*/cpufreq/energy_performance_preference (power, balance_power, balance_performance, performance)
    //      intel_pstate and amd_pstate
    // TODO: /sys/firmware/acpi/platform_profile (low-power, balanced, performance)
}

impl SystemFds {
    pub fn init(n: usize) -> io::Result<Self> {
        let mut available_scaling_governers =
            PersFd::new("/sys/devices/system/cpu/cpu0/cpufreq/scaling_available_governors", false)?;
        assert_eq!("performance powersave", available_scaling_governers.read_value()?);

        let mut scaling_governer: Vec<PersFd> = vec![];
        let mut cpu_freq: Vec<PersFd> = vec![];
        let mut max_cpu_freq: Vec<PersFd> = vec![];
        let mut min_cpu_freq: Vec<PersFd> = vec![];
        for i in 0..n {
            let scaling_gov_path =
                format!("/sys/devices/system/cpu/cpu{}/cpufreq/scaling_governor", i);
            let cpu_freq_path =
                format!("/sys/devices/system/cpu/cpu{}/cpufreq/scaling_cur_freq", i);
            let max_cpu_freq_path = format!("/sys/devices/system/cpu/cpu{}/cpufreq/scaling_min_freq", i);
            let min_cpu_freq_path = format!("/sys/devices/system/cpu/cpu{}/cpufreq/scaling_max_freq", i);

            scaling_governer.push(PersFd::new(&scaling_gov_path, true)?);
            cpu_freq.push(PersFd::new(&cpu_freq_path, false)?);
            max_cpu_freq.push(PersFd::new(&max_cpu_freq_path, true)?);
            min_cpu_freq.push(PersFd::new(&min_cpu_freq_path, true)?);
        }

        Ok(Self {
            cpu_core_count: n,
            scaling_governer,
            cpu_freq,
            max_cpu_freq,
            min_cpu_freq,
            cpu_temp: PersFd::new("/sys/class/thermal/thermal_zone0/temp", false)?
        })
    }

    pub fn read_scaling_governer(&mut self) -> io::Result<ScalingGoverner> {
        let gov = ScalingGoverner::from_string(&self.scaling_governer[0].read_value()?);
        assert_ne!(gov, ScalingGoverner::Unknown);

        for fd in &mut self.scaling_governer[1..] {
            let val = ScalingGoverner::from_string(&fd.read_value()?);
            assert_eq!(gov, val);
        }

        Ok(gov)
    }

    pub fn set_scaling_governer(&mut self, scaling_governer: ScalingGoverner) -> io::Result<()> {
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

        for fd in &mut self.scaling_governer.iter_mut() {
            fd.set_value(write)?;
        }

        Ok(())
    }

    pub fn read_avg_cpu_freq(&mut self) -> io::Result<usize> {
        let mut total = 0;

        for fd in &mut self.cpu_freq {
            let val: String = fd.read_value()?;
            total += val.parse::<usize>().expect("failed to parse integer");
        }

        Ok(total / self.cpu_core_count)
    }

    pub fn read_min_cpu_freq(&mut self) -> io::Result<usize> {
        let prev = &self.min_cpu_freq[0].read_value()?.parse::<usize>()
            .expect("failed to parse usize");

        for fd in &mut self.min_cpu_freq[1..] {
            let val = &fd.read_value()?.clone().parse::<usize>()
                .expect("failed to parse usize");
            assert_eq!(prev, val);
        }

        Ok(*prev)
    }

    //pub fn set_min_cpu_freq(&mut self) -> io::Result<usize> {}

    pub fn read_max_cpu_freq(&mut self) -> io::Result<usize> {
        let prev = &self.max_cpu_freq[0].read_value()?.parse::<usize>()
            .expect("failed to parse usize");

        for fd in &mut self.max_cpu_freq[1..] {
            let val = &fd.read_value()?.clone().parse::<usize>()
                .expect("failed to parse usize");
            assert_eq!(prev, val);
        }

        Ok(*prev)
    }

    //pub fn set_max_cpu_freq(&mut self) -> io::Result<usize> {}

    /// celcius
    pub fn read_cpu_temp(&mut self) -> io::Result<usize> {
        let temp = &self.cpu_temp.read_value()?.parse::<usize>()
            .expect("failed to parse usize") / 1000;
        Ok(temp)
    }
}
