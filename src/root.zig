const std = @import("std");
const builtin = @import("builtin");
const c = @cImport({
    @cInclude("libudev.h");
});

const mem = std.mem;
const OpenFlags = std.fs.File.OpenFlags;
const Allocator = std.mem.Allocator;
const assert = std.debug.assert;

pub const Event = enum {
    PowerInPlug,
    PowerUnPlug,

    PeriodicCheck,

    LowBattery,
    HighCpuTemp,
    HighCpuLoad,
    LoadNormalized,

    Unknown,
};

pub const EventPoller = struct {
    udev: *c.udev,
    monitor: *c.udev_monitor,
    last_periodic_check: std.time.Instant,
    periodic_interval_ns: u64,

    pub fn init(interval_s: u64) !EventPoller {
        const udev = c.udev_new() orelse return error.UdevInitFailed;
        errdefer _ = c.udev_unref(udev);
        const monitor = c.udev_monitor_new_from_netlink(udev, "udev")
            orelse return error.MonitorCreationFailed;
        errdefer _ = c.udev_monitor_unref(monitor);

        _ = c.udev_monitor_filter_add_match_subsystem_devtype(monitor, "power_supply", null);
        _ = c.udev_monitor_enable_receiving(monitor);

        return .{
            .udev = udev,
            .monitor = monitor,
            .last_periodic_check = try std.time.Instant.now(),
            .periodic_interval_ns = interval_s * std.time.ns_per_s,
        };
    }

    pub fn deinit(self: *EventPoller) void {
        _ = c.udev_monitor_unref(self.monitor);
        _ = c.udev_unref(self.udev);
    }

    pub fn poll_events(self: *EventPoller) !Event {
        const now = try std.time.Instant.now();
        const elapsed = now.since(self.last_periodic_check);

        if (elapsed >= self.periodic_interval_ns) {
            self.last_periodic_check = now;
            return Event.PeriodicCheck;
        }

        const diff = self.periodic_interval_ns - elapsed;
        const timeout_ms: i32 = @intCast(diff / std.time.ns_per_ms);

        const fd = c.udev_monitor_get_fd(self.monitor);
        var fds = [1]std.posix.pollfd{.{
            .fd = fd,
            .events = std.posix.POLL.IN,
            .revents = 0,
        }};

        const poll_res = try std.posix.poll(&fds, timeout_ms);

        if (poll_res > 0) {
            const dev = c.udev_monitor_receive_device(self.monitor);
            if (dev != null) {
                defer _ = c.udev_device_unref(dev);

                const action = c.udev_device_get_action(dev);
                if (action == null or !mem.eql(u8, mem.span(action), "change")) {
                    return Event.Unknown;
                }

                const name = c.udev_device_get_property_value(dev, "POWER_SUPPLY_NAME");
                if (name) |n| {
                    const name_str = mem.span(n);
                    if (is_ac_adapter(name_str)) {
                        const online = c.udev_device_get_property_value(dev, "POWER_SUPPLY_ONLINE");
                        if (online) |o| {
                            const online_str = mem.span(o);
                            if (mem.eql(u8, online_str, "1")) return Event.PowerInPlug;
                            if (mem.eql(u8, online_str, "0")) return Event.PowerUnPlug;
                        }
                    }
                }
            }
        }

        return Event.Unknown;
    }

    fn is_ac_adapter(name: []const u8) bool {
        const adapters = [_][]const u8{ "ACAD", "AC", "ADP1", "AC0" };
        for (adapters) |a| {
            if (mem.eql(u8, name, a)) return true;
        }
        return false;
    }

    fn state_transition(event: Event, system_state: *SystemState) void {
        const old_state = system_state.state;
        system_state.state = switch (old_state) {
            .Performance => switch (event) {
                .PowerInPlug => .Performance,
                .PowerUnPlug => .Powersave,
                .LowBattery => .Powersave,

                .HighCpuTemp => .Balanced,
                .HighCpuLoad => .Balanced,

                else => old_state,
            },
            .Balanced => switch (event) {
                .PowerInPlug => .Performance,
                .PowerUnPlug => .Powersave,
                .LowBattery => .Powersave,

                .LoadNormalized => .Performance,

                else => old_state,
            },
            .Powersave => switch (event) {
                .PowerInPlug => .Performance,
                .PowerUnPlug => .Powersave,
                .LowBattery => .Powersave,

                else => old_state,
            },
        };
    }

    fn periodic_check(system_state: *SystemState) !Event {
        const low_battery_level = try system_state.battery_states.read_battery_capacity() <= 25;
        const high_cpu_temp = try system_state.cpu_states.read_cpu_temp() >= 85;
        const high_cpu_load = try system_state.cpu_states.read_cpu_load() >= 85.0;
        const is_plugged_in = try system_state.battery_states.read_charging_status() == .Charging;
        const current_state = system_state.state;

        const event = if (low_battery_level)
            Event.LowBattery
        else if (!is_plugged_in and (current_state == .Performance or current_state == .Balanced))
            Event.PowerUnPlug
        else if (is_plugged_in and current_state == .Powersave)
            Event.PowerInPlug
        else if (high_cpu_temp or high_cpu_load)
            Event.HighCpuLoad
        else if (is_plugged_in and current_state == .Balanced)
            Event.LoadNormalized
        else
            Event.Unknown;

        return event;
    }

    pub fn handle_event(i_event: Event, system_state: *SystemState) !void {
        const event = EventPoller.periodic_check(system_state) catch i_event;
        EventPoller.state_transition(event, system_state);
        switch (system_state.state) {
            State.Powersave => try system_state.set_powersave_mode(),
            State.Balanced => try system_state.set_balanced_mode(),
            State.Performance => try system_state.set_performance_mode(),
        }
    }
};

