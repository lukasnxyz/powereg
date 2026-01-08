const std = @import("std");
const l_root = @import("root.zig");

const mem = std.mem;
const PersFd = l_root.PersFd;

pub const AcpiType = enum { ThinkPad, IdeaPad, Unknown };

pub const ChargingStatus = enum {
    Charging,
    DisCharging,
    NotCharging,
    Unknown,

    const CHARGING: []const u8 = "Charging";
    const DISCHARGING: []const u8 = "Discharging";
    const NOTCHARGING: []const u8 = "Not charging";

    pub fn fromString(s: []const u8) @This() {
        if (mem.eql(u8, CHARGING, s)) {
            return .Charging;
        } else if (mem.eql(u8, DISCHARGING, s)) {
            return .DisCharging;
        } else if (mem.eql(u8, NOTCHARGING, s)) {
            return .NotCharging;
        } else {
            return .Unknown;
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

    pub fn fromString(s: []const u8) @This() {
        if (mem.eql(u8, LOW_POWER, s)) {
            return .LowPower;
        } else if (mem.eql(u8, BALANCED, s)) {
            return .Balanced;
        } else if (mem.eql(u8, PERFORMANCE, s)) {
            return .Performance;
        } else {
            return .Unknown;
        }
    }

    pub fn toString(pp: PlatformProfile) []const u8 {
        switch (pp) {
            .LowPower => return PlatformProfile.LOW_POWER,
            .Balanced => return PlatformProfile.BALANCED,
            .Performance => return PlatformProfile.PERFORMANCE,
            .Unknown => return PlatformProfile.BALANCED,
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
        self.battery_charging_status.close();
        self.battery_capacity.close();
        self.charge_start_threshold.close();
        self.charge_stop_threshold.close();
        self.total_power_draw.close();
        self.platform_profile.close();
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
            try self.readChargingStatus(),
            try self.readBatteryCapacity(),
            try self.readChargeStopThreshold(),
            try self.readChargeStartThreshold(),
            try self.readTotalPowerDraw(),
            try self.readPlatformProfile(),
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

    pub fn readChargingStatus(self: *@This()) !ChargingStatus {
        return ChargingStatus.fromString(try self.battery_charging_status.readValue());
    }

    pub fn readBatteryCapacity(self: *@This()) !u8 {
        return std.fmt.parseInt(u8, try self.battery_capacity.readValue(), 10);
    }

    pub fn readChargeStopThreshold(self: *@This()) !u8 {
        return std.fmt.parseInt(u8, try self.charge_stop_threshold.readValue(), 10);
    }

    pub fn setChargeStopThreshold(self: *@This(), stop: u8) !void {
        var buffer: [5]u8 = undefined;
        const string = try std.fmt.bufPrint(&buffer, "{d}", .{stop});
        try self.charge_stop_threshold.setValue(string);
    }

    pub fn readChargeStartThreshold(self: *@This()) !u8 {
        return std.fmt.parseInt(u8, try self.charge_start_threshold.readValue(), 10);
    }

    pub fn setChargeStartThreshold(self: *@This(), start: u8) !void {
        var buffer: [5]u8 = undefined;
        const string = try std.fmt.bufPrint(&buffer, "{d}", .{start});
        try self.charge_start_threshold.setValue(string);
    }

    pub fn readTotalPowerDraw(self: *@This()) !f32 {
        const power_uw = try std.fmt.parseFloat(f32, try self.total_power_draw.readValue());
        const watts = power_uw / 1_000_000.0;
        return watts;
    }

    pub fn readPlatformProfile(self: *@This()) !PlatformProfile {
        return PlatformProfile.fromString(try self.platform_profile.readValue());
    }

    pub fn setPlatformProfile(self: *@This(), pp: PlatformProfile) !void {
        try self.platform_profile.setValue(pp.toString());
    }
};
