const std = @import("std");
const Allocator = std.mem.Allocator;
const assert = std.debug.assert;
const builtin = @import("builtin");
const OpenFlags = std.fs.File.OpenFlags;

pub fn main() !void {
    var gpa = std.heap.GeneralPurposeAllocator(.{}){};
    defer _ = gpa.deinit();
    const allocator = gpa.allocator();

    var system_state = try SystemState.init(allocator);
    defer system_state.deinit(allocator);

    const ArgType = enum { live, monitor, daemon, install, uninstall };
    const arg_type = try parseArg(ArgType);
    switch (arg_type) {
        .live => {
            std.debug.print("Running in live mode...\n", .{});

            try system_state.battery_states.print();
        },
        .monitor => {
            std.debug.print("Running in monitor mode...\n", .{});
        },
        .daemon => {
            std.debug.print("Running in daemon mode...\n", .{});
        },
        .install => {
            std.debug.print("Running install...\n", .{});
        },
        .uninstall => {
            std.debug.print("Running uninstall...\n", .{});
        },
    }
}

fn parseArg(comptime EnumType: type) !EnumType {
    var args = std.process.args();
    _ = args.next();

    var found_arg: ?EnumType = null;
    var arg_count: usize = 0;

    while (args.next()) |arg| {
        arg_count += 1;

        if (!std.mem.startsWith(u8, arg, "--")) {
            std.debug.print("Error: Argument must start with '--', got: {s}\n", .{arg});
            return error.InvalidArgumentFormat;
        }

        const arg_name = arg[2..];

        if (std.meta.stringToEnum(EnumType, arg_name)) |value| {
            if (found_arg != null) {
                std.debug.print("Error: Multiple arguments provided. Only one is allowed.\n", .{});
                return error.TooManyArguments;
            }
            found_arg = value;
        } else {
            std.debug.print("Error: Invalid argument '--{s}'\n", .{arg_name});
            std.debug.print("Valid options: ", .{});
            inline for (@typeInfo(EnumType).@"enum".fields, 0..) |field, i| {
                if (i > 0) std.debug.print(", ", .{});
                std.debug.print("--{s}", .{field.name});
            }
            std.debug.print("\n", .{});
            return error.InvalidArgument;
        }
    }

    if (found_arg) |arg| {
        return arg;
    } else {
        std.debug.print("Error: No argument provided.\n", .{});
        std.debug.print("Valid options: ", .{});
        inline for (@typeInfo(EnumType).@"enum".fields, 0..) |field, i| {
            if (i > 0) std.debug.print(", ", .{});
            std.debug.print("--{s}", .{field.name});
        }
        std.debug.print("\n", .{});
        return error.NoArgument;
    }
}

pub const PersFdError = error {
    InvalidFilePermsErr,
};

pub const PersFd = struct {
    file: std.fs.File,
    write: bool,
    buffer: [512]u8 = undefined,

    pub fn init(path: []const u8, write: bool) !PersFd {
        const flags = if (write) OpenFlags{ .mode = .read_write } else OpenFlags{ .mode = .read_only };
        const file = try std.fs.cwd().openFile(path, flags);
        return PersFd{
            .file = file,
            .write = write,
        };
    }

    pub fn deinit(self: *PersFd) void {
        @memset(&self.buffer, 0);
        self.file.close();
    }

    pub fn read_value(self: *PersFd) ![]const u8 {
        // TODO: readAll here is very dangerous with buffer[512] for /proc/stat
        try self.file.seekTo(0);
        @memset(&self.buffer, 0);
        const bytes_read = try self.file.readAll(&self.buffer);
        const raw_content = self.buffer[0..bytes_read];
        return std.mem.trim(u8, raw_content, &std.ascii.whitespace);
    }

    pub fn set_value(self: PersFd, value: []const u8) !void {
        if (!self.write) {
            return error.InvalidFilePermsErr;
        }

        try self.file.setEndPos(0);
        try self.file.writeAll(value);
        try self.file.sync();
    }
};

