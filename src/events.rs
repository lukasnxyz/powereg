use crate::{
    battery::ChargingStatus,
    system_state::{State, SystemState, SystemStateError},
};
use std::{
    fmt, io,
    os::unix::io::AsRawFd,
    time::{Duration, Instant},
};
use udev::MonitorBuilder;

#[derive(Clone)]
pub enum Event {
    PowerInPlug,
    PowerUnPlug,

    PeriodicCheck,
    LowBattery,
    HighCpuLoad,
    LowCpuLoad,

    Unknown,
    Error(String),
}

impl Event {
    const HIGH_CPU_LOAD: f64 = 35.0;
    const LOW_CPU_LOAD: f64 = 30.0;

    fn state_transition(self: &Event, system_state: &SystemState) {
        let old_state = *system_state.state.borrow();

        let new_state = match old_state {
            State::Performance => match self {
                Event::PowerInPlug => State::Performance,
                Event::PowerUnPlug | Event::LowBattery => State::Powersave,
                Event::HighCpuLoad | Event::LowCpuLoad => State::Performance,
                _ => old_state,
            },
            State::Balanced => match self {
                Event::PowerInPlug => State::Performance,
                Event::PowerUnPlug | Event::LowBattery => State::Powersave,
                Event::HighCpuLoad | Event::LowCpuLoad => State::Performance,
                _ => old_state,
            },
            State::Powersave => match self {
                Event::PowerInPlug => State::Performance,
                Event::PowerUnPlug | Event::LowBattery => State::Powersave,
                _ => old_state,
            },
        };

        *system_state.state.borrow_mut() = new_state;
    }

    fn periodic_check(system_state: &SystemState, cpu_load: f64) -> Result<Event, SystemStateError> {
        let low_battery = system_state.battery_states.read_battery_capacity()? <= 20;
        if low_battery {
            return Ok(Event::LowBattery);
        }

        let charging_status = system_state.battery_states.read_charging_status()?;
        let discharging = charging_status == ChargingStatus::DisCharging;

        let boost = system_state.cpu_states.read_cpu_boost()?;
        let high_cpu_load = cpu_load >= Event::HIGH_CPU_LOAD;
        let low_cpu_load = cpu_load < Event::LOW_CPU_LOAD;

        if high_cpu_load && !discharging && !boost {
            return Ok(Event::HighCpuLoad);
        } else if low_cpu_load && !discharging && boost {
            return Ok(Event::LowCpuLoad);
        }

        if discharging {
            Ok(Event::PowerUnPlug)
        } else {
            Ok(Event::PowerInPlug)
        }
    }

    pub fn handle_event(self: &Event, system_state: &SystemState) -> Result<(), SystemStateError> {
        let cpu_load = system_state.cpu_states.read_cpu_load()?;

        let event = Self::periodic_check(system_state, cpu_load).unwrap_or(self.clone());

        let old_state = *system_state.state.borrow();
        event.state_transition(system_state);
        let new_state = *system_state.state.borrow();

        // in its own branch because cpu boost may change depending on cpu load
        if new_state == State::Performance {
            system_state.set_performance_mode(cpu_load >= Event::HIGH_CPU_LOAD)?;
            return Ok(());
        }

        if old_state != new_state {
            match new_state {
                State::Powersave => system_state.set_powersave_mode()?,
                State::Balanced => system_state.set_balanced_mode()?,
                State::Performance => unreachable!(), // Handled by branch above
            }
        }

        Ok(())
    }
}

impl fmt::Display for Event {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Event::PowerInPlug => write!(f, "power plugged in"),
            Event::PowerUnPlug => write!(f, "power un plugged"),

            Event::PeriodicCheck => write!(f, "periodic check"),

            Event::LowBattery => write!(f, "low battery"),
            Event::LowCpuLoad => write!(f, "low cpu load"),
            Event::HighCpuLoad => write!(f, "high cpu load"),

            Event::Unknown => write!(f, "unknown event occured"),
            Event::Error(err) => write!(f, "an error occured: {}", err),
        }
    }
}

pub struct EventPoller {
    socket: udev::MonitorSocket,
    last_periodic_check: Instant,
    periodic_interval: Duration,
}

impl EventPoller {
    pub fn new(interval_duration_s: u8) -> io::Result<Self> {
        let socket = MonitorBuilder::new()?
            .match_subsystem("power_supply")?
            .listen()?;

        Ok(Self {
            socket,
            last_periodic_check: Instant::now(),
            periodic_interval: Duration::from_secs(interval_duration_s.into()),
        })
    }

    pub fn poll_events(&mut self) -> Event {
        let elapsed = self.last_periodic_check.elapsed();
        let timeout_ms = if elapsed >= self.periodic_interval {
            0
        } else {
            (self.periodic_interval - elapsed).as_millis() as i32
        };

        let mut fds = [libc::pollfd {
            fd: self.socket.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        }];

        let result = unsafe { libc::poll(fds.as_mut_ptr(), 1, timeout_ms) };

        if result < 0 {
            return Event::Error(io::Error::last_os_error().to_string());
        }

        if self.last_periodic_check.elapsed() >= self.periodic_interval {
            self.last_periodic_check = Instant::now();
            return Event::PeriodicCheck;
        }

        for event in self.socket.iter() {
            if event.event_type() == udev::EventType::Change {
                if let Some(name) = event.property_value("POWER_SUPPLY_NAME") {
                    let name_str = name.to_str().unwrap_or("");

                    if name_str == "ACAD"
                        || name_str == "AC"
                        || name_str == "ADP1"
                        || name_str == "AC0"
                    {
                        if let Some(online) = event.property_value("POWER_SUPPLY_ONLINE") {
                            let online_str = online.to_str().unwrap_or("");

                            match online_str {
                                "1" => return Event::PowerInPlug,
                                "0" => return Event::PowerUnPlug,
                                _ => return Event::Unknown,
                            }
                        }
                    }
                }
            }
        }

        Event::Unknown
    }
}
