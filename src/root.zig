const std = @import("std");
const builtin = @import("builtin");
const l_utils = @import("utils.zig");
const l_system_state = @import("system_state.zig");
const l_battery_states = @import("battery_states.zig");
const l_cpu_states = @import("cpu_states.zig");
const l_events = @import("events.zig");

pub const Allocator = std.mem.Allocator;
pub const PersFd = l_utils.PersFd;
pub const SystemState = l_system_state.SystemState;
pub const SystemStateError = l_system_state.SystemStateError;
pub const AcpiType = l_battery_states.AcpiType;
pub const StrCol = l_utils.StrCol;
pub const CpuType = l_cpu_states.CpuType;
pub const EventPoller = l_events.EventPoller;

pub const ConfigError = error { InvalidThresholds };
pub const Config = struct {
    battery_stop_threshold: u8,
    battery_start_threshold: u8,

    pub fn init(allocator: Allocator) !@This() {
        var buffer: [128]u8 = undefined;
        const path = try Config.get_config_path(allocator, &buffer);
        std.debug.print("path: {s}\n", .{path});
        return try parse_file(allocator, path);
    }

    fn get_config_path(allocator: Allocator, buffer: []u8) ![]u8 {
        var env_map = try std.process.getEnvMap(allocator);
        defer env_map.deinit();

        if (env_map.get("SUDO_USER")) |sudo_user| {
            return try std.fmt.bufPrint(
                buffer,
                "/home/{s}/.config/powereg/powereg.conf",
                .{sudo_user}
            );
        }

        if (env_map.get("HOME")) |home| {
            return try std.fmt.bufPrint(
                buffer,
                "{s}/.config/powereg/powereg.conf",
                .{home}
            );
        }

        return error.EnvironmentError;
    }

    fn parse_file(allocator: Allocator, path: []const u8) !@This() {
        var fd = try PersFd.init(path, false);
        defer fd.deinit();
        const text = try fd.read_value();

        const parsed = try std.json.parseFromSlice(
            @This(),
            allocator,
            text,
            .{}
        );
        defer parsed.deinit();

        return parsed.value;
    }

    pub fn apply(self: @This(), system_state: *SystemState) !void {
        if (system_state.acpi_type != AcpiType.ThinkPad)
            return SystemStateError.InvalidAcpiType;

        if (self.battery_start_threshold > self.battery_stop_threshold)
            return ConfigError.InvalidThresholds;

        try system_state.battery_states.set_charge_stop_threshold(self.battery_stop_threshold);
        std.debug.print("Battery charge stop threshold set to {}\n", .{self.battery_stop_threshold});

        try system_state.battery_states.set_charge_start_threshold(self.battery_start_threshold);
        std.debug.print("Battery charge start threshold set to {}\n", .{self.battery_start_threshold});
    }
};