pub const CpuType = enum { AMD, Intel, Unknown };
pub const AcpiType = enum { ThinkPad, IdeaPad, Unknown };
pub const State = enum { Powersave, Balanced, Performance };
pub const SystemState = struct {
    linux: bool,
    cpu_type: CpuType,
    acpi_type: AcpiType,

    cpu_states: CpuStates,
    battery_states: BatteryStates,

    state: State,

    pub fn init(allocator: Allocator) !SystemState {
        const n = try SystemState.num_cpu_cores();
        return .{
            .linux = SystemState.detect_linux(),
            .cpu_type = undefined,
            .acpi_type = try SystemState.detect_acpi_type(),
            .cpu_states = try CpuStates.init(allocator, n, undefined),
            .battery_states = try BatteryStates.init(),
            .state = undefined,
        };
    }

    pub fn deinit(self: *SystemState, allocator: Allocator) void {
        self.battery_states.deinit();
        self.cpu_states.deinit(allocator);
    }

    //pub fn set_powersave_mode(self: SystemState) !void {}
    //pub fn set_balanced_mode(self: SystemState) !void {}
    //pub fn set_performance_mode(self: SystemState) !void {}

    fn detect_linux() bool {
        const compile_time = if (builtin.os.tag == .linux) true else false;
        const proc_exists = if (std.fs.cwd().access("/proc", .{})) true else |_| false;
        const sys_exists = if (std.fs.cwd().access("/sys", .{})) true else |_| false;
        const etc_exists = if (std.fs.cwd().access("/etc", .{})) true else |_| false;

        const etc_os_release = if (std.fs.cwd().access("/etc/os-release", .{})) true else |_| false;
        const usr_os_release = if (std.fs.cwd().access("/usr/lib/os-release", .{})) true else |_| false;
        const has_os_release = etc_os_release or usr_os_release;

        return compile_time
            or (proc_exists and sys_exists)
            or (proc_exists and sys_exists and etc_exists and has_os_release);
    }

    fn num_cpu_cores() !usize {
        const cpu_dir_path = "/sys/devices/system/cpu/";
        var count: usize = 0;

        var dir = try std.fs.openDirAbsolute(cpu_dir_path, .{ .iterate = true });
        defer dir.close();

        var it = dir.iterate();

        while (try it.next()) |entry| {
            if (entry.kind != .directory) continue;

            const name = entry.name;

            if (std.mem.startsWith(u8, name, "cpu") and !std.mem.eql(u8, name, "cpufreq")) {
                const suffix = name[3..];

                _ = std.fmt.parseInt(u32, suffix, 10) catch continue;
                count += 1;
            }
        }

        return count;
    }

    fn detect_acpi_type() !AcpiType {
        const thinkpad = "thinkpad";
        const ideapad = "ideapad";

        var pv = try PersFd.init("/sys/class/dmi/id/product_version", false);
        defer pv.deinit();
        if (pv.read_value()) |product_version| {
            const trimmed = std.mem.trim(u8, product_version, &std.ascii.whitespace);
            var lowered: [pv.buffer.len]u8 = undefined;
            _ = std.ascii.lowerString(&lowered, trimmed);

            if (std.mem.indexOf(u8, &lowered, thinkpad) != null) return AcpiType.ThinkPad;
            if (std.mem.indexOf(u8, &lowered, ideapad) != null) return AcpiType.IdeaPad;
        } else |_| {
        }

        var pn = try PersFd.init("/sys/class/dmi/id/product_name", false);
        defer pn.deinit();
        if (pn.read_value()) |product_name| {
            const trimmed = std.mem.trim(u8, product_name, &std.ascii.whitespace);
            var lowered: [pv.buffer.len]u8 = undefined;
            _ = std.ascii.lowerString(&lowered, trimmed);
            if (std.mem.indexOf(u8, &lowered, thinkpad) != null) return AcpiType.ThinkPad;
            if (std.mem.indexOf(u8, &lowered, ideapad) != null) return AcpiType.IdeaPad;
        } else |_| {
        }

        if (std.fs.cwd().access("/proc/acpi/ibm", .{})) {
            return AcpiType.ThinkPad;
        } else |_| {
        }

        return AcpiType.Unknown;
    }
};


