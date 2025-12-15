use crate::{
    events::handle_event,
    fds::SystemFds,
    system_state::{set_performance_mode, set_powersave_mode, ChargingStatus, SystemState},
};
use clap::Parser;
use events::poll_events;
use std::{io, os::unix::io::AsRawFd};
use udev::MonitorBuilder;

mod events;
mod fds;
mod system_state;

#[derive(Parser, Debug)]
#[command(version, about)]
struct Args {
    #[arg(long)]
    daemon: bool,
}

fn main() -> io::Result<()> {
    // TODO: require super user privileges

    let system_state = SystemState::init();
    if !system_state.linux {
        println!("need to be running on linux!");
        return Ok(());
    }
    println!("{}", system_state);

    /*
    let args = Args::parse();
    if args.daemon {
        println!("daemon mode implemented");
        return Ok(());
    } else {
        println!("running non-daemon mode");
    }
    */

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

    loop {
        let event = poll_events(&socket);
        handle_event(&event, &mut system_fds)?;
    }
}
