use std::{fmt, io, os::unix::io::AsRawFd};

pub enum Event {
    PowerInPlug,
    PowerUnPlug,
    Unknown,
    Error(String),
    //LowBattery,
    //FullBattery,
    //DisplayOn,
    //DisplayOff,
}

impl fmt::Display for Event {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Event::PowerInPlug => write!(f, "power plugged in"),
            Event::PowerUnPlug => write!(f, "power un plugged"),
            Event::Unknown => write!(f, "unknown event occured"),
            Event::Error(err) => write!(f, "an error occured: {}", err),
        }
    }
}

pub fn poll_events(socket: &udev::MonitorSocket) -> Event {
    let mut fds = [libc::pollfd {
        fd: socket.as_raw_fd(),
        events: libc::POLLIN,
        revents: 0,
    }];
    let result = unsafe { libc::poll(fds.as_mut_ptr(), 1, -1) };
    if result < 0 {
        return Event::Error(io::Error::last_os_error().to_string());
    }

    for event in socket.iter() {
        if event.event_type() == udev::EventType::Change {
            if let Some(name) = event.property_value("POWER_SUPPLY_NAME") {
                let name_str = name.to_str().unwrap_or("");

                if name_str == "ACAD" || name_str == "AC" || name_str == "ADP1" || name_str == "AC0"
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

pub fn handle_event(event: &Event) {
    match event {
        Event::PowerInPlug => {
            println!("event: {}", event);
        }
        Event::PowerUnPlug => {
            println!("event: {}", event);
        }
        Event::Unknown => {}
        Event::Error(_) => {}
    }
}