pub const ScalingGoverner = enum {
    Powersave,
    Performance,
    Unknown,

    const PERFORMANCE: []const u8 = "performance";
    const POWERSAVE: []const u8 = "powersave";

    pub fn from_string(s: []const u8) ScalingGoverner {
        if (std.mem.eql(u8, POWERSAVE, s)) {
            return ScalingGoverner.Powersave;
        } else if (std.mem.eql(u8, PERFORMANCE, s)) {
            return ScalingGoverner.Performance;
        } else {
            return ScalingGoverner.Unknown;
        }
    }
};

pub const EPP = enum {
    Default,
    Performance,
    BalancePerformance,
    BalancePower,
    Power,
    Unknown,

    const DEFAULT: []const u8 = "default";
    const PERFORMANCE: []const u8 = "performance";
    const BALANCE_PERFORMANCE: []const u8 = "balance_performance";
    const BALANCE_POWER: []const u8 = "balance_power";
    const POWER: []const u8 = "power";

    pub fn from_string(s: []const u8) EPP {
        if (std.mem.eql(u8, DEFAULT, s)) {
            return EPP.Default;
        } else if (std.mem.eql(u8, PERFORMANCE, s)) {
            return EPP.Performance;
        } else if (std.mem.eql(u8, BALANCE_PERFORMANCE, s)) {
            return EPP.BalancePerformance;
        } else if (std.mem.eql(u8, BALANCE_POWER, s)) {
            return EPP.BalancePower;
        } else if (std.mem.eql(u8, POWER, s)) {
            return EPP.Power;
        } else {
            return EPP.Unknown;
        }
    }
};

pub const CpuStatesError = error {
    InvalidScalingGovs,
};

