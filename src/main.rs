use crate::{
    events::{handle_event, EventPoller},
    fds::SystemFds,
    setup::{check_running_daemon_mode, install_daemon, uninstall_daemon},
    system_state::{set_performance_mode, set_powersave_mode, ChargingStatus, Config, SystemState},
    tui::run_tui,
};
use clap::Parser;
use std::{
    io,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread,
};

mod events;
mod fds;
mod setup;
mod system_state;
mod tui;

const CONFIG_PATH: &str = "~/.config/powereg/config.toml";

#[derive(Parser, Debug)]
#[command(version, about)]
#[group(id = "mode", required = true, multiple = false)]
struct Args {
    #[arg(long, help = "Run in live mode")]
    pub live: bool,
    #[arg(long, help = "Monitor running daemon and system stats")]
    pub monitor: bool,
    #[arg(long, help = "Run powereg daemon mode")]
    pub daemon: bool,
    #[arg(long, help = "Install powereg as a daemon on your system")]
    pub install: bool,
    #[arg(long, help = "Uninstall powereg on your system")]
    pub uninstall: bool,
}

fn main() -> io::Result<()> {
    let args = Args::parse();

    if !unsafe { libc::geteuid() == 0 } {
        println!("Need to run with root privileges!");
        return Ok(());
    }

    let system_state = SystemState::init();
    if !system_state.linux {
        println!("Need to be running on Linux!");
        return Ok(());
    }
    println!("{}", system_state);

    let mut system_fds = SystemFds::init(system_state.num_cpu_cores)?;
    match system_fds.read_battery_charging_status()? {
        ChargingStatus::Charging => set_performance_mode(&system_fds)?,
        ChargingStatus::DisCharging => set_powersave_mode(&system_fds)?,
        ChargingStatus::NotCharging => set_performance_mode(&system_fds)?,
        ChargingStatus::Unknown => set_powersave_mode(&system_fds)?,
    }

    match Config::parse(CONFIG_PATH) {
        Ok(config) => config.apply(&system_fds, &system_state)?,
        Err(e) => eprintln!("Error loading config: {}", e),
    };

    if args.live {
        if check_running_daemon_mode()? {
            println!("powereg already running in daemon mode!");
            println!("\tuse powereg --monitor");
            return Ok(());
        }

        let stop_signal = Arc::new(AtomicBool::new(false));
        let r = stop_signal.clone();
        let event_handle = thread::spawn(move || -> io::Result<()> {
            let n_system_fds = SystemFds::init(system_state.num_cpu_cores)?;

            let mut poller = EventPoller::new()?;
            while !r.load(Ordering::Relaxed) {
                let event = poller.poll_events();
                handle_event(&event, &n_system_fds)?;
            }

            Ok(())
        });

        let terminal = ratatui::init();
        let _ = run_tui(terminal, &system_fds);

        stop_signal.store(true, Ordering::Relaxed);
        let _ = event_handle.join();
    } else if args.monitor {
        //if !check_running_daemon_mode()? {
        //    println!("start powereg daemon mode with sudo powereg --daemon!");
        //    return Ok(());
        //}

        let terminal = ratatui::init();
        let _ = run_tui(terminal, &system_fds);
    } else if args.daemon {
        let mut poller = EventPoller::new()?;
        loop {
            let event = poller.poll_events();
            handle_event(&event, &mut system_fds)?;
        }
    } else if args.install {
        install_daemon()?;
    } else if args.uninstall {
        uninstall_daemon()?;
    }

    Ok(())
}
