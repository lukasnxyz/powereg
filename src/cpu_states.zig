const std = @import("std");
const l_utils = @import("utils.zig");

const mem = std.mem;
const assert = std.debug.assert;
const PersFd = l_utils.PersFd;
const StrCol = l_utils.StrCol;

pub const N_CPUS = @import("build_options").cpu_count;
pub const CpuType = enum { AMD, Intel, Unknown };

pub const ScalingGoverner = enum {
    Powersave,
    Performance,
    Unknown,

    const PERFORMANCE: []const u8 = "performance";
    const POWERSAVE: []const u8 = "powersave";

    pub fn from_string(s: []const u8) @This() {
        if (mem.eql(u8, POWERSAVE, s)) {
            return ScalingGoverner.Powersave;
        } else if (mem.eql(u8, PERFORMANCE, s)) {
            return ScalingGoverner.Performance;
        } else {
            return ScalingGoverner.Unknown;
        }
    }
};

pub const AmdEPP = enum {
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

    pub fn from_string(s: []const u8) @This() {
        if (mem.eql(u8, DEFAULT, s)) {
            return AmdEPP.Default;
        } else if (mem.eql(u8, PERFORMANCE, s)) {
            return AmdEPP.Performance;
        } else if (mem.eql(u8, BALANCE_PERFORMANCE, s)) {
            return AmdEPP.BalancePerformance;
        } else if (mem.eql(u8, BALANCE_POWER, s)) {
            return AmdEPP.BalancePower;
        } else if (mem.eql(u8, POWER, s)) {
            return AmdEPP.Power;
        } else {
            return AmdEPP.Unknown;
        }
    }
};