pub const CpuStates = struct {
    cpu_core_count: usize,
    cpu_type: CpuType,

    scaling_governer: std.ArrayListUnmanaged(PersFd),
    epp: std.ArrayListUnmanaged(PersFd),
    cpu_turbo_boost: PersFd,
    min_cpu_freq: std.ArrayListUnmanaged(PersFd),
    max_cpu_freq: std.ArrayListUnmanaged(PersFd),
    cpu_freq: std.ArrayListUnmanaged(PersFd), // TODO: possibly wrong (not same as btop)
    cpu_temp: PersFd,
    cpu_load: PersFd,       // TODO: possibly wrong
    cpu_power_draw: PersFd, // TODO: possibly wrong


    // TODO: a good way to split this would be via just having strings for amd and intel
    //      for specific paths and having those be dynamic
    pub fn init(allocator: Allocator, n: usize, cpu_type: CpuType) !CpuStates {
        var available_scaling_govs =
            try PersFd.init("/sys/devices/system/cpu/cpu0/cpufreq/scaling_available_governors", false);
        var amd_pstate_status =
            try PersFd.init("/sys/devices/system/cpu/amd_pstate/status", false);

        if (!std.mem.eql(u8, try available_scaling_govs.read_value(), "performance powersave")) {
            std.debug.print("correct options for scaling governers", .{});
            return error.InvalidScalingGovs;
        }

        if (!std.mem.eql(u8, try amd_pstate_status.read_value(), "active")) {
            std.debug.print("amd_pstate is active", .{});
            return error.InvalidScalingGovs;
        }

        var scaling_governer = std.ArrayListUnmanaged(PersFd){};
        var epp = std.ArrayListUnmanaged(PersFd){};
        var cpu_freq = std.ArrayListUnmanaged(PersFd){};
        var max_cpu_freq = std.ArrayListUnmanaged(PersFd){};
        var min_cpu_freq = std.ArrayListUnmanaged(PersFd){};

        for (0..n) |i| {
            var buf: [70]u8 = undefined;

            const scaling_gov_path = try std.fmt.bufPrint(&buf, "/sys/devices/system/cpu/cpu{d}/cpufreq/scaling_governor", .{i});
            try scaling_governer.append(allocator, try PersFd.init(scaling_gov_path, true));

            const epp_path = try std.fmt.bufPrint(&buf, "/sys/devices/system/cpu/cpu{}/cpufreq/energy_performance_preference", .{i});
            try epp.append(allocator, try PersFd.init(epp_path, true));

            const cpu_freq_path = try std.fmt.bufPrint(&buf, "/sys/devices/system/cpu/cpu{}/cpufreq/scaling_cur_freq", .{i});
            try cpu_freq.append(allocator, try PersFd.init(cpu_freq_path, false));

            const min_cpu_freq_path = try std.fmt.bufPrint(&buf, "/sys/devices/system/cpu/cpu{}/cpufreq/scaling_min_freq", .{i});
            try min_cpu_freq.append(allocator, try PersFd.init(min_cpu_freq_path, true));

            const max_cpu_freq_path = try std.fmt.bufPrint(&buf, "/sys/devices/system/cpu/cpu{}/cpufreq/scaling_max_freq", .{i});
            try max_cpu_freq.append(allocator, try PersFd.init(max_cpu_freq_path, true));
        }

        return .{
            .cpu_core_count = n,
            .cpu_type = cpu_type,
            .scaling_governer = scaling_governer,
            .epp = epp,
            .cpu_turbo_boost = try PersFd.init("/sys/devices/system/cpu/cpufreq/boost", true),
            .min_cpu_freq = min_cpu_freq,
            .max_cpu_freq = max_cpu_freq,
            .cpu_freq = cpu_freq,
            .cpu_temp = try PersFd.init("/sys/class/thermal/thermal_zone0/temp", false),
            .cpu_load = try PersFd.init("/proc/stat", false),
            //.cpu_load = undefined,
            .cpu_power_draw = try PersFd.init("/sys/class/powercap/intel-rapl:0/energy_uj", false),
        };
    }

    pub fn deinit(self: *CpuStates, allocator: Allocator) void {
        for (self.scaling_governer.items,
            self.epp.items,
            self.min_cpu_freq.items,
            self.max_cpu_freq.items,
            self.cpu_freq.items) |*sg, *epp, *micf, *macf, *cf|
        {
            sg.deinit();
            epp.deinit();
            micf.deinit();
            macf.deinit();
            cf.deinit();
        }

        self.scaling_governer.deinit(allocator);
        self.epp.deinit(allocator);
        self.min_cpu_freq.deinit(allocator);
        self.max_cpu_freq.deinit(allocator);
        self.cpu_freq.deinit(allocator);

        self.cpu_turbo_boost.deinit();
        self.cpu_temp.deinit();
        self.cpu_load.deinit();
        self.cpu_power_draw.deinit();
    }

    pub fn print(self: *CpuStates) !void {
        const output =
        \\CPU:
        \\  cpu type: {any}
        \\  scaling governer: {any}
        \\  epp: {any}
        \\  cpu turbo boost: {d}
        \\  min/max cpu freq: {d}-{d} GHz
        \\  cpu freq: {d} GHz
        \\  cpu temp: {d}°C
        \\  cpu load: {d}%
        \\  cpu power draw: {d} W,
        ;
        std.debug.print(output, .{
            //try self.read_charging_status(),
            //try self.read_battery_capacity(),
            //try self.read_charge_start_threshold(),
            //try self.read_charge_stop_threshold(),
            //try self.read_total_power_draw(),
            //try self.read_platform_profile(),
        });
    }

    pub fn read_scaling_governer(self: *CpuStates) !ScalingGoverner {
        const gov =
            ScalingGoverner.from_string(try self.scaling_governer.items[0].borrow_mut().read_value());

        assert(gov != ScalingGoverner.Unknown);

        for (self.scaling_governer.items[1..]) |fd| {
            const val = ScalingGoverner.from_string(try fd.borrow_mut().read_value());
            assert(gov == val);
        }

        return gov;
    }

    pub fn set_scaling_governer(self: *CpuStates, sg: ScalingGoverner) !void {
        let write = match scaling_governer {
            ScalingGoverner::Powersave => POWERSAVE,
            ScalingGoverner::Performance => PERFORMANCE,
            _ => return Err(CpuStatesError::InvalidScalingGovVal),
        };

        //println!("Setting cpu performance preference to: {}", write);

        for fd in &self.scaling_governer {
            fd.borrow_mut().set_value(write)?;
        }

        Ok(())
    }

    pub fn read_epp(self: *CpuStates) !EPP {
        let gov = EPP::from_string(&self.epp[0].borrow_mut().read_value()?);
        assert_ne!(gov, EPP::Unknown, "EPP is not unknown");

        for fd in &self.epp[1..] {
            let val = EPP::from_string(&fd.borrow_mut().read_value()?);
            assert_eq!(gov, val, "EPP is the same for all cpu cores");
        }

        Ok(gov)
    }

    pub fn set_epp(self: *CpuStates, epp: EPP) !void {
        let write = match epp {
            EPP::EDefault => DEFAULT,
            EPP::Performance => PERFORMANCE,
            EPP::BalancePerformance => BALANCE_PERFORMANCE,
            EPP::BalancePower => BALANCE_POWER,
            EPP::Power => POWER,
            _ => return Err(CpuStatesError::InvalidEPPVal),
        };

        //println!("Setting CPU epp to: {}", write);

        for fd in &self.epp {
            fd.borrow_mut().set_value(write)?;
        }

        Ok(())
    }

    pub fn read_cpu_turbo_boost(self: *CpuStates) !u8 {
        let val = self
            .cpu_turbo_boost
            .borrow_mut()
            .read_value()?
            .parse::<u8>()?;
        Ok(val)
    }

    pub fn set_cpu_turbo_boost(self: *CpuStates, boost: u8) !void {
        self.cpu_turbo_boost
            .borrow_mut()
            .set_value(&boost.to_string())?;
        Ok(())
    }

    /// GHz
    pub fn read_avg_cpu_freq(self: *CpuStates) !f32 {
        let mut total: usize = 0;

        for fd in &self.cpu_freq {
            let val: String = fd.borrow_mut().read_value()?;
            total += val.parse::<usize>()?;
        }

        Ok(((total / self.cpu_core_count) as f32) / 1_000_000.0)
    }

    /// GHz
    pub fn read_min_cpu_freq(self: *CpuStates) !f32 {
        let prev: usize = self.min_cpu_freq[0].borrow_mut().read_value()?.parse()?;

        for fd in &self.min_cpu_freq[1..] {
            let val = fd.borrow_mut().read_value()?.clone().parse()?;
            assert_eq!(prev, val, "min_cpu_freq is the same for all cpu cores");
        }

        Ok((prev as f32) / 1_000_000.0)
    }

    //pub fn set_min_cpu_freq(&self) -> io::Result<usize> {}

    /// GHz
    pub fn read_max_cpu_freq(self: *CpuStates) !f32 {
        let prev: usize = self.max_cpu_freq[0].borrow_mut().read_value()?.parse()?;

        for fd in &self.max_cpu_freq[1..] {
            let val: usize = fd.borrow_mut().read_value()?.clone().parse()?;
            assert_eq!(prev, val, "max_cpu_freq is the same for all cpu cores");
        }

        Ok((prev as f32) / 1_000_000.0)
    }

    //pub fn set_max_cpu_freq(&mut self) -> io::Result<usize> {}

    /// celcius
    pub fn read_cpu_temp(self: *CpuStates) !usize {
        let temp: usize = self.cpu_temp.borrow_mut().read_value()?.parse()?;
        Ok(temp / 1000)
    }

    pub fn read_cpu_load(self: *CpuStates) !f64 {
        let proc_stat = self.cpu_load.borrow_mut().read_value()?;
        let line = proc_stat
            .lines()
            .next()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "Empty /proc/stat"))?;

        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 5 {
            return Err(CpuStatesError::GeneralIoErr(io::Error::new(
                io::ErrorKind::InvalidData,
                "Invalid /proc/stat format",
            )));
        }

        let prev: Vec<u64> = parts[1..]
            .iter()
            .map(|s| {
                s.parse()
                    .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "Parse error"))
            })
            .collect::<io::Result<Vec<_>>>()?;
        let prev_total: u64 = prev.iter().sum();
        let prev_idle = prev[3] + if prev.len() > 4 { prev[4] } else { 0 };

        thread::sleep(Duration::from_millis(250));

        let proc_stat = self.cpu_load.borrow_mut().read_value()?;
        let line = proc_stat
            .lines()
            .next()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "Empty /proc/stat"))?;

        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 5 {
            return Err(CpuStatesError::GeneralIoErr(io::Error::new(
                io::ErrorKind::InvalidData,
                "Invalid /proc/stat format",
            )));
        }

        let now: Vec<u64> = parts[1..]
            .iter()
            .map(|s| {
                s.parse()
                    .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "Parse error"))
            })
            .collect::<io::Result<Vec<_>>>()?;
        let now_total: u64 = now.iter().sum();
        let now_idle = now[3] + if now.len() > 4 { now[4] } else { 0 };

        let total_delta = (now_total as i64 - prev_total as i64).max(1) as u64;
        let idle_delta = now_idle as i64 - prev_idle as i64;

        let load_percent = if total_delta > 0 {
            let busy_delta = total_delta as i64 - idle_delta;
            (busy_delta.max(0) as f64 / total_delta as f64) * 100.0
        } else {
            0.0
        };

        Ok(load_percent)
    }

    pub fn read_cpu_power_draw(self: *CpuStates) !f32 {
        let start: u64 = self.cpu_power_draw.borrow_mut().read_value()?.parse()?;

        std::thread::sleep(std::time::Duration::from_secs_f32(0.5));

        let end: u64 = self.cpu_power_draw.borrow_mut().read_value()?.parse()?;

        let watts = (end - start) as f32 / 1_000_000.0;
        Ok(watts)
    }
};

