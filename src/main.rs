use crate::{
    events::handle_event,
    fds::SystemFds,
    setup::{check_running_daemon_mode, install_daemon, uninstall_daemon},
    system_state::{set_performance_mode, set_powersave_mode, ChargingStatus, SystemState},
    tui::run_tui,
};
use clap::Parser;
use events::poll_events;
use std::io::{self, Write};
use udev::MonitorBuilder;

mod events;
mod fds;
mod setup;
mod system_state;
mod tui;

#[derive(Parser, Debug)]
#[command(version, about)]
struct Args {
    #[arg(long, help = "Install powereg as a daemon on your system")]
    pub install: bool,
    #[arg(long, help = "Uninstall powereg on your system")]
    pub uninstall: bool,
    #[arg(long, help = "Run in live mode")]
    pub live: bool,
    #[arg(long, help = "Monitor running daemon and system stats")]
    pub monitor: bool,
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

    // TODO: read config file on startup for battery thresholds

    //if args.monitor {
    //    // TODO: make sure daemon mode running
    //    println!("system monitor not implemented yet!");
    //    return Ok(());
    //}

    // TODO: more events:
    //      low battery (< 20%)
    //      high cpu temp
    let socket = MonitorBuilder::new()?
        .match_subsystem("power_supply")?
        .listen()?;

    let mut system_fds = SystemFds::init(system_state.num_cpu_cores)?;
    match system_fds.read_battery_charging_status()? {
        ChargingStatus::Charging => set_performance_mode(&system_fds)?,
        ChargingStatus::DisCharging => set_powersave_mode(&system_fds)?,
        ChargingStatus::NotCharging => set_performance_mode(&system_fds)?,
        ChargingStatus::Unknown => set_powersave_mode(&system_fds)?,
    }
    println!("{}", system_fds);

    if args.live {
        if check_running_daemon_mode()? {
            println!("powereg already running in daemon mode!");
            println!("\tuse powereg --monitor");
            return Ok(());
        }

        loop {
            let event = poll_events(&socket);
            handle_event(&event, &mut system_fds)?;
        }
    } else if args.monitor {
        //if !check_running_daemon_mode()? {
        //    println!("start powereg daemon mode with sudo powereg --daemon!");
        //    return Ok(());
        //}

        //loop {
        //    print!("\x1B[2J\x1B[1;1H");
        //    std::io::stdout().flush().unwrap();
        //    println!("{}", system_fds);
        //    std::thread::sleep(std::time::Duration::from_secs(2));
        //}

        let terminal = ratatui::init();
        let _ = run_tui(terminal, system_fds);
    } else if args.install {
        install_daemon()?;
    } else if args.uninstall {
        uninstall_daemon()?;
    }

    Ok(())
}
