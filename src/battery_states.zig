const std = @import("std");
const l_utils = @import("utils.zig");

const mem = std.mem;
const PersFd = l_utils.PersFd;

pub const AcpiType = enum { ThinkPad, IdeaPad, Unknown };

pub const ChargingStatus = enum {
    Charging,
    DisCharging,
    NotCharging,
    Unknown,

    const CHARGING: []const u8 = "Charging";
    const DISCHARGING: []const u8 = "Discharging";
    const NOTCHARGING: []const u8 = "Not charging";

    pub fn from_string(s: []const u8) @This() {
        if (mem.eql(u8, CHARGING, s)) {
            return ChargingStatus.Charging;
        } else if (mem.eql(u8, DISCHARGING, s)) {
            return ChargingStatus.DisCharging;
        } else if (mem.eql(u8, NOTCHARGING, s)) {
            return ChargingStatus.NotCharging;
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

    pub fn from_string(s: []const u8) @This() {
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
    charge_stop_threshold: PersFd,
    charge_start_threshold: PersFd,
    total_power_draw: PersFd,
    platform_profile: PersFd,

    pub fn init() !@This() {
        // TODO: search /sys/class/power_supply/ and search for BAT* and choose the highest number
        //      note: doesn't work for something like thinkpad with 2 batteries
        //      note: if no BAT* then means on desktop and should crash program with according error
        return .{
            .battery_charging_status = try PersFd.init("/sys/class/power_supply/BAT0/status", false),
            .battery_capacity = try PersFd.init("/sys/class/power_supply/BAT0/capacity", false),
            .charge_stop_threshold = try PersFd.init("/sys/class/power_supply/BAT0/charge_stop_threshold", true),
            .charge_start_threshold = try PersFd.init("/sys/class/power_supply/BAT0/charge_start_threshold", true),
            .total_power_draw = try PersFd.init("/sys/class/power_supply/BAT0/power_now", false),
            .platform_profile = try PersFd.init("/sys/firmware/acpi/platform_profile", true),
        };
    }

    pub fn deinit(self: *@This()) void {
        self.battery_charging_status.deinit();
        self.battery_capacity.deinit();
        self.charge_start_threshold.deinit();
        self.charge_stop_threshold.deinit();
        self.total_power_draw.deinit();
        self.platform_profile.deinit();
    }

    pub fn print(self: *@This()) !void {
        const output =
            \\Battery:
            \\  charging status: {any}
            \\  battery capacity: {d:.2}%
            \\  charge stop threshold: {d:.2}%
            \\  charge start threshold: {d:.2}%
            \\  total power draw: {d:.2} W
            \\  platform profile: {any}
            \\
        ;

        std.debug.print(output, .{
            try self.read_charging_status(),
            try self.read_battery_capacity(),
            try self.read_charge_stop_threshold(),
            try self.read_charge_start_threshold(),
            try self.read_total_power_draw(),
            try self.read_platform_profile(),
        });
    }

    //fn load_charging_status() !PersFd {
    //    const base = "/sys/class/power_supply";
    //    var dir = try std.fs.openDirAbsolute(base, .{ .iterate = true });
    //    defer dir.close();

    //    var it = dir.iterate();
    //    while (try it.next()) |entry| {
    //        if (entry.kind != .sym_link) continue;

    //        if (mem.startsWith(u8, entry.name, "AC") or
    //            mem.startsWith(u8, entry.name, "ACAD"))
    //        {
    //            var power_dir = try dir.openDir(entry.name, .{});
    //            defer power_dir.close();

    //            power_dir.access("online", .{}) catch continue;

    //            var final_path: [128]u8 = undefined;
    //            const path_slice = try std.fmt.bufPrint(&final_path, "{s}/{s}/online", .{ base, entry.name });

    //            return PersFd.init(path_slice, false);
    //        }
    //    }

    //    return error.FileNotFound;
    //}

    pub fn read_charging_status(self: *@This()) !ChargingStatus {
        return ChargingStatus.from_string(try self.battery_charging_status.read_value());
    }

    pub fn read_battery_capacity(self: *@This()) !u8 {
        return std.fmt.parseInt(u8, try self.battery_capacity.read_value(), 10);
    }

    pub fn read_charge_stop_threshold(self: *@This()) !u8 {
        return std.fmt.parseInt(u8, try self.charge_stop_threshold.read_value(), 10);
    }

    pub fn set_charge_stop_threshold(self: *@This(), stop: u8) !void {
        var buffer: [5]u8 = undefined;
        const string = try std.fmt.bufPrint(&buffer, "{d}", .{stop});
        try self.charge_stop_threshold.set_value(string);
    }

    pub fn read_charge_start_threshold(self: *@This()) !u8 {
        return std.fmt.parseInt(u8, try self.charge_start_threshold.read_value(), 10);
    }

    pub fn set_charge_start_threshold(self: *@This(), start: u8) !void {
        var buffer: [5]u8 = undefined;
        const string = try std.fmt.bufPrint(&buffer, "{d}", .{start});
        try self.charge_start_threshold.set_value(string);
    }

    pub fn read_total_power_draw(self: *@This()) !f32 {
        const power_uw = try std.fmt.parseFloat(f32, try self.total_power_draw.read_value());
        const watts = power_uw / 1_000_000.0;
        return watts;
    }

    pub fn read_platform_profile(self: *@This()) !PlatformProfile {
        return PlatformProfile.from_string(try self.platform_profile.read_value());
    }

    pub fn set_platform_profile(self: *@This(), pp: PlatformProfile) !void {
        try self.platform_profile.set_value(pp.to_string());
    }
};
