use crate::{events::handle_event, fds::SystemFds, system_state::SystemState};
use clap::Parser;
use events::poll_events;
use std::{io, os::unix::io::AsRawFd};
use udev::MonitorBuilder;

mod events;
mod fds;
mod system_state;

#[derive(Parser, Debug)]
#[command(version, about)]
struct Args {
    #[arg(long)]
    daemon: bool,
}

fn main() -> io::Result<()> {
    let system_state = SystemState::init();
    if !system_state.linux {
        println!("need to be running on linux!");
        return Ok(());
    }
    println!("{}", system_state);

    let args = Args::parse();

    if args.daemon {
        println!("daemon mode implemented");
        return Ok(());
    } else {
        println!("running non-daemon mode");
    }

    let socket = MonitorBuilder::new()?
        .match_subsystem("power_supply")?
        .listen()?;

    let fd = socket.as_raw_fd();
    println!("udev monitor started successfully on FD: {}", fd);

    // TODO: need to call either set performance or powersave upon startup as well

    let mut system_fds = SystemFds::init(system_state.num_cpu_cores)?;
    loop {
        let event = poll_events(&socket);
        handle_event(&event, &mut system_fds)?;
    }
}

/*
POSSIBLE OPTIMIZATION VECTORS
    - /sys/devices/system/cpu/cpu * /cpufreq/scaling_governor
    - enable/disable /sys/devices/system/cpu/cpufreq/boost
    - battery thresholds on thinkpad
*/

/*
fn read_ac_status() -> io::Result<String> {
    let mut file = File::open(AC_ONLINE_PATH)?;
    let mut content = String::new();
    file.read_to_string(&mut content)?;
    Ok(content.trim().to_string())
}

fn check_initial_ac_status() -> io::Result<bool> {
    use std::fs;
    use std::path::Path;

    let power_supply_path = Path::new("/sys/class/power_supply");

    if let Ok(entries) = fs::read_dir(power_supply_path) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name_str = name.to_string_lossy();

            if name_str.starts_with("AC") || name_str.starts_with("ACAD") {
                let online_path = entry.path().join("online");
                if let Ok(content) = fs::read_to_string(online_path) {
                    return Ok(content.trim() == "1");
                }
            }
        }
    }

    Ok(false)
}
*/

