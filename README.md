## powereg

### Getting started
```
cargo build --release
sudo cp target/release/powereg /usr/local/bin/
sudo powereg --install
```

#### Credits
Heavily inspired by [auto-cpufreq](https://github.com/AdnanHodzic/auto-cpufreq), I just wanted
a power management tool in rust bc rust is cool

#### TODO to v1
- [X] Separate CPU and battery configs
- [ ] Non fatal errors shouldn't crash
- [ ] Thinkpad ACPI check as well as only support amd for now
- [ ] Event loop better and proper events
- [ ] TUI displaying all information
