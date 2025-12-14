## powereg

### notes
- https://github.com/AdnanHodzic/auto-cpufreq
- https://gitlab.freedesktop.org/upower/power-profiles-daemon

#### possible optimization vectors
- amd_pstate
    - `/sys/devices/system/cpu/amd_pstate/status`
    - `/sys/devices/system/cpu/cpu*/cpufreq/scaling_min_freq and scaling_max_freq`
    - `/sys/devices/system/cpu/cpu*/cpufreq/energy_performance_preference`
- `/sys/devices/system/cpu/cpu*/cpufreq/scaling_governor`
- enable/disable `/sys/devices/system/cpu/cpufreq/boost`
- battery thresholds on thinkpad