pub const ChargingStatus = enum {
    Charging,
    DisCharging,
    Unknown,

    const CHARGING: []const u8 = "1";
    const DISCHARGING: []const u8 = "0";

    pub fn from_string(s: []const u8) ChargingStatus {
        if (std.mem.eql(u8, CHARGING, s)) {
            return ChargingStatus.Charging;
        } else if (std.mem.eql(u8, DISCHARGING, s)) {
            return ChargingStatus.DisCharging;
        } else {
            return ChargingStatus.Unknown;
        }
    }
};

pub const PlatformProfile = enum {
    LowPower,
    Balanced,
    Performance,
    Unknown,

    const LOW_POWER: []const u8 = "low-power";
    const BALANCED: []const u8 = "balanced";
    const PERFORMANCE: []const u8 = "performance";

    pub fn from_string(s: []const u8) PlatformProfile {
        if (std.mem.eql(u8, LOW_POWER, s)) {
            return PlatformProfile.LowPower;
        } else if (std.mem.eql(u8, BALANCED, s)) {
            return PlatformProfile.Balanced;
        } else if (std.mem.eql(u8, PERFORMANCE, s)) {
            return PlatformProfile.Performance;
        } else {
            return PlatformProfile.Unknown;
        }
    }

    pub fn to_string(pp: PlatformProfile) []const u8 {
        switch (pp) {
            PlatformProfile.LowPower => return PlatformProfile.LOW_POWER,
            PlatformProfile.Balanced => return PlatformProfile.BALANCED,
            PlatformProfile.Performance => return PlatformProfile.PERFORMANCE,
            PlatformProfile.Unknown => return PlatformProfile.BALANCED,
        }
    }
};

