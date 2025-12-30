const std = @import("std");
const powereg = @import("powereg");
const mem = std.mem;
const fs = std.fs;

const LOOP_DURATION = 5;
const SERVICE_NAME = "powereg";
const SERVICE_PATH = "/etc/systemd/system/powereg.service";
const BINARY_PATH = "/usr/local/bin/powereg";
const RUN_FLAG = "--daemon";

pub fn main() !void {
    var gpa = std.heap.GeneralPurposeAllocator(.{}){};
    defer _ = gpa.deinit();
    const allocator = gpa.allocator();

    var system_state = try powereg.SystemState.init(allocator);
    try system_state.post_init();
    defer system_state.deinit(allocator);

    if (!system_state.linux) {
        std.debug.print("Currently only supporting AMD cpus!", .{});
        return;
    }

    if (system_state.cpu_type != powereg.CpuType.AMD) {
        std.debug.print("Currently only supporting AMD cpus!", .{});
        return;
    }

    const ArgType = enum { live, monitor, daemon, install, uninstall };
    const arg_type = try parseArg(ArgType);
    switch (arg_type) {
        .live => {
            if (try check_running_daemon_mode(allocator)) {
                std.debug.print("Powereg is already running in daemon mode!\n", .{});
                std.debug.print("\tuse 'sudo powereg --monitor'", .{});
                return;
            }

            var poller = try powereg.EventPoller.init(LOOP_DURATION);
            defer poller.deinit();
            while (true) {
                std.debug.print("\x1B[2J\x1B[1;1H", .{});
                try system_state.print();
                const event = try poller.poll_events();
                try powereg.EventPoller.handle_event(event, &system_state);
            }
        },
        .monitor => {
            if (!try check_running_daemon_mode(allocator)) {
                std.debug.print("Powereg is not running in daemon mode!\n", .{});
                std.debug.print("\tuse 'sudo powereg --install'", .{});
                return;
            }

            var poller = try powereg.EventPoller.init(LOOP_DURATION);
            defer poller.deinit();
            while (true) {
                std.debug.print("\x1B[2J\x1B[1;1H", .{});
                try system_state.print();
                _ = try poller.poll_events();
            }
        },
        .daemon => {
            var poller = try powereg.EventPoller.init(LOOP_DURATION);
            defer poller.deinit();
            while (true) {
                const event = try poller.poll_events();
                try powereg.EventPoller.handle_event(event, &system_state);
            }
        },
        .install => {
            if (try check_running_daemon_mode(allocator)) {
                std.debug.print("Powereg is already running in daemon mode!\n", .{});
                return;
            }

            try install_daemon(allocator);
        },
        .uninstall => {
            if (!try check_running_daemon_mode(allocator)) {
                std.debug.print("Powereg is not running in daemon mode!\n", .{});
                return;
            }

            try uninstall_daemon(allocator);
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

        if (!mem.startsWith(u8, arg, "--")) {
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

pub fn check_running_daemon_mode(allocator: mem.Allocator) !bool {
    std.debug.print("\x1b[33mRunning 'systemctl is-active powereg'\x1b[0m\n", .{});

    const result = try std.process.Child.run(.{
        .allocator = allocator,
        .argv = &[_][]const u8{ "systemctl", "is-active", SERVICE_NAME },
    });
    defer allocator.free(result.stdout);
    defer allocator.free(result.stderr);

    const status_text = mem.trim(u8, result.stdout, &std.ascii.whitespace);

    if (mem.eql(u8, status_text, "active")) {
        return true;
    } else if (mem.eql(u8, status_text, "inactive")) {
        return false;
    } else {
        return false;
    }
}

pub fn install_daemon(allocator: mem.Allocator) !void {
    _ = check_installed_power_tools(allocator) catch false;

    const service_file = try std.fmt.allocPrint(allocator,
        \\[Unit]
        \\Description=PowerEG - Power Management Daemon
        \\After=network.target
        \\Documentation=man:{s}(8)
        \\
        \\[Service]
        \\Type=simple
        \\User=root
        \\ExecStart={s} {s}
        \\Restart=on-failure
        \\RestartSec=10
        \\
        \\# Security and isolation options
        \\ProtectSystem=strict
        \\ProtectHome=yes
        \\NoNewPrivileges=true
        \\PrivateTmp=yes
        \\
        \\# Logging
        \\StandardOutput=journal
        \\StandardError=journal
        \\SyslogIdentifier={s}
        \\
        \\[Install]
        \\WantedBy=multi-user.target
        \\
    , .{ SERVICE_NAME, BINARY_PATH, RUN_FLAG, SERVICE_NAME });
    defer allocator.free(service_file);

    const file = fs.cwd().createFile(SERVICE_PATH, .{}) catch |err| {
        std.debug.print("\x1b[31mFailed to write service file to {s}: {}\x1b[0m\n", .{ SERVICE_PATH, err });
        return err;
    };
    defer file.close();

    try file.writeAll(service_file);

    std.debug.print("\x1b[33mRunning 'systemctl daemon-reload'\x1b[0m\n", .{});
    var result = try std.process.Child.run(.{
        .allocator = allocator,
        .argv = &[_][]const u8{ "systemctl", "daemon-reload" },
    });
    defer allocator.free(result.stdout);
    defer allocator.free(result.stderr);

    if (result.term.Exited != 0) {
        std.debug.print("\x1b[31msystemctl daemon-reload failed: {s}\x1b[0m\n", .{result.stderr});
        return error.SystemctlFailed;
    }

    std.debug.print("\x1b[33mRunning 'systemctl enable powereg'\x1b[0m\n", .{});
    result = try std.process.Child.run(.{
        .allocator = allocator,
        .argv = &[_][]const u8{ "systemctl", "enable", SERVICE_NAME },
    });
    defer allocator.free(result.stdout);
    defer allocator.free(result.stderr);

    if (result.term.Exited != 0) {
        std.debug.print("\x1b[31msystemctl enable failed: {s}\x1b[0m\n", .{result.stderr});
        return error.SystemctlFailed;
    }

    std.debug.print("\x1b[33mRunning 'systemctl start powereg'\x1b[0m\n", .{});
    result = try std.process.Child.run(.{
        .allocator = allocator,
        .argv = &[_][]const u8{ "systemctl", "start", SERVICE_NAME },
    });
    defer allocator.free(result.stdout);
    defer allocator.free(result.stderr);

    if (result.term.Exited != 0) {
        std.debug.print("\x1b[31msystemctl start failed: {s}\x1b[0m\n", .{result.stderr});
        return error.SystemctlFailed;
    }

    std.debug.print("\x1b[32mPowereg succesfully installed and started via systemd!\x1b[0m\n", .{});
}

pub fn uninstall_daemon(allocator: mem.Allocator) !void {
    std.debug.print("\x1b[33mRunning 'systemctl disable powereg'\x1b[0m\n", .{});
    var result = try std.process.Child.run(.{
        .allocator = allocator,
        .argv = &[_][]const u8{ "systemctl", "disable", SERVICE_NAME },
    });
    defer allocator.free(result.stdout);
    defer allocator.free(result.stderr);

    if (result.term.Exited != 0) {
        std.debug.print("\x1b[31msystemctl disable failed: {s}\x1b[0m\n", .{result.stderr});
    }

    std.debug.print("\x1b[33mRunning 'systemctl stop powereg'\x1b[0m\n", .{});
    result = try std.process.Child.run(.{
        .allocator = allocator,
        .argv = &[_][]const u8{ "systemctl", "stop", SERVICE_NAME },
    });
    defer allocator.free(result.stdout);
    defer allocator.free(result.stderr);

    if (result.term.Exited != 0) {
        std.debug.print("\x1b[31msystemctl stop failed: {s}\x1b[0m\n", .{result.stderr});
    }

    fs.cwd().deleteFile(SERVICE_PATH) catch |err| {
        std.debug.print("\x1b[31mFailed to remove service file at {s}: {}\x1b[0m\n", .{ SERVICE_PATH, err });
        return err;
    };

    std.debug.print("\x1b[33mRunning 'systemctl daemon-reload'\x1b[0m\n", .{});
    result = try std.process.Child.run(.{
        .allocator = allocator,
        .argv = &[_][]const u8{ "systemctl", "daemon-reload" },
    });
    defer allocator.free(result.stdout);
    defer allocator.free(result.stderr);

    if (result.term.Exited != 0) {
        std.debug.print("\x1b[31msystemctl daemon-reload failed: {s}\x1b[0m\n", .{result.stderr});
        return error.SystemctlFailed;
    }

    std.debug.print("\x1b[32mPowereg uninstalled successfully!\x1b[0m\n", .{});
}

fn check_installed_power_tools(allocator: mem.Allocator) !bool {
    const services = [_][]const u8{
        "power-profiles-daemon.service",
        "tlp.service",
        "auto-cpufreq.service",
    };

    var conflicts_found = false;

    for (services) |service| {
        const result = std.process.Child.run(.{
            .allocator = allocator,
            .argv = &[_][]const u8{ "systemctl", "is-active", service },
        }) catch continue;
        defer allocator.free(result.stdout);
        defer allocator.free(result.stderr);

        const status_str = mem.trim(u8, result.stdout, &std.ascii.whitespace);

        if (mem.eql(u8, status_str, "active")) {
            std.debug.print("\x1b[33mFound running service: {s}\x1b[0m\n", .{service});
            conflicts_found = true;

            std.debug.print("\t\x1b[33mAttempting to stop {s}...\x1b[0m\n", .{service});
            const stop_result = std.process.Child.run(.{
                .allocator = allocator,
                .argv = &[_][]const u8{ "systemctl", "stop", service },
            }) catch {
                std.debug.print("\t\x1b[31mFailed to stop {s}\x1b[0m\n", .{service});
                continue;
            };
            defer allocator.free(stop_result.stdout);
            defer allocator.free(stop_result.stderr);

            if (stop_result.term.Exited == 0) {
                std.debug.print("\t\x1b[32mSuccessfully stopped {s}\x1b[0m\n", .{service});
            } else {
                std.debug.print("\t\x1b[31mFailed to stop {s}\x1b[0m\n", .{service});
                continue;
            }

            std.debug.print("\t\x1b[33mAttempting to disable {s}...\x1b[0m\n", .{service});
            const disable_result = std.process.Child.run(.{
                .allocator = allocator,
                .argv = &[_][]const u8{ "systemctl", "disable", service },
            }) catch {
                std.debug.print("\t\x1b[31mFailed to disable {s}\x1b[0m\n", .{service});
                continue;
            };
            defer allocator.free(disable_result.stdout);
            defer allocator.free(disable_result.stderr);

            if (disable_result.term.Exited == 0) {
                std.debug.print("\t\x1b[32mSuccessfully disabled {s}\x1b[0m\n", .{service});
            } else {
                std.debug.print("\t\x1b[31mFailed to disable {s}\x1b[0m\n", .{service});
            }
        }
    }

    if (!conflicts_found) {
        std.debug.print("\x1b[32mNo conflicting power management services found\x1b[0m\n", .{});
    }

    return conflicts_found;
}
