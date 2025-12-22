use std::{io, process::Command};

const SERVICE_NAME: &str = "powereg";
const SERVICE_PATH: &str = "/etc/systemd/system/powereg.service";
const BINARY_PATH: &str = "/usr/local/bin/powereg";
const RUN_FLAG: &str = "--daemon";

pub fn check_running_daemon_mode() -> io::Result<bool> {
    let output = std::process::Command::new("systemctl")
        .args(&["is-active", SERVICE_NAME])
        .output()
        .map_err(|e| {
            io::Error::new(
                e.kind(),
                format!("Failed to run 'systemctl is-active': {}", e),
            )
        })?;

    Ok(output.status.success())
}

pub fn install_daemon() -> io::Result<()> {
    if check_installed_power_tools() {
        println!("Make sure that you are not running any other power management tools such as");
        println!("\tpower-profiles-daemon\ttlp\tauto-cpufreq");
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "Failed to start powereg",
        ));
    }

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
            format!("Failed to write service file to {}: {}", SERVICE_PATH, e),
        )
    })?;

    let output = Command::new("systemctl")
        .arg("daemon-reload")
        .output()
        .map_err(|e| {
            io::Error::new(
                e.kind(),
                format!("Failed to run 'systemctl daemon-reload': {}", e),
            )
        })?;
    if !output.status.success() {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            format!(
                "systemctl daemon-reload failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ),
        ));
    }

    println!("enabling daemon");
    let output = Command::new("systemctl")
        .args(&["enable", SERVICE_NAME])
        .output()
        .map_err(|e| {
            io::Error::new(e.kind(), format!("Failed to run 'systemctl enable': {}", e))
        })?;
    if !output.status.success() {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            format!(
                "systemctl enable failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ),
        ));
    }

    println!("starting daemon");
    let output = Command::new("systemctl")
        .args(&["start", SERVICE_NAME])
        .output()
        .map_err(|e| io::Error::new(e.kind(), format!("Failed to run 'systemctl start': {}", e)))?;
    if !output.status.success() {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            format!(
                "systemctl start failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ),
        ));
    }

    println!("powereg succesfully installed and started via systemd!");

    Ok(())
}

pub fn uninstall_daemon() -> io::Result<()> {
    println!("disabling daemon");
    let output = Command::new("systemctl")
        .args(&["disable", SERVICE_NAME])
        .output()
        .map_err(|e| {
            io::Error::new(
                e.kind(),
                format!("Failed to run 'systemctl disable': {}", e),
            )
        })?;
    if !output.status.success() {
        eprintln!(
            "Warning: systemctl disable failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    println!("stop daemon");
    let output = Command::new("systemctl")
        .args(&["stop", SERVICE_NAME])
        .output()
        .map_err(|e| io::Error::new(e.kind(), format!("Failed to run 'systemctl stop': {}", e)))?;
    if !output.status.success() {
        eprintln!(
            "Warning: systemctl stop failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    println!("uninstalling daemon");
    std::fs::remove_file(SERVICE_PATH).map_err(|e| {
        io::Error::new(
            e.kind(),
            format!("Failed to remove service file at {}: {}", SERVICE_PATH, e),
        )
    })?;

    let output = Command::new("systemctl")
        .arg("daemon-reload")
        .output()
        .map_err(|e| {
            io::Error::new(
                e.kind(),
                format!("Failed to run 'systemctl daemon-reload': {}", e),
            )
        })?;
    if !output.status.success() {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            format!(
                "systemctl daemon-reload failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ),
        ));
    }

    println!("powereg uninstalled successfully!");

    Ok(())
}

fn check_installed_power_tools() -> bool {
    // TODO: make sure power-profilesdaemon is not running
    // TODO: make tlp is not running
    // TODO: make sure auto-cpufreq is not running

    false
}