const PersFdError = error{
    InvalidFilePermsErr,
};

const PersFd = struct {
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
        return mem.trim(u8, raw_content, &std.ascii.whitespace);
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
        const cpu_type = SystemState.detect_cpu_type();
        return .{
            .linux = SystemState.detect_linux(),
            .cpu_type = cpu_type,
            .acpi_type = try SystemState.detect_acpi_type(),
            .cpu_states = try CpuStates.init(allocator, n, cpu_type),
            .battery_states = try BatteryStates.init(),
            .state = State.Powersave,
        };
    }

    pub fn post_init(self: *SystemState) !void {
        const status = try self.battery_states.read_charging_status();
        switch (status) {
            ChargingStatus.Charging => return self.set_powersave_mode(),
            ChargingStatus.DisCharging => return self.set_balanced_mode(),
            ChargingStatus.Unknown => return self.set_performance_mode(),
        }
    }

    pub fn deinit(self: *SystemState, allocator: Allocator) void {
        self.battery_states.deinit();
        self.cpu_states.deinit(allocator);
    }

    pub fn print(self: *SystemState) !void {
        std.debug.print("", .{});

        try self.cpu_states.print();
        try self.battery_states.print();
    }

    pub fn set_powersave_mode(self: *SystemState) !void {
        try self.cpu_states.set_scaling_governer(ScalingGoverner.Powersave);
        try self.cpu_states.set_epp(EPP.Power);
        try self.battery_states.set_platform_profile(PlatformProfile.LowPower);
        try self.cpu_states.set_cpu_turbo_boost(0);
    }

    pub fn set_balanced_mode(self: *SystemState) !void {
        if (try self.battery_states.read_charging_status() != ChargingStatus.Charging) {
            try self.cpu_states.set_scaling_governer(ScalingGoverner.Powersave);
            try self.cpu_states.set_epp(EPP.Power);
            try self.battery_states.set_platform_profile(PlatformProfile.LowPower);
        } else {
            try self.cpu_states.set_scaling_governer(ScalingGoverner.Powersave);
            try self.cpu_states.set_epp(EPP.BalancePower);
            try self.battery_states.set_platform_profile(PlatformProfile.Balanced);
        }

        try self.cpu_states.set_cpu_turbo_boost(0);
    }

    pub fn set_performance_mode(self: *SystemState) !void {
        if (try self.battery_states.read_charging_status() == ChargingStatus.DisCharging)
            return;

        try self.cpu_states.set_scaling_governer(ScalingGoverner.Performance);
        try self.cpu_states.set_epp(EPP.Performance);
        try self.battery_states.set_platform_profile(PlatformProfile.Performance);
        try self.cpu_states.set_cpu_turbo_boost(1);
    }

    fn detect_linux() bool {
        const compile_time = if (builtin.os.tag == .linux) true else false;
        const proc_exists = if (std.fs.cwd().access("/proc", .{})) true else |_| false;
        const sys_exists = if (std.fs.cwd().access("/sys", .{})) true else |_| false;
        const etc_exists = if (std.fs.cwd().access("/etc", .{})) true else |_| false;

        const etc_os_release = if (std.fs.cwd().access("/etc/os-release", .{})) true else |_| false;
        const usr_os_release = if (std.fs.cwd().access("/usr/lib/os-release", .{})) true else |_| false;
        const has_os_release = etc_os_release or usr_os_release;

        return compile_time or (proc_exists and sys_exists) or (proc_exists and sys_exists and etc_exists and has_os_release);
    }

    fn detect_cpu_type() CpuType {
        var fd = PersFd.init("/proc/cpuinfo", false) catch
            return CpuType.Unknown;
        const val = fd.read_value() catch
            return CpuType.Unknown;

        var line_iter = mem.splitScalar(u8, val, '\n');
        while (line_iter.next()) |line| {
            if (mem.startsWith(u8, line, "vendor_id")) {
                if (mem.indexOf(u8, line, "GenuineIntel") != null) {
                    return .Intel;
                } else if (mem.indexOf(u8, line, "AuthenticAMD") != null) {
                    return .AMD;
                }
            }
        }

        if (detect_cpu_via_cpuid()) |cpu_type|
            return cpu_type;

        return CpuType.Unknown;
    }

    fn detect_cpu_via_cpuid() ?CpuType {
        if (builtin.cpu.arch != .x86 or builtin.cpu.arch != .x86_64)
            return null;

        var eax: u32 = undefined;
        var ebx: u32 = undefined;
        var ecx: u32 = undefined;
        var edx: u32 = undefined;

        // cpuid(eax=0) -> vendor id in ebx, ecx, edx
        asm volatile ("cpuid"
            : [_eax] "={eax}" (eax),
              [_ebx] "={ebx}" (ebx),
              [_ecx] "={ecx}" (ecx),
              [_edx] "={edx}" (edx),
            : [leaf] "{eax}" (@as(u32, 0)),
            : .{ .memory = true }
        );

        var vendor: [12]u8 = undefined;
        @memcpy(vendor[0..4], mem.asBytes(&ebx));
        @memcpy(vendor[4..8], mem.asBytes(&edx));
        @memcpy(vendor[8..12], mem.asBytes(&ecx));

        if (mem.eql(u8, &vendor, "GenuineIntel")) {
            return CpuType.Intel;
        } else if (mem.eql(u8, &vendor, "AuthenticAMD")) {
            return CpuType.AMD;
        }

        return null;
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

            if (mem.startsWith(u8, name, "cpu") and !mem.eql(u8, name, "cpufreq")) {
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
            const trimmed = mem.trim(u8, product_version, &std.ascii.whitespace);
            var lowered: [pv.buffer.len]u8 = undefined;
            _ = std.ascii.lowerString(&lowered, trimmed);

            if (mem.indexOf(u8, &lowered, thinkpad) != null) return AcpiType.ThinkPad;
            if (mem.indexOf(u8, &lowered, ideapad) != null) return AcpiType.IdeaPad;
        } else |_| {}

        var pn = try PersFd.init("/sys/class/dmi/id/product_name", false);
        defer pn.deinit();
        if (pn.read_value()) |product_name| {
            const trimmed = mem.trim(u8, product_name, &std.ascii.whitespace);
            var lowered: [pv.buffer.len]u8 = undefined;
            _ = std.ascii.lowerString(&lowered, trimmed);
            if (mem.indexOf(u8, &lowered, thinkpad) != null) return AcpiType.ThinkPad;
            if (mem.indexOf(u8, &lowered, ideapad) != null) return AcpiType.IdeaPad;
        } else |_| {}

        if (std.fs.cwd().access("/proc/acpi/ibm", .{})) {
            return AcpiType.ThinkPad;
        } else |_| {}

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
        if (mem.eql(u8, POWERSAVE, s)) {
            return ScalingGoverner.Powersave;
        } else if (mem.eql(u8, PERFORMANCE, s)) {
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
        if (mem.eql(u8, DEFAULT, s)) {
            return EPP.Default;
        } else if (mem.eql(u8, PERFORMANCE, s)) {
            return EPP.Performance;
        } else if (mem.eql(u8, BALANCE_PERFORMANCE, s)) {
            return EPP.BalancePerformance;
        } else if (mem.eql(u8, BALANCE_POWER, s)) {
            return EPP.BalancePower;
        } else if (mem.eql(u8, POWER, s)) {
            return EPP.Power;
        } else {
            return EPP.Unknown;
        }
    }
};

