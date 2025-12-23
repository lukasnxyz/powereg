use crate::system_state::{SystemState, SystemStateError};
use std::{
    fmt, io,
    os::unix::io::AsRawFd,
    time::{Duration, Instant},
};
use udev::MonitorBuilder;

// TODO: more events:
//      low battery (< 20%)
//      high cpu temp
pub enum Event {
    PowerInPlug,
    PowerUnPlug,
    PeriodicCheck,
    Unknown,
    Error(String),
}

impl fmt::Display for Event {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Event::PowerInPlug => write!(f, "power plugged in"),
            Event::PowerUnPlug => write!(f, "power un plugged"),
            Event::Unknown => write!(f, "unknown event occured"),
            Event::Error(err) => write!(f, "an error occured: {}", err),
            Event::PeriodicCheck => write!(f, "periodic check"),
        }
    }
}

pub struct EventPoller {
    socket: udev::MonitorSocket,
    last_periodic_check: Instant,
    periodic_interval: Duration,
}

impl EventPoller {
    pub fn new() -> io::Result<Self> {
        let socket = MonitorBuilder::new()?
            .match_subsystem("power_supply")?
            .listen()?;

        Ok(Self {
            socket,
            last_periodic_check: Instant::now(),
            periodic_interval: Duration::from_secs(5),
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

pub fn handle_event(event: &Event, system_state: &SystemState) -> Result<(), SystemStateError> {
    match event {
        Event::PowerInPlug => {
            println!("event: {}", event);
            system_state.set_performance_mode()?;
        }
        Event::PowerUnPlug => {
            println!("event: {}", event);
            system_state.set_powersave_mode()?;
        }
        Event::PeriodicCheck => {
            //check_battery_level(system_fds)?;
            //check_cpu_temperature(system_fds)?;
        }
        Event::Unknown => {}
        Event::Error(_) => {}
    }

    //IncCPULoad,
    //DropCPULoad,

    //LowBattery,
    //FullBattery,

    // TODO: if printing fails, don't crash
    println!("{}", system_state.cpu_states);
    println!("{}", system_state.battery_states);

    Ok(())
}