pub const BatteryStates = struct {
    battery_charging_status: PersFd,
    battery_capacity: PersFd,
    charge_start_threshold: PersFd,
    charge_stop_threshold: PersFd,
    total_power_draw: PersFd,
    platform_profile: PersFd,

    pub fn init() !BatteryStates {
        return .{
            .battery_charging_status =
                try BatteryStates.load_charging_status(),
            .battery_capacity =
                try PersFd.init("/sys/class/power_supply/BAT0/capacity", false),
            .charge_start_threshold =
                try PersFd.init("/sys/class/power_supply/BAT0/charge_start_threshold", true),
            .charge_stop_threshold =
                try PersFd.init("/sys/class/power_supply/BAT0/charge_stop_threshold", true),
            .total_power_draw =
                try PersFd.init("/sys/class/power_supply/BAT0/power_now", false),
            .platform_profile =
                try PersFd.init("/sys/firmware/acpi/platform_profile", true),
        };
    }

    pub fn deinit(self: *BatteryStates) void {
        self.battery_charging_status.deinit();
        self.battery_capacity.deinit();
        self.charge_start_threshold.deinit();
        self.charge_stop_threshold.deinit();
        self.total_power_draw.deinit();
        self.platform_profile.deinit();
    }

    pub fn print(self: *BatteryStates) !void {
        const output =
        \\Battery:
        \\  charging status: {any}
        \\  battery capacity: {d}%
        \\  charge start threshold: {d}%
        \\  charge stop threshold: {d}%
        \\  total power draw: {d} W
        \\  platform profile: {any}
        \\
        ;
        std.debug.print(output, .{
            try self.read_charging_status(),
            try self.read_battery_capacity(),
            try self.read_charge_start_threshold(),
            try self.read_charge_stop_threshold(),
            try self.read_total_power_draw(),
            try self.read_platform_profile(),
        });
    }

    fn load_charging_status() !PersFd {
        const base = "/sys/class/power_supply";
        var dir = try std.fs.openDirAbsolute(base, .{ .iterate = true });
        defer dir.close();

        var it = dir.iterate();
        while (try it.next()) |entry| {
            if (entry.kind != .sym_link) continue;

            if (std.mem.startsWith(u8, entry.name, "AC") or
                std.mem.startsWith(u8, entry.name, "ACAD"))
            {
                var power_dir = try dir.openDir(entry.name, .{});
                defer power_dir.close();

                power_dir.access("online", .{}) catch continue;

                var final_path: [128]u8 = undefined;
                const path_slice = try std.fmt.bufPrint(&final_path, "{s}/{s}/online", .{ base, entry.name });

                return PersFd.init(path_slice, false);
            }
        }

        return error.FileNotFound;
    }

    pub fn read_charging_status(self: *BatteryStates) !ChargingStatus {
        return ChargingStatus.from_string(try self.battery_charging_status.read_value());
    }

    pub fn read_battery_capacity(self: *BatteryStates) !usize {
        return std.fmt.parseInt(usize, try self.battery_capacity.read_value(), 10);
    }

    pub fn read_charge_start_threshold(self: *BatteryStates) !usize {
        return std.fmt.parseInt(usize, try self.charge_start_threshold.read_value(), 10);
    }

    pub fn set_charge_start_threshold(self: *BatteryStates, start: usize) !void {
        const buffer: [5]u8 = undefined;
        const string = try std.fmt.bufPrint(&buffer, "{d}", .{start});
        try self.charge_start_threshold.set_value(string);
    }

    pub fn read_charge_stop_threshold(self: *BatteryStates) !usize {
        return std.fmt.parseInt(usize, try self.charge_stop_threshold.read_value(), 10);
    }

    pub fn set_charge_stop_threshold(self: *BatteryStates, stop: usize) !void {
        const buffer: [5]u8 = undefined;
        const string = try std.fmt.bufPrint(&buffer, "{d}", .{stop});
        try self.charge_stop_threshold.set_value(string);
    }

    pub fn read_total_power_draw(self: *BatteryStates) !f32 {
        const power_uw = try std.fmt.parseFloat(f32, try self.total_power_draw.read_value());
        const watts = power_uw / 1_000_000.0;
        return watts;
    }

    pub fn read_platform_profile(self: *BatteryStates) !PlatformProfile {
        return PlatformProfile.from_string(try self.platform_profile.read_value());
    }

    pub fn set_platform_profile(self: *BatteryStates, pp: PlatformProfile) !void {
        try self.platform_profile.set_value(pp.to_string());
    }
};
