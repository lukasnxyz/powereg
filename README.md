## Powereg
A simple power management tool that controls the cpus power profile, EPP, as well as battery thresholds for ThinkPads.
Currently only supports AMD cpus. If you have an Intel cpu and would like to expand this project, feel free submit a pr!

<<<<<<< HEAD
=======
** Still in development **

>>>>>>> rust-rewrite
### Getting started
```
./install.sh
sudo powereg --install
```
<<<<<<< HEAD

You can configure powereg via `~/.config/powereg/powereg.conf`.

### Options
Powereg will need to be run with sudo
=======
You can configure powereg via `~/.config/powereg/powereg.conf`.

### Options
powereg will need to be run with sudo
>>>>>>> rust-rewrite
- `--monitor`: simply display your system states while powereg runs as a daemon in the background.
- `--live`: runs powereg in the background while showing your system states.
- `--daemon`: runs powereg with no feedback.
- `--install`: install powereg via `systemctl enable` and `systemctl start`.
- `--uninstall`: uninstalls powereg via `systemctl disable` and `systemctl stop`.

### License
- MIT License (./LICENSE)

### Credits
- Inspired by auto-cpufreq (https://github.com/AdnanHodzic/auto-cpufreq).

<<<<<<< HEAD
## Todo
----
- [ ] auto start/stop bluetooth ('bluetoothctl power off/on')
- [ ] intel_pstate support
- [ ] make high/low cpu load and temp configurable in config

=======
### Todo
- [ ] auto start/stop bluetooth ('bluetoothctl power off/on')
- [ ] intel_pstate support
- [ ] make high/low cpu load and temp configurable in config
>>>>>>> rust-rewrite
- [ ] tests somehow?
- [ ] nicer --live/--monitor cli view/updating
- [ ] remove need for allocator (only in Config now)
- [ ] use libsystemd over calling system shell commands for systemctl
<<<<<<< HEAD
=======
- [ ] interact directly with libudev instead of crate
- [ ] custom cli args parse function
>>>>>>> rust-rewrite