pub const CpuStatesError = error{
    InvalidScalingGovVal,
    InvalidEPPVal,
    InvalidCpuCount,
    InvalidAMDPstate,
    UnsupportedCpuType,
};
pub const CpuStates = struct {
    cpu_type: CpuType,
    scaling_governer: [N_CPUS]PersFd,
    min_cpu_freq: [N_CPUS]PersFd,
    max_cpu_freq: [N_CPUS]PersFd,
    cpu_freq: [N_CPUS]PersFd,
    cpu_temp: PersFd,
    cpu_load: PersFd,
    cpu_boost: PersFd,

    amd_epp: ?[N_CPUS]PersFd,
    cpu_power_draw: ?PersFd,

    pub fn init(cpu_type: CpuType) !@This() {
        if (N_CPUS != try std.Thread.getCpuCount()) {
            std.debug.print("# of cpu cores on the build system not the same as on the run system!\n", .{});
            return error.InvalidCpuCount;
        }

        var available_asgr = try PersFd.init("/sys/devices/system/cpu/cpu0/cpufreq/scaling_available_governors", false);
        const asgr = try available_asgr.read_value();
        if ((mem.indexOf(u8, asgr, "performance") == null) or
            (mem.indexOf(u8, asgr, "powersave") == null))
        {
            std.debug.print("Incorrect available scaling governer options!", .{});
            return error.InvalidScalingGovVal;
        }

        var scaling_governer: [N_CPUS]PersFd = undefined;
        var cpu_freq: [N_CPUS]PersFd = undefined;
        var max_cpu_freq: [N_CPUS]PersFd = undefined;
        var min_cpu_freq: [N_CPUS]PersFd = undefined;

        var buf: [70]u8 = undefined;
        for (0..N_CPUS) |i| {
            const scaling_gov_path = try std.fmt.bufPrint(&buf, "/sys/devices/system/cpu/cpu{d}/cpufreq/scaling_governor", .{i});
            scaling_governer[i] = try PersFd.init(scaling_gov_path, true);

            const cpu_freq_path = try std.fmt.bufPrint(&buf, "/sys/devices/system/cpu/cpu{}/cpufreq/scaling_cur_freq", .{i});
            cpu_freq[i] = try PersFd.init(cpu_freq_path, false);

            const min_cpu_freq_path = try std.fmt.bufPrint(&buf, "/sys/devices/system/cpu/cpu{}/cpufreq/scaling_min_freq", .{i});
            min_cpu_freq[i] = try PersFd.init(min_cpu_freq_path, true);

            const max_cpu_freq_path = try std.fmt.bufPrint(&buf, "/sys/devices/system/cpu/cpu{}/cpufreq/scaling_max_freq", .{i});
            max_cpu_freq[i] = try PersFd.init(max_cpu_freq_path, true);
        }

        var amd_epp: ?[N_CPUS]PersFd = null;
        var cpu_power_draw: ?PersFd = null;
        if (cpu_type == CpuType.AMD) {
            var amd_pstate = try PersFd.init("/sys/devices/system/cpu/amd_pstate/status", true);
            const r_amd_pstate = try amd_pstate.read_value();
            if (mem.indexOf(u8, r_amd_pstate, "active") == null) {
                std.debug.print("amd_pstate is not active!\n", .{});
                std.debug.print("Attempting to set amd_pstate to 'active'\n", .{});
                amd_pstate.set_value("active") catch |e| {
                    std.debug.print("Failed setting amd_pstate to 'active': {any}\n", .{e});
                    return error.InvalidAMDPstate;
                };
            }

            var s_amd_epp: [N_CPUS]PersFd = undefined;
            for (0..N_CPUS) |i| {
                const epp_path = try std.fmt.bufPrint(&buf, "/sys/devices/system/cpu/cpu{}/cpufreq/energy_performance_preference", .{i});
                s_amd_epp[i] = try PersFd.init(epp_path, true);
            }
            amd_epp = s_amd_epp;

            cpu_power_draw = try PersFd.init("/sys/class/powercap/intel-rapl:0/energy_uj", false);
        } else if (cpu_type == CpuType.Intel) {
            return error.UnsupportedCpuType;
        } else {
            return error.UnsupportedCpuType;
        }

        return .{
            .cpu_type = cpu_type,
            .scaling_governer = scaling_governer,
            .min_cpu_freq = min_cpu_freq,
            .max_cpu_freq = max_cpu_freq,
            .cpu_freq = cpu_freq,
            .cpu_temp = try PersFd.init("/sys/class/thermal/thermal_zone0/temp", false),
            .cpu_load = try PersFd.init("/proc/stat", false),
            .cpu_boost = try PersFd.init("/sys/devices/system/cpu/cpufreq/boost", true),

            .amd_epp = amd_epp,
            .cpu_power_draw = cpu_power_draw,
        };
    }

    pub fn deinit(self: *@This()) void {
        for (0..N_CPUS) |i| {
            self.scaling_governer[i].deinit();
            self.min_cpu_freq[i].deinit();
            self.max_cpu_freq[i].deinit();
            self.cpu_freq[i].deinit();
        }
        if (self.amd_epp) |*amd_epp| {
            for (0..N_CPUS) |i| amd_epp[i].deinit();
        }

        self.cpu_boost.deinit();
        self.cpu_temp.deinit();
        self.cpu_load.deinit();

        if (self.cpu_power_draw) |*pdraw| pdraw.deinit();
    }

    pub fn print(self: *@This()) !void {
        const output =
            \\CPU:
            \\  cpu type: {any}
            \\  scaling governer: {any}
            \\  amd epp: {any}
            \\  cpu turbo boost: {any}
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
            try self.read_amd_epp(),
            try self.read_cpu_boost(),
            try self.read_min_cpu_freq(),
            try self.read_max_cpu_freq(),
            try self.read_avg_cpu_freq(),
            try self.read_cpu_temp(),
            try self.read_cpu_load(),
            try self.read_cpu_power_draw(),
        });
    }

    pub fn read_scaling_governer(self: *@This()) !ScalingGoverner {
        const gov = ScalingGoverner.from_string(try self.scaling_governer[0].read_value());
        assert(gov != ScalingGoverner.Unknown);

        for (1..N_CPUS) |i| {
            const val = ScalingGoverner.from_string(try self.scaling_governer[i].read_value());
            assert(gov == val);
        }

        return gov;
    }

    pub fn set_scaling_governer(self: *@This(), sg: ScalingGoverner) !void {
        const write = switch (sg) {
            ScalingGoverner.Powersave => ScalingGoverner.POWERSAVE,
            ScalingGoverner.Performance => ScalingGoverner.PERFORMANCE,
            else => return error.InvalidScalingGovVal,
        };

        for (0..N_CPUS) |i| {
            try self.scaling_governer[i].set_value(write);
        }
    }

    pub fn read_amd_epp(self: *@This()) !AmdEPP {
        if (self.amd_epp) |*amd_epp| {
            const gov = AmdEPP.from_string(try amd_epp[0].read_value());
            assert(gov != AmdEPP.Unknown);

            for (1..N_CPUS) |i|
                assert(gov == AmdEPP.from_string(try amd_epp[i].read_value()));

            return gov;
        }
        return AmdEPP.Unknown;
    }

    pub fn set_amd_epp(self: *@This(), epp: AmdEPP) !void {
        if (self.amd_epp) |*amd_epp| {
            const write = switch (epp) {
                AmdEPP.Default => AmdEPP.DEFAULT,
                AmdEPP.Performance => AmdEPP.PERFORMANCE,
                AmdEPP.BalancePerformance => AmdEPP.BALANCE_PERFORMANCE,
                AmdEPP.BalancePower => AmdEPP.BALANCE_POWER,
                AmdEPP.Power => AmdEPP.POWER,
                else => return error.InvalidEPPVal,
            };

            for (0..N_CPUS) |i|
                try amd_epp[i].set_value(write);
        } else {
            std.debug.print("{s}\n", .{StrCol.red("set_amd_epp: can't set because cpu_type != .AMD")});
            return;
        }
    }

    pub fn read_cpu_boost(self: *@This()) !bool {
        return try std.fmt.parseInt(u8, try self.cpu_boost.read_value(), 10) == 1;
    }

    pub fn set_cpu_boost(self: *@This(), boost: bool) !void {
        var buf: [3]u8 = undefined;
        const str = try std.fmt.bufPrint(&buf, "{}", .{@intFromBool(boost)});
        try self.cpu_boost.set_value(str);
    }

    // GHz
    pub fn read_avg_cpu_freq(self: *@This()) !f32 {
        var total: usize = 0;

        for (0..N_CPUS) |i| {
            const val = try self.cpu_freq[i].read_value();
            total += try std.fmt.parseInt(usize, val, 10);
        }

        return @as(f32, @floatFromInt(total / N_CPUS)) / 1_000_000.0;
    }

    // GHz
    pub fn read_min_cpu_freq(self: *@This()) !f32 {
        const prev = try std.fmt.parseInt(usize, try self.min_cpu_freq[0].read_value(), 10);

        for (1..N_CPUS) |i| {
            const val = try std.fmt.parseInt(usize, try self.min_cpu_freq[i].read_value(), 10);
            assert(val == prev);
        }

        return @as(f32, @floatFromInt(prev)) / 1_000_000.0;
    }

    // GHz
    pub fn read_max_cpu_freq(self: *@This()) !f32 {
        const prev = try std.fmt.parseInt(usize, try self.max_cpu_freq[0].read_value(), 10);

        for (1..N_CPUS) |i| {
            const val = try std.fmt.parseInt(usize, try self.max_cpu_freq[i].read_value(), 10);
            assert(val == prev);
        }

        return @as(f32, @floatFromInt(prev)) / 1_000_000.0;
    }

    // celcius
    pub fn read_cpu_temp(self: *@This()) !usize {
        const temp = try std.fmt.parseInt(usize, try self.cpu_temp.read_value(), 10);
        return temp / 1000;
    }

    // TODO: better way to do this?
    pub fn read_cpu_load(self: *@This()) !f64 {
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

        if (total_delta == 0) return 0.0;

        const load_percent = if (total_delta > 0) blk: {
            const busy_delta = total_delta - idle_delta;
            const busy_clamped = @max(busy_delta, 0);
            break :blk (@as(f64, @floatFromInt(busy_clamped)) / @as(f64, @floatFromInt(total_delta))) * 100.0;
        } else 0.0;

        return load_percent;
    }

    pub fn read_cpu_power_draw(self: *@This()) !f32 {
        if (self.cpu_power_draw) |*cpu_power_draw| {
            const start = try std.fmt.parseInt(u64, try cpu_power_draw.read_value(), 10);
            const start_time = std.time.milliTimestamp();

            std.Thread.sleep(500 * std.time.ns_per_ms);

            const end = try std.fmt.parseInt(u64, try cpu_power_draw.read_value(), 10);
            const end_time = std.time.milliTimestamp();

            // wrapped around or invalid read
            if (end < start) return 0.0;

            const energy_delta_uj = end - start;
            const time_delta_ms = @as(f32, @floatFromInt(end_time - start_time));

            // convert: (microjoules / milliseconds) * 1000 = milliwatts, then / 1000 = watts
            //  -> microjoules / milliseconds / 1000
            const watts = (@as(f32, @floatFromInt(energy_delta_uj)) / time_delta_ms) / 1000.0;

            return watts;
        }
        return 0.0;
    }
};
