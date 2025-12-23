use crate::{
    events::{handle_event, EventPoller},
    setup::{check_running_daemon_mode, install_daemon, uninstall_daemon},
    system_state::{Config, SystemState},
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

mod battery;
mod cpu;
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
    #[arg(long, help = "Monitor running daemon and system stats")]
    pub monitor: bool,
    #[arg(long, help = "Run in live mode")]
    pub live: bool,
    #[arg(long, help = "Run powereg daemon mode")]
    pub daemon: bool,
    #[arg(long, help = "Install powereg as a daemon on your system")]
    pub install: bool,
    #[arg(long, help = "Uninstall powereg on your system")]
    pub uninstall: bool,
}

fn main() {
    let args = Args::parse();

    if !unsafe { libc::geteuid() == 0 } {
        println!("Need to run with root privileges!");
        return;
    }

    let system_state = SystemState::init().unwrap();
    system_state.post_init().unwrap();
    if !system_state.linux {
        println!("Need to be running on Linux!");
        return;
    }
    println!("{}", system_state);

    match Config::parse(CONFIG_PATH) {
        Ok(config) => {
            match config.apply(&system_state) {
                Ok(_) => {}
                Err(e) => println!("Error while applying config: {e}"),
            }
        }
        Err(e) => eprintln!("Error loading config: {}", e),
    };

    if args.monitor {
        //if !check_running_daemon_mode()? {
        //    println!("start powereg daemon mode with sudo powereg --daemon!");
        //    return Ok(());
        //}

        let terminal = ratatui::init();
        let _ = run_tui(terminal, &system_state);
    } else if args.live {
        if check_running_daemon_mode().unwrap() {
            println!("powereg already running in daemon mode!");
            println!("\tuse powereg --monitor");
            return;
        }

        let stop_signal = Arc::new(AtomicBool::new(false));
        let r = stop_signal.clone();
        let event_handle = thread::spawn(move || -> io::Result<()> {
            let mut poller = EventPoller::new().unwrap();
            while !r.load(Ordering::Relaxed) {
                let event = poller.poll_events();
                handle_event(&event, &system_state).unwrap();
            }

            Ok(())
        });

        let terminal = ratatui::init();
        let _ = run_tui(terminal, &system_state);

        stop_signal.store(true, Ordering::Relaxed);
        let _ = event_handle.join();
    } else if args.daemon {
        let mut poller = EventPoller::new().unwrap();
        loop {
            let event = poller.poll_events();
            handle_event(&event, &system_state).unwrap();
        }
    } else if args.install {
        install_daemon().unwrap();
    } else if args.uninstall {
        uninstall_daemon().unwrap();
    }
}