pub const CpuStatesError = error{
    InvalidScalingGovVal,
    InvalidEPPVal,
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
    cpu_load: PersFd, // TODO: possibly wrong
    cpu_power_draw: PersFd, // TODO: possibly wrong

    // TODO: a good way to split this would be via just having strings for amd and intel
    //      for specific paths and having those be dynamic
    pub fn init(allocator: Allocator, n: usize, cpu_type: CpuType) !CpuStates {
        var available_scaling_govs =
            try PersFd.init("/sys/devices/system/cpu/cpu0/cpufreq/scaling_available_governors", false);
        var amd_pstate_status =
            try PersFd.init("/sys/devices/system/cpu/amd_pstate/status", false);

        if (!mem.eql(u8, try available_scaling_govs.read_value(), "performance powersave")) {
            std.debug.print("correct options for scaling governers", .{});
            return error.InvalidScalingGovVal;
        }

        if (!mem.eql(u8, try amd_pstate_status.read_value(), "active")) {
            std.debug.print("amd_pstate is active", .{});
            return error.InvalidScalingGovVal;
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
        for (self.scaling_governer.items, self.epp.items, self.min_cpu_freq.items, self.max_cpu_freq.items, self.cpu_freq.items) |*sg, *epp, *micf, *macf, *cf| {
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
            \\  cpu turbo boost: {d:.2}
            \\  min/max cpu freq: {d:.2}-{d:.2} GHz
            \\  cpu freq: {d:.2} GHz
            \\  cpu temp: {d:.2}°C
            \\  cpu load: {d:.2}%
            \\  cpu power draw: {d:.2} W
            \\
        ;
        std.debug.print(output, .{
            self.cpu_type,
            try self.read_scaling_governer(),
            try self.read_epp(),
            try self.read_cpu_turbo_boost(),
            try self.read_min_cpu_freq(),
            try self.read_max_cpu_freq(),
            try self.read_avg_cpu_freq(),
            try self.read_cpu_temp(),
            try self.read_cpu_load(),
            try self.read_cpu_power_draw(),
        });
    }

    pub fn read_scaling_governer(self: *CpuStates) !ScalingGoverner {
        const gov =
            ScalingGoverner.from_string(try self.scaling_governer.items[0].read_value());

        assert(gov != ScalingGoverner.Unknown);

        for (self.scaling_governer.items[1..]) |*fd| {
            const val = ScalingGoverner.from_string(try fd.read_value());
            assert(gov == val);
        }

        return gov;
    }

    pub fn set_scaling_governer(self: *CpuStates, sg: ScalingGoverner) !void {
        const write = switch (sg) {
            ScalingGoverner.Powersave => ScalingGoverner.POWERSAVE,
            ScalingGoverner.Performance => ScalingGoverner.PERFORMANCE,
            else => return error.InvalidScalingGovVal,
        };

        for (self.scaling_governer.items) |*fd| {
            try fd.set_value(write);
        }
    }

    pub fn read_epp(self: *CpuStates) !EPP {
        const gov = EPP.from_string(try self.epp.items[0].read_value());
        assert(gov != EPP.Unknown);

        for (self.epp.items[1..]) |*fd| {
            const val = EPP.from_string(try fd.read_value());
            assert(gov == val);
        }

        return gov;
    }

    pub fn set_epp(self: *CpuStates, epp: EPP) !void {
        const write = switch (epp) {
            EPP.Default => EPP.DEFAULT,
            EPP.Performance => EPP.PERFORMANCE,
            EPP.BalancePerformance => EPP.BALANCE_PERFORMANCE,
            EPP.BalancePower => EPP.BALANCE_POWER,
            EPP.Power => EPP.POWER,
            else => return error.InvalidEPPVal,
        };

        for (self.epp.items) |*fd| {
            try fd.set_value(write);
        }
    }

    pub fn read_cpu_turbo_boost(self: *CpuStates) !u8 {
        return std.fmt.parseInt(u8, try self.cpu_turbo_boost.read_value(), 10);
    }

    pub fn set_cpu_turbo_boost(self: *CpuStates, boost: u8) !void {
        var buf: [3]u8 = undefined;
        const str = try std.fmt.bufPrint(&buf, "{}", .{boost});
        try self.cpu_turbo_boost.set_value(str);
    }

    // GHz
    pub fn read_avg_cpu_freq(self: *CpuStates) !f32 {
        var total: usize = 0;

        for (self.cpu_freq.items) |*fd| {
            const val = try fd.read_value();
            total += try std.fmt.parseInt(usize, val, 10);
        }

        return @as(f32, @floatFromInt(total / self.cpu_core_count)) / 1_000_000.0;
    }

    // GHz
    pub fn read_min_cpu_freq(self: *CpuStates) !f32 {
        const prev = try std.fmt.parseInt(usize, try self.min_cpu_freq.items[0].read_value(), 10);

        for (self.min_cpu_freq.items[1..]) |*fd| {
            const val = try std.fmt.parseInt(usize, try fd.read_value(), 10);
            assert(val == prev);
        }

        return @as(f32, @floatFromInt(prev)) / 1_000_000.0;
    }

    ////pub fn set_min_cpu_freq(&self) -> io::Result<usize> {}

    // GHz
    pub fn read_max_cpu_freq(self: *CpuStates) !f32 {
        const prev = try std.fmt.parseInt(usize, try self.max_cpu_freq.items[0].read_value(), 10);

        for (self.max_cpu_freq.items[1..]) |*fd| {
            const val = try std.fmt.parseInt(usize, try fd.read_value(), 10);
            assert(val == prev);
        }

        return @as(f32, @floatFromInt(prev)) / 1_000_000.0;
    }

    ////pub fn set_max_cpu_freq(&mut self) -> io::Result<usize> {}

    // celcius
    pub fn read_cpu_temp(self: *CpuStates) !usize {
        const temp = try std.fmt.parseInt(usize, try self.cpu_temp.read_value(), 10);
        return temp / 1000;
    }

    pub fn read_cpu_load(self: *CpuStates) !f64 {
        const proc_stat = try self.cpu_load.read_value();
        var line_iter = mem.splitScalar(u8, proc_stat, '\n');
        const line = line_iter.next() orelse
            return error.EmptyProcStat;

        var parts = mem.tokenizeAny(u8, line, " \t");
        _ = parts.next(); // skip first field (cpu label)

        var prev: [10]u64 = undefined;
        var prev_len: usize = 0;
        while (parts.next()) |part| {
            if (prev_len >= prev.len) return error.TooManyFields;
            const val = std.fmt.parseInt(u64, part, 10) catch
                return error.ParseError;
            prev[prev_len] = val;
            prev_len += 1;
        }

        if (prev_len < 4) {
            return error.InvalidProcStatFormat;
        }

        var prev_total: u64 = 0;
        for (prev[0..prev_len]) |val| {
            prev_total += val;
        }

        const prev_idle = prev[3] + if (prev_len > 4) prev[4] else 0;

        std.Thread.sleep(200 * std.time.ns_per_ms);

        const proc_stat2 = try self.cpu_load.read_value();
        var line_iter2 = mem.splitScalar(u8, proc_stat2, '\n');
        const line2 = line_iter2.next() orelse
            return error.EmptyProcStat;

        var parts2 = mem.tokenizeAny(u8, line2, " \t");
        _ = parts2.next(); // skip first field

        var now: [10]u64 = undefined;
        var now_len: usize = 0;
        while (parts2.next()) |part| {
            if (now_len >= now.len) return error.TooManyFields;
            const val = std.fmt.parseInt(u64, part, 10) catch
                return error.ParseError;
            now[now_len] = val;
            now_len += 1;
        }

        if (now_len < 4) {
            return error.InvalidProcStatFormat;
        }

        var now_total: u64 = 0;
        for (now[0..now_len]) |val| {
            now_total += val;
        }

        const now_idle = now[3] + if (now_len > 4) now[4] else 0;

        const total_delta = @max(@as(i64, @intCast(now_total)) - @as(i64, @intCast(prev_total)), 1);
        const idle_delta = @as(i64, @intCast(now_idle)) - @as(i64, @intCast(prev_idle));

        const load_percent = if (total_delta > 0) blk: {
            const busy_delta = total_delta - idle_delta;
            const busy_clamped = @max(busy_delta, 0);
            break :blk (@as(f64, @floatFromInt(busy_clamped)) / @as(f64, @floatFromInt(total_delta))) * 100.0;
        } else 0.0;

        return load_percent;
    }

    pub fn read_cpu_power_draw(self: *CpuStates) !f32 {
        const start = try std.fmt.parseInt(u64, try self.cpu_power_draw.read_value(), 10);

        std.Thread.sleep(500 * std.time.ns_per_ms);

        const end = try std.fmt.parseInt(u64, try self.cpu_power_draw.read_value(), 10);

        const watts = @as(f32, @floatFromInt(end - start)) / 1_000_000.0;
        return watts;
    }
};

pub const ChargingStatus = enum {
    Charging,
    DisCharging,
    Unknown,

    const CHARGING: []const u8 = "1";
    const DISCHARGING: []const u8 = "0";

    pub fn from_string(s: []const u8) ChargingStatus {
        if (mem.eql(u8, CHARGING, s)) {
            return ChargingStatus.Charging;
        } else if (mem.eql(u8, DISCHARGING, s)) {
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
        if (mem.eql(u8, LOW_POWER, s)) {
            return PlatformProfile.LowPower;
        } else if (mem.eql(u8, BALANCED, s)) {
            return PlatformProfile.Balanced;
        } else if (mem.eql(u8, PERFORMANCE, s)) {
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
            .battery_charging_status = try BatteryStates.load_charging_status(),
            .battery_capacity = try PersFd.init("/sys/class/power_supply/BAT0/capacity", false),
            .charge_start_threshold = try PersFd.init("/sys/class/power_supply/BAT0/charge_start_threshold", true),
            .charge_stop_threshold = try PersFd.init("/sys/class/power_supply/BAT0/charge_stop_threshold", true),
            .total_power_draw = try PersFd.init("/sys/class/power_supply/BAT0/power_now", false),
            .platform_profile = try PersFd.init("/sys/firmware/acpi/platform_profile", true),
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
            \\  battery capacity: {d:.2}%
            \\  charge start threshold: {d:.2}%
            \\  charge stop threshold: {d:.2}%
            \\  total power draw: {d:.2} W
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

            if (mem.startsWith(u8, entry.name, "AC") or
                mem.startsWith(u8, entry.name, "ACAD"))
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
