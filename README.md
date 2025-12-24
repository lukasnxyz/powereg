## powereg

### Getting started
```
cargo build --release && sudo cp target/release/powereg /usr/local/bin/
sudo powereg --install
```

#### Credits
Heavily inspired by [auto-cpufreq](https://github.com/AdnanHodzic/auto-cpufreq), I just wanted
a power management tool in rust bc rust is cool

#### TODO to v1
- [X] Separate CPU and battery configs
- [X] Non fatal errors shouldn't crash
- [X] Thinkpad ACPI check only for now
- [ ] only support amd for now check
- [ ] Event loop better and proper events
- [X] TUI displaying all information
