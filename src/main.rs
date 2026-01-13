use clap::Parser;
use powereg::events::EventPoller;
use powereg::setup::{check_running_daemon_mode, install_daemon, uninstall_daemon};
use powereg::system_state::SystemState;
use powereg::utils::{StyledString, Config};

const LOOP_DURATION: u8 = 5;

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
    if !unsafe { libc::geteuid() == 0 } {
        eprintln!("{}", "Need to run with root privileges!".red());
        return;
    }

    let system_state = SystemState::init().unwrap();
    system_state.post_init().unwrap();
    if !system_state.linux {
        eprintln!("{}", "Need to be running on Linux!".red());
        return;
    }

    // TODO: listen for 'q' to quit out

    let args = Args::parse();
    if args.monitor {
        if !check_running_daemon_mode().unwrap() {
            println!("{}", "powereg not running in daemon mode!".red());
            println!("{}", "\tuse 'sudo powereg --install'".red());
        }

        let mut poller = EventPoller::new(LOOP_DURATION).unwrap();
        loop {
            print!("\x1B[2J\x1B[1;1H");
            println!("{}", system_state);
            let _ = poller.poll_events();
        }
    } else if args.live {
        setup_config(&system_state);

        if check_running_daemon_mode().unwrap() {
            println!("{}", "Powereg already running in daemon mode!".red());
            println!("{}", "\tuse 'sudo powereg --monitor'".red());
            return;
        }

        let mut poller = EventPoller::new(LOOP_DURATION).unwrap();
        loop {
            print!("\x1B[2J\x1B[1;1H");
            println!("{}", system_state);
            let event = poller.poll_events();
            event.handle_event(&system_state).unwrap();
        }
    } else if args.daemon {
        setup_config(&system_state);

        let mut poller = EventPoller::new(LOOP_DURATION).unwrap();
        loop {
            let event = poller.poll_events();
            event.handle_event(&system_state).unwrap();
        }
    } else if args.install {
        setup_config(&system_state);

        if check_running_daemon_mode().unwrap() {
            println!("{}", "Powereg already running in daemon mode!".red());
            return;
        }

        install_daemon().unwrap();
    } else if args.uninstall {
        if !check_running_daemon_mode().unwrap() {
            println!("{}", "Powereg is not running in daemon mode!".red());
            return;
        }

        uninstall_daemon().unwrap();
    }
}

fn setup_config(system_state: &SystemState) {
    if let Ok(config_path) = Config::get_config_path() {
        println!("Config path: {config_path}");
        match Config::parse(&config_path) {
            Ok(config) => match config.apply(&system_state) {
                Ok(_) => {}
                Err(e) => println!("{} {}", "Error while applying config:".red(), e),
            },
            Err(e) => eprintln!("{} {}", "Error loading config:".red(), e),
        };
    } else {
        println!("{}", "Error loading config".red());
    }
}
