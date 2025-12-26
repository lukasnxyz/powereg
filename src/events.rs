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

pub enum Event {
    PowerInPlug,
    PowerUnPlug,

    PeriodicCheck,

    LowBattery,
    HighCpuTemp,
    HighCpuLoad,
    LoadNormalized,

    Unknown,
    Error(String),
}

impl fmt::Display for Event {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Event::PowerInPlug => write!(f, "power plugged in"),
            Event::PowerUnPlug => write!(f, "power un plugged"),

            Event::PeriodicCheck => write!(f, "periodic check"),

            Event::LowBattery => write!(f, "low battery"),
            Event::HighCpuTemp => write!(f, "high cpu temp"),
            Event::HighCpuLoad => write!(f, "high cpu load"),
            Event::LoadNormalized => write!(f, "load normalized"),

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

    fn state_transition(event: &Event, system_state: &SystemState) {
        let old_state = system_state.state.clone().into_inner();
        *system_state.state.borrow_mut() = match (old_state, event) {
            (_, Event::PowerInPlug) => State::Performance,
            (_, Event::PowerUnPlug) => State::Powersave,

            (_, Event::LowBattery) => State::Powersave,

            (State::Performance, Event::HighCpuTemp) => State::Balanced,
            (State::Performance, Event::HighCpuLoad) => State::Balanced,

            (State::Balanced, Event::LoadNormalized) => State::Performance,

            _ => old_state,
        };
    }

    fn periodic_check(system_state: &SystemState) -> Result<Event, SystemStateError> {
        let low_battery_level = system_state.battery_states.read_battery_capacity()? <= 25;
        let high_cpu_temp = system_state.cpu_states.read_cpu_temp()? >= 85;
        let high_cpu_load = system_state.cpu_states.read_cpu_load()? >= 85.0;
        let is_plugged_in =
            system_state.battery_states.read_charging_status()? == ChargingStatus::Charging;

        let current_state = *system_state.state.borrow();

        let event = if low_battery_level {
            Event::LowBattery
        } else if !is_plugged_in
            && (current_state == State::Performance || current_state == State::Balanced)
        {
            Event::PowerUnPlug
        } else if is_plugged_in && current_state == State::Powersave {
            Event::PowerInPlug
        } else if high_cpu_temp || high_cpu_load {
            Event::HighCpuLoad
        } else if is_plugged_in && current_state == State::Balanced {
            Event::LoadNormalized
        } else {
            Event::Unknown
        };

        Ok(event)
    }

    pub fn handle_event(event: Event, system_state: &SystemState) -> Result<(), SystemStateError> {
        let mut event = event;
        match event {
            Event::PeriodicCheck => event = Self::periodic_check(&system_state)?,
            _ => {}
        }

        Self::state_transition(&event, &system_state);
        match *system_state.state.borrow() {
            State::Powersave => system_state.set_powersave_mode()?,
            State::Balanced => system_state.set_balanced_mode()?,
            State::Performance => system_state.set_performance_mode()?,
        }

        Ok(())
    }
}
