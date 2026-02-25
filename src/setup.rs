use crate::utils::StyledString;
use std::{io, process::Command};

const SERVICE_NAME: &str = "powereg";
const SERVICE_PATH: &str = "/etc/systemd/system/powereg.service";
const BINARY_PATH: &str = "/usr/local/bin/powereg";
const RUN_FLAG: &str = "--daemon";

pub fn check_running_daemon_mode() -> io::Result<bool> {
    println!("{}", "Running 'systemctl is-active powereg'".yellow());
    let output = Command::new("systemctl")
        .args(&["is-active", SERVICE_NAME])
        .output()
        .map_err(|e| {
            io::Error::new(
                e.kind(),
                format!("Failed to run 'systemctl is-active': {}", e),
            )
        })?;

    let status_text = String::from_utf8_lossy(&output.stdout)
        .trim()
        .to_lowercase();
    match status_text.as_str() {
        "active" => Ok(true),
        "inactive" => Ok(false),
        _ => Ok(false),
    }
}

pub fn install_daemon() -> io::Result<()> {
    let _ = check_installed_power_tools();

    let service_file = format!(
        r#"[Unit]
Description=PowerEG - Power Management Daemon
After=network.target
Documentation=man:{}(8)

[Service]
Type=simple
User=root
ExecStart={} {}
Restart=on-failure
RestartSec=10

# Security and isolation options
ProtectSystem=strict
ProtectHome=yes
NoNewPrivileges=true
PrivateTmp=yes

# Logging
StandardOutput=journal
StandardError=journal
SyslogIdentifier={}

[Install]
WantedBy=multi-user.target
    "#,
        SERVICE_NAME, BINARY_PATH, RUN_FLAG, SERVICE_NAME
    );

    std::fs::write(SERVICE_PATH, service_file).map_err(|e| {
        io::Error::new(
            e.kind(),
            format!(
                "{} {}: {}",
                "Failed to write service file to".red(),
                SERVICE_PATH,
                e
            ),
        )
    })?;

    println!("{}", "Running 'systemctl daemon-reload'".yellow());
    let output = Command::new("systemctl")
        .arg("daemon-reload")
        .output()
        .map_err(|e| {
            io::Error::new(
                e.kind(),
                format!("{} {}", "Failed to run 'systemctl daemon-reload':".red(), e),
            )
        })?;
    if !output.status.success() {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            format!(
                "{} {}",
                "systemctl daemon-reload failed:".red(),
                String::from_utf8_lossy(&output.stderr),
            ),
        ));
    }

    println!("{}", "Running 'systemctl enable powereg'".yellow());
    let output = Command::new("systemctl")
        .args(&["enable", SERVICE_NAME])
        .output()
        .map_err(|e| {
            io::Error::new(
                e.kind(),
                format!("{} {}", "Failed to run 'systemctl enable':".red(), e),
            )
        })?;
    if !output.status.success() {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            format!(
                "{} {}",
                "systemctl enable failed:".red(),
                String::from_utf8_lossy(&output.stderr),
            ),
        ));
    }

    println!("{}", "Running 'systemctl start powereg'".yellow());
    let output = Command::new("systemctl")
        .args(&["start", SERVICE_NAME])
        .output()
        .map_err(|e| {
            io::Error::new(
                e.kind(),
                format!("{} {}", "Failed to run 'systemctl start':".red(), e),
            )
        })?;
    if !output.status.success() {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            format!(
                "{} {}",
                "systemctl start failed:".red(),
                String::from_utf8_lossy(&output.stderr),
            ),
        ));
    }

    println!(
        "{}",
        "Powereg succesfully installed and started via systemd!".green()
    );

    Ok(())
}

pub fn uninstall_daemon() -> io::Result<()> {
    println!("{}", "Running 'systemctl disable powereg'".yellow());
    let output = Command::new("systemctl")
        .args(&["disable", SERVICE_NAME])
        .output()
        .map_err(|e| {
            io::Error::new(
                e.kind(),
                format!("{} {}", "Failed to run 'systemctl disable':".red(), e),
            )
        })?;
    if !output.status.success() {
        eprintln!(
            "{} {}",
            "systemctl disable failed:".red(),
            String::from_utf8_lossy(&output.stderr),
        );
    }

    println!("{}", "Running 'systemctl stop powereg'".yellow());
    let output = Command::new("systemctl")
        .args(&["stop", SERVICE_NAME])
        .output()
        .map_err(|e| {
            io::Error::new(
                e.kind(),
                format!("{} {}", "Failed to run 'systemctl stop':".red(), e),
            )
        })?;
    if !output.status.success() {
        eprintln!(
            "{} {}",
            "systemctl stop failed:".red(),
            String::from_utf8_lossy(&output.stderr),
        );
    }

    std::fs::remove_file(SERVICE_PATH).map_err(|e| {
        io::Error::new(
            e.kind(),
            format!(
                "{} {}: {}",
                "Failed to remove service file at".red(),
                SERVICE_PATH,
                e
            ),
        )
    })?;

    println!("{}", "Running 'systemctl daemon-reload'".yellow());
    let output = Command::new("systemctl")
        .arg("daemon-reload")
        .output()
        .map_err(|e| {
            io::Error::new(
                e.kind(),
                format!("{} {}", "Failed to run 'systemctl daemon-reload':".red(), e),
            )
        })?;
    if !output.status.success() {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            format!(
                "{} {}",
                "systemctl daemon-reload failed:".red(),
                String::from_utf8_lossy(&output.stderr),
            ),
        ));
    }

    println!("{}", "Powereg uninstalled successfully!".green());

    Ok(())
}

fn check_installed_power_tools() -> bool {
    let services = vec![
        "power-profiles-daemon.service",
        "tlp.service",
        "auto-cpufreq.service",
    ];

    let mut conflicts_found = false;

    for service in services {
        let status = Command::new("systemctl")
            .args(&["is-active", service])
            .output();

        if let Ok(output) = status {
            let status_str = String::from_utf8_lossy(&output.stdout).trim().to_string();

            if status_str == "active" {
                println!("{} {}", "Found running service:".yellow(), service);
                conflicts_found = true;

                println!("\t{} {}...", "Attempting to stop".yellow(), service);
                let stop_result = Command::new("systemctl").args(&["stop", service]).output();

                match stop_result {
                    Ok(output) if output.status.success() => {
                        println!("\t{} {}", "Successfully stopped".green(), service);
                    }
                    _ => {
                        println!("\t{} {}", "Failed to stop".red(), service);
                        continue;
                    }
                }

                println!("\t{} {}...", "Attempting to disable".yellow(), service);
                let disable_result = Command::new("systemctl")
                    .args(&["disable", service])
                    .output();

                match disable_result {
                    Ok(output) if output.status.success() => {
                        println!("\t{} {}", "Successfully disabled".green(), service);
                    }
                    _ => {
                        println!("\t{} {}", "Failed to disable".red(), service);
                    }
                }
            }
        }
    }

    if !conflicts_found {
        println!(
            "{}",
            "No conflicting power management services found".green()
        );
    }

    conflicts_found
}