/*
- read off bat (charging, nothing, or discharging)
- read system/cpu load

- read/switch governers: /sys/devices/system/cpu/cpu0/cpufreq/scaling_available_governors
- lower max frequency on battery
- battery thresholds

#include <stdio.h>
#include <stdlib.h>
#include <stdint.h>
#include <unistd.h>
#include <string.h>
#include <assert.h>

#include "battery.h"
#include "cpu.h"

typedef struct {
  long nprocs;
  float *cpu_freqs;
  int cpu_temp;
  CPUGovState cpu_gov_state;
  BatteryStatus battery_status;
  int battery_charge;
} SystemState;

int init_load_system_state(long nprocs, SystemState *system_state) {
  system_state->nprocs = nprocs;
  system_state->cpu_freqs = malloc(nprocs * sizeof(float));
  if (!system_state->cpu_freqs) {
    perror("malloc vals"); return -1;
  }

  // TODO: actually don't do like fopen fclose every function call

  if (get_cpu_freqs(system_state->cpu_freqs) == -1) {
    free(system_state->cpu_freqs);
    perror("read_cpu_freqs"); return -1;
  }
  if (get_cpu_temp(&system_state->cpu_temp) == -1) {
    free(system_state->cpu_freqs);
    perror("read_cpu_temp"); return -1;
  }
  if (get_cpu_gov(&system_state->cpu_gov_state) == -1) {
    free(system_state->cpu_freqs);
    perror("read_cpu_gov"); return -1;
  }
  if (get_battery_status(&system_state->battery_status) == -1) {
    free(system_state->cpu_freqs);
    perror("get_battery_status"); return -1;
  }
  if (get_battery_charge(&system_state->battery_charge) == -1) {
    free(system_state->cpu_freqs);
    perror("get_battery_charge"); return -1;
  }

  return 0;
}

void deinit_system_state(SystemState *system_state) {
  free(system_state->cpu_freqs);
}

void print_system_state(SystemState system_state) {
  for (size_t i = 0; i < 16; ++i) printf("core %zu: %.3f\n", i, system_state.cpu_freqs[i]);
  printf("cpu temp: %d'C\n", system_state.cpu_temp);
  printf("current cpu governer: "); print_cpu_gov_state(system_state.cpu_gov_state);
  printf("battery status: "); print_batter_status(system_state.battery_status);
  printf("battery charge: %d\n", system_state.battery_charge);
}

int main(void) {
  long nprocs = sysconf(_SC_NPROCESSORS_ONLN);
  if (nprocs < 1) {
    perror("sysconf"); return -1;
  }

  SystemState system_state;
  if (init_load_system_state(nprocs, &system_state) == -1) {
    return -1;
  }
  print_system_state(system_state);
  if (get_cpu_govs() == -1) {
    return -1;
  }

  //for (;;) {
  //}

  deinit_system_state(&system_state);

  return 0;
}

#ifndef __BATTERY_H__
#define __BATTERY_H__

#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "types.h"

static const char *BATTERY_STATUS = "/sys/class/power_supply/BAT0/status";
static const char *BATTERY_CHARGE = "/sys/class/power_supply/BAT0/capacity";
static const char *BATTERY_START_THRESHOLD = "/sys/class/power_supply/BAT0/charge_start_threshold";
static const char *BATTERY_STOP_THRESHOLD = "/sys/class/power_supply/BAT0/charge_stop_threshold";

typedef enum {
  BATTERY_STATUS_DISCHARGING,
  BATTERY_STATUS_CHARGING,
  BATTERY_STATUS_FULLY_CHARGED,
} BatteryStatus;

void print_batter_status(BatteryStatus battery_status) {
  switch (battery_status) {
    case BATTERY_STATUS_DISCHARGING:
      printf("BATTERY_STATUS_DISCHARGING\n");
      break;
    case BATTERY_STATUS_CHARGING:
      printf("BATTERY_STATUS_CHARGING\n");
      break;
    case BATTERY_STATUS_FULLY_CHARGED:
      printf("BATTERY_STATUS_FULLY_CHARGED\n");
      break;
  }
}

int get_battery_status(BatteryStatus *battery_status) {
  FILE *file = fopen(BATTERY_STATUS, "r");
  if (file == NULL) return -1;

  char line[LINE_LEN];
  if (fgets(line, LINE_LEN, file)) {
    size_t len = strlen(line);
    if (len > 0 && line[len-1] == '\n') line[len-1] = '\0';

    if (strncmp(line, "Discharging", len) == 0) {
      *battery_status = BATTERY_STATUS_DISCHARGING;
    } else if (strncmp(line, "Charging", len) == 0) {
      *battery_status = BATTERY_STATUS_CHARGING;
    } else if (strncmp(line, "Fully-Charged", len) == 0) {
      *battery_status = BATTERY_STATUS_FULLY_CHARGED;
    } else {
      fclose(file);
      return -1;
    }
  } else {
    fclose(file);
    return -1;
  }
  fclose(file);

  return 0;
}

int get_battery_charge(int *battery_charge) {
  FILE *file = fopen(BATTERY_CHARGE, "r");
  if (file == NULL) return -1;

  char line[LINE_LEN];
  if (fgets(line, LINE_LEN, file)) {
    *battery_charge = (int)strtof(line, NULL);
  } else {
    fclose(file);
    return -1;
  }

  fclose(file);

  return 0;
}

#endif

#ifndef __CPU_H__
#define __CPU_H__

#include <stdio.h>
#include <stdlib.h>
#include <stdint.h>
#include <string.h>
#include <stdbool.h>

#include "types.h"

#define ST_CPU_FREQ "cpu MHz"

static const char *CPU_INFO_FILE = "/proc/cpuinfo";
static const char *CPU_TEMP_FILE = "/sys/class/thermal/thermal_zone0/temp";
static const char *CPU_GOV_FILE = "/sys/devices/system/cpu/cpu0/cpufreq/scaling_governor";
static const char *CPU_AVAILABLE_GOVS_FILE = "/sys/devices/system/cpu/cpu0/cpufreq/scaling_available_governors";

typedef enum {
  CPUGOVSTATE_PERFORMANCE,
  CPUGOVSTATE_POWERSAVE,
} CPUGovState;

void print_cpu_gov_state(CPUGovState state) {
  switch (state) {
    case CPUGOVSTATE_PERFORMANCE:
      printf("CPUGovState_PERFORMANCE\n"); break;
    case CPUGOVSTATE_POWERSAVE:
      printf("CPUGovState_POWERSAVE\n"); break;
  }
}

static bool check_cpu_gov_state(char *str) {
  if (strcmp(str, "performance")) return true;
  else if (strcmp(str, "powersave")) return true;
  return false;
}

int get_cpu_freqs(float *cpu_freqs) {
  FILE *file = fopen(CPU_INFO_FILE, "r");
  if (file == NULL) return -1;

  char line[LINE_LEN];
  for (uint8_t i = 0; fgets(line, LINE_LEN, file);) {
    if (strstr(line, ST_CPU_FREQ)) {
      float value;

      if (sscanf(line, "%*[^:]: %f", &value) != 1) {
        perror("extracting number");
        fclose(file);
        return -1;
      }

      cpu_freqs[i] = value;
      ++i;
    }
  }

  fclose(file);
  return 0;
}

int get_cpu_temp(int *cpu_temp) {
  FILE *file = fopen(CPU_TEMP_FILE, "r");
  if (file == NULL) return -1;

  char line[LINE_LEN];
  if (fgets(line, LINE_LEN, file)) {
    *cpu_temp = (int)(strtof(line, NULL) / 1000);
  } else {
    fclose(file);
    return -1;
  }

  fclose(file);
  return 0;
}

int get_cpu_gov(CPUGovState *state) {
  FILE *file = fopen(CPU_GOV_FILE, "r");
  if (file == NULL) return -1;

  char line[LINE_LEN];
  if (fgets(line, LINE_LEN, file)) {
    size_t len = strlen(line);
    if (len > 0 && line[len-1] == '\n') line[len-1] = '\0';
    if (strncmp(line, "performance", len) == 0) {
      *state = CPUGOVSTATE_PERFORMANCE;
    } else if (strncmp(line, "powersave", len) == 0) {
      *state = CPUGOVSTATE_POWERSAVE;
    } else {
      fclose(file);
      return -1;
    }
  }

  fclose(file);
  return 0;
}

int get_cpu_govs(void) {
  FILE *file = fopen(CPU_AVAILABLE_GOVS_FILE, "r");
  if (file == NULL) return -1;

  char line[LINE_LEN];
  if (fgets(line, LINE_LEN, file)) {
    line[strcspn(line, "\n")] = '\0';

    char *token = strtok(line, " ");
    while (token != NULL) {
      if (!check_cpu_gov_state(token)) {
        fclose(file);
        return -1;
      }
      token = strtok(NULL, " ");
    }
  }

  fclose(file);
  return -1;
}

#endif
*/
