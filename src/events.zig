const std = @import("std");
const c = @cImport({
    @cInclude("libudev.h");
});
const l_system_state = @import("system_state.zig");

const mem = std.mem;
const SystemState = l_system_state.SystemState;
const State = l_system_state.State;

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

    pub fn init(interval_s: u64) !@This() {
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

    pub fn deinit(self: *@This()) void {
        _ = c.udev_monitor_unref(self.monitor);
        _ = c.udev_unref(self.udev);
    }

    pub fn poll_events(self: *@This()) !Event {
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
