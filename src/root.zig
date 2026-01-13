const std = @import("std");
const builtin = @import("builtin");
const l_system_state = @import("system_state.zig");
const l_battery_states = @import("battery_states.zig");
const l_cpu_states = @import("cpu_states.zig");
const l_events = @import("events.zig");

pub const Allocator = std.mem.Allocator;
pub const SystemState = l_system_state.SystemState;
pub const SystemStateError = l_system_state.SystemStateError;
pub const AcpiType = l_battery_states.AcpiType;
pub const CpuType = l_cpu_states.CpuType;
pub const EventPoller = l_events.EventPoller;
const mem = std.mem;
const OpenFlags = std.fs.File.OpenFlags;

pub const PersFdError = error{InvalidFilePermsErr};
pub const PersFd = struct {
    file: std.fs.File,
    write: bool,
    buffer: [512]u8 = undefined,

    pub fn init(path: []const u8, write: bool) !@This() {
        const flags = if (write) OpenFlags{ .mode = .read_write } else OpenFlags{ .mode = .read_only };
        const file = try std.fs.cwd().openFile(path, flags);
        return PersFd{
            .file = file,
            .write = write,
        };
    }

    pub fn close(self: *@This()) void {
        @memset(&self.buffer, 0);
        self.file.close();
    }

    pub fn readValue(self: *@This()) ![]const u8 {
        try self.file.seekTo(0);
        @memset(&self.buffer, 0);
        const bytes_read = try self.file.readAll(&self.buffer);
        const raw_content = self.buffer[0..bytes_read];
        return mem.trim(u8, raw_content, &std.ascii.whitespace);
    }

    pub fn setValue(self: @This(), value: []const u8) !void {
        if (!self.write) {
            return error.InvalidFilePermsErr;
        }

        try self.file.setEndPos(0);
        try self.file.writeAll(value);
        try self.file.sync();
    }
};

pub const ConfigError = error{InvalidThresholds};
pub const Config = struct {
    battery_stop_threshold: u8,
    battery_start_threshold: u8,

    pub fn init(allocator: Allocator) !@This() {
        var buffer: [128]u8 = undefined;
        const path = try Config.getConfigPath(allocator, &buffer);
        std.debug.print("path: {s}\n", .{path});
        return try parseFile(allocator, path);
    }

    fn getConfigPath(allocator: Allocator, buffer: []u8) ![]u8 {
        var env_map = try std.process.getEnvMap(allocator);
        defer env_map.deinit();

        if (env_map.get("SUDO_USER")) |sudo_user| {
            return try std.fmt.bufPrint(buffer, "/home/{s}/.config/powereg/powereg.conf", .{sudo_user});
        }

        if (env_map.get("HOME")) |home| {
            return try std.fmt.bufPrint(buffer, "{s}/.config/powereg/powereg.conf", .{home});
        }

        return error.EnvironmentError;
    }

    fn parseFile(allocator: Allocator, path: []const u8) !@This() {
        var fd = try PersFd.init(path, false);
        defer fd.close();
        const text = try fd.readValue();

        const parsed = try std.json.parseFromSlice(@This(), allocator, text, .{});
        defer parsed.deinit();

        return parsed.value;
    }

    pub fn apply(self: @This(), system_state: *SystemState) !void {
        if (system_state.acpi_type != AcpiType.ThinkPad)
            return SystemStateError.InvalidAcpiType;

        if (self.battery_start_threshold >= self.battery_stop_threshold)
            return ConfigError.InvalidThresholds;

        if (self.battery_start_threshold > 100 or
            self.battery_start_threshold < 0 or
            self.battery_stop_threshold > 100 or
            self.battery_stop_threshold < 0
        )
            return ConfigError.InvalidThresholds;

        try system_state.battery_states.setChargeStopThreshold(self.battery_stop_threshold);
        std.debug.print("Battery charge stop threshold set to {}\n", .{self.battery_stop_threshold});

        try system_state.battery_states.setChargeStartThreshold(self.battery_start_threshold);
        std.debug.print("Battery charge start threshold set to {}\n", .{self.battery_start_threshold});
    }
};

pub const StrCol = struct {
    const reset = "\x1b[0m";
    const red_code = "\x1b[31m";
    const green_code = "\x1b[32m";
    const yellow_code = "\x1b[33m";

    pub fn red(comptime s: []const u8) []const u8 {
        return red_code ++ s ++ reset;
    }

    pub fn green(comptime s: []const u8) []const u8 {
        return green_code ++ s ++ reset;
    }

    pub fn yellow(comptime s: []const u8) []const u8 {
        return yellow_code ++ s ++ reset;
    }
};
