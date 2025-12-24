use powereg::events::{handle_event, EventPoller};
use powereg::setup::{check_running_daemon_mode, install_daemon, uninstall_daemon};
use powereg::system_state::{Config, SystemState};
use clap::Parser;

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
        eprintln!("Need to run with root privileges!");
        return;
    }

    let system_state = SystemState::init().unwrap();
    system_state.post_init().unwrap();
    if !system_state.linux {
        eprintln!("Need to be running on Linux!");
        return;
    }
    println!("{}", system_state);

    if let Ok(config_path) = Config::get_config_path() {
        println!("config path: {config_path}");
        match Config::parse(&config_path) {
            Ok(config) => match config.apply(&system_state) {
                Ok(_) => {}
                Err(e) => println!("Error while applying config: {e}"),
            },
            Err(e) => eprintln!("Error loading config: {}", e),
        };
    } else {
        eprintln!("Error loading config");
    }

    if args.monitor {
        //if !check_running_daemon_mode()? {
        //    println!("start powereg daemon mode with sudo powereg --daemon!");
        //    return Ok(());
        //}

        loop {
            println!("{}", system_state.cpu_states);
            println!("{}", system_state.battery_states);
            std::thread::sleep(std::time::Duration::from_millis(500));
        }
    } else if args.live {
        if check_running_daemon_mode().unwrap() {
            println!("powereg already running in daemon mode!");
            println!("\tuse powereg --monitor");
            return;
        }

        let mut poller = EventPoller::new(5).unwrap();
        loop {
            let event = poller.poll_events();
            handle_event(&event, &system_state).unwrap();
            print!("\x1B[2J\x1B[1;1H"); // Clear screen and move cursor to top
            println!("{}", system_state.cpu_states);
            println!("{}", system_state.battery_states);
        }
    } else if args.daemon {
        let mut poller = EventPoller::new(5).unwrap();
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
