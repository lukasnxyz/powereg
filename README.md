## Powereg
A simple power management tool that controls the cpus power profile, EPP, as well as battery thresholds for ThinkPads.
Currently only supports AMD cpus. If you have an Intel cpu and would like to expand this project, feel free submit a pr!

** Still in development **

### Getting started
```
./install.sh
sudo powereg --install
```
You can configure powereg via `~/.config/powereg/powereg.conf`.

### Options
powereg will need to be run with sudo
- `--monitor`: simply display your system states while powereg runs as a daemon in the background.
- `--live`: runs powereg in the background while showing your system states.
- `--daemon`: runs powereg with no feedback.
- `--install`: install powereg via `systemctl enable` and `systemctl start`.
- `--uninstall`: uninstalls powereg via `systemctl disable` and `systemctl stop`.

### License
- MIT License (./LICENSE)

### Credits
- Inspired by auto-cpufreq (https://github.com/AdnanHodzic/auto-cpufreq).

### Todo
- [ ] install script check for cargo, libudev, and systemd
- [ ] intel_pstate support
- [ ] auto start/stop bluetooth ('bluetoothctl power off/on')
- [ ] make high/low cpu load and temp configurable in config
- [ ] tests somehow?
- [ ] use libsystemd over calling system shell commands for systemctl
- [ ] interact directly with libudev instead of crate
- [ ] custom cli args parse function
