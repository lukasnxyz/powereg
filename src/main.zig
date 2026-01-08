const std = @import("std");
const powereg = @import("powereg");

const mem = std.mem;
const fs = std.fs;
const StrCol = powereg.StrCol;
const SystemState = powereg.SystemState;

const LOOP_DURATION = 5;
const SERVICE_NAME = "powereg";
const SERVICE_PATH = "/etc/systemd/system/powereg.service";
const BINARY_PATH = "/usr/local/bin/powereg";
const RUN_FLAG = "--daemon";

pub fn main() !void {
    if (!(std.posix.getuid() == 0)) {
        std.debug.print("{s}\n", .{StrCol.red("Powereg needs to be run with root privilege (sudo)")});
        return;
    }

    var gpa = std.heap.DebugAllocator(.{}){};
    defer std.debug.assert(gpa.deinit() == .ok);
    const allocator = gpa.allocator();

    var system_state = powereg.SystemState.init() catch |e| {
        std.debug.print("Error while setting up system state: {any}", .{e});
        return;
    };
    try system_state.postInit();
    defer system_state.deinit();

    if (!system_state.linux) {
        std.debug.print("{s}\n", .{StrCol.red("Powereg is only made for Linux systems!")});
        return;
    }

    const ArgType = enum { live, monitor, daemon, install, uninstall };
    const arg_type = try parseArg(ArgType);
    switch (arg_type) {
        .live => {
            if (try checkRunningDaemonMode(allocator)) {
                std.debug.print("{s}\n", .{StrCol.yellow("Powereg is already running in daemon mode!")});
                std.debug.print("\t{s}\n", .{StrCol.yellow("use 'sudo powereg --monitor'")});
                return;
            }

            configSetup(allocator, &system_state);

            var poller = try powereg.EventPoller.init(LOOP_DURATION);
            defer poller.deinit();
            while (true) {
                std.debug.print("\x1B[2J\x1B[1;1H", .{});
                try system_state.print();
                // TODO: listen for 'q' to quit out
                const event = try poller.pollEvents();
                try powereg.EventPoller.handleEvent(event, &system_state);
            }
        },
        .monitor => {
            if (!try checkRunningDaemonMode(allocator)) {
                std.debug.print("{s}\n", .{StrCol.yellow("Powereg is not running in daemon mode!")});
                std.debug.print("\t{s}\n", .{StrCol.yellow("use 'sudo powereg --install'")});
            }

            var poller = try powereg.EventPoller.init(LOOP_DURATION);
            defer poller.deinit();
            while (true) {
                std.debug.print("\x1B[2J\x1B[1;1H", .{});
                try system_state.print();
                _ = try poller.pollEvents();
            }
        },
        .daemon => {
            configSetup(allocator, &system_state);

            var poller = try powereg.EventPoller.init(LOOP_DURATION);
            defer poller.deinit();
            while (true) {
                const event = try poller.pollEvents();
                try powereg.EventPoller.handleEvent(event, &system_state);
            }
        },
        .install => {
            if (try checkRunningDaemonMode(allocator)) {
                std.debug.print("{s}\n", .{StrCol.yellow("Powereg is already running in daemon mode!")});
                return;
            }

            configSetup(allocator, &system_state);

            try installDaemon(allocator);
        },
        .uninstall => {
            if (!try checkRunningDaemonMode(allocator)) {
                std.debug.print("{s}\n", .{StrCol.yellow("Powereg is not running in daemon mode!")});
                return;
            }

            try uninstallDaemon(allocator);
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
            std.debug.print("{s} {s}\n", .{ StrCol.red("Error: Argument must start with '--', got:"), arg });
            return error.InvalidArgumentFormat;
        }

        const arg_name = arg[2..];

        if (std.meta.stringToEnum(EnumType, arg_name)) |value| {
            if (found_arg != null) {
                std.debug.print("{s}\n", .{StrCol.red("Error: Multiple arguments provided. Only one is allowed.")});
                return error.TooManyArguments;
            }
            found_arg = value;
        } else {
            std.debug.print("{s} '--{s}'\n", .{ StrCol.red("Error: Invalid argument"), arg_name });
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
        std.debug.print("{s}\n", .{StrCol.red("Error: No argument provided.")});
        std.debug.print("Valid options: ", .{});
        inline for (@typeInfo(EnumType).@"enum".fields, 0..) |field, i| {
            if (i > 0) std.debug.print(", ", .{});
            std.debug.print("--{s}", .{field.name});
        }
        std.debug.print("\n", .{});
        return error.NoArgument;
    }
}

fn configSetup(allocator: std.mem.Allocator, system_state: *SystemState) void {
    if (powereg.Config.init(allocator)) |cfg| {
        cfg.apply(system_state) catch |e| {
            std.debug.print("Error while applying config: {any}.\n", .{e});
        };
    } else |e| {
        std.debug.print("Failed finding/reading config file: {any}\n", .{e});
    }

}

fn checkRunningDaemonMode(allocator: mem.Allocator) !bool {
    std.debug.print("{s}\n", .{StrCol.yellow("Running 'systemctl is-active powereg'")});

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

fn installDaemon(allocator: mem.Allocator) !void {
    _ = checkInstalledPowerTools(allocator) catch false;

    const service_file = try std.fmt.allocPrint(allocator,
        \\[Unit]
        \\Description=PowerEG - Power Management Daemon
        \\After=network.target
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
    , .{ BINARY_PATH, RUN_FLAG, SERVICE_NAME });
    defer allocator.free(service_file);

    const file = fs.cwd().createFile(SERVICE_PATH, .{}) catch |err| {
        std.debug.print("{s}: {}\n", .{ StrCol.red("Failed to write service file to " ++ SERVICE_PATH), err });
        return err;
    };
    defer file.close();

    try file.writeAll(service_file);

    std.debug.print("{s}\n", .{StrCol.yellow("Running 'systemctl daemon-reload'")});
    var result = try std.process.Child.run(.{
        .allocator = allocator,
        .argv = &[_][]const u8{ "systemctl", "daemon-reload" },
    });
    defer allocator.free(result.stdout);
    defer allocator.free(result.stderr);

    if (result.term.Exited != 0) {
        std.debug.print("{s} {s}\n", .{ StrCol.red("systemctl daemon-reload failed:"), result.stderr });
        return error.SystemctlFailed;
    }

    std.debug.print("{s}\n", .{StrCol.yellow("Running 'systemctl enable powereg'")});
    result = try std.process.Child.run(.{
        .allocator = allocator,
        .argv = &[_][]const u8{ "systemctl", "enable", SERVICE_NAME },
    });
    defer allocator.free(result.stdout);
    defer allocator.free(result.stderr);

    if (result.term.Exited != 0) {
        std.debug.print("{s} {s}\n", .{ StrCol.red("systemctl enable failed:"), result.stderr });
        return error.SystemctlFailed;
    }

    std.debug.print("{s}\n", .{StrCol.yellow("Running 'systemctl start powereg'")});
    result = try std.process.Child.run(.{
        .allocator = allocator,
        .argv = &[_][]const u8{ "systemctl", "start", SERVICE_NAME },
    });
    defer allocator.free(result.stdout);
    defer allocator.free(result.stderr);

    if (result.term.Exited != 0) {
        std.debug.print("{s} {s}\n", .{ StrCol.red("systemctl start failed:"), result.stderr });
        return error.SystemctlFailed;
    }

    std.debug.print("{s}\n", .{StrCol.green("Powereg successfully installed and started via systemd!")});
}

fn uninstallDaemon(allocator: mem.Allocator) !void {
    std.debug.print("{s}\n", .{StrCol.yellow("Running 'systemctl disable powereg'")});
    var result = try std.process.Child.run(.{
        .allocator = allocator,
        .argv = &[_][]const u8{ "systemctl", "disable", SERVICE_NAME },
    });
    defer allocator.free(result.stdout);
    defer allocator.free(result.stderr);

    if (result.term.Exited != 0) {
        std.debug.print("{s} {s}\n", .{ StrCol.red("systemctl disable failed:"), result.stderr });
    }

    std.debug.print("{s}\n", .{StrCol.yellow("Running 'systemctl stop powereg'")});
    result = try std.process.Child.run(.{
        .allocator = allocator,
        .argv = &[_][]const u8{ "systemctl", "stop", SERVICE_NAME },
    });
    defer allocator.free(result.stdout);
    defer allocator.free(result.stderr);

    if (result.term.Exited != 0) {
        std.debug.print("{s} {s}\n", .{ StrCol.red("systemctl stop failed:"), result.stderr });
    }

    fs.cwd().deleteFile(SERVICE_PATH) catch |err| {
        std.debug.print("{s}: {}\n", .{ StrCol.red("Failed to remove service file at " ++ SERVICE_PATH), err });
        return err;
    };

    std.debug.print("{s}\n", .{StrCol.yellow("Running 'systemctl daemon-reload'")});
    result = try std.process.Child.run(.{
        .allocator = allocator,
        .argv = &[_][]const u8{ "systemctl", "daemon-reload" },
    });
    defer allocator.free(result.stdout);
    defer allocator.free(result.stderr);

    if (result.term.Exited != 0) {
        std.debug.print("{s} {s}\n", .{ StrCol.red("systemctl daemon-reload failed:"), result.stderr });
        return error.SystemctlFailed;
    }

    std.debug.print("{s}\n", .{StrCol.green("Powereg uninstalled successfully!")});
}

fn checkInstalledPowerTools(allocator: mem.Allocator) !bool {
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
            std.debug.print("{s} {s}\n", .{ StrCol.yellow("Found running service:"), service });
            conflicts_found = true;

            std.debug.print("\t{s} {s}...\n", .{ StrCol.yellow("Attempting to stop"), service });
            const stop_result = std.process.Child.run(.{
                .allocator = allocator,
                .argv = &[_][]const u8{ "systemctl", "stop", service },
            }) catch {
                std.debug.print("\t{s} {s}\n", .{ StrCol.red("Failed to stop"), service });
                continue;
            };
            defer allocator.free(stop_result.stdout);
            defer allocator.free(stop_result.stderr);

            if (stop_result.term.Exited == 0) {
                std.debug.print("\t{s} {s}\n", .{ StrCol.green("Successfully stopped"), service });
            } else {
                std.debug.print("\t{s} {s}\n", .{ StrCol.red("Failed to stop"), service });
                continue;
            }

            std.debug.print("\t{s} {s}...\n", .{ StrCol.yellow("Attempting to disable"), service });
            const disable_result = std.process.Child.run(.{
                .allocator = allocator,
                .argv = &[_][]const u8{ "systemctl", "disable", service },
            }) catch {
                std.debug.print("\t{s} {s}\n", .{ StrCol.red("Failed to disable"), service });
                continue;
            };
            defer allocator.free(disable_result.stdout);
            defer allocator.free(disable_result.stderr);

            if (disable_result.term.Exited == 0) {
                std.debug.print("\t{s} {s}\n", .{ StrCol.green("Successfully disabled"), service });
            } else {
                std.debug.print("\t{s} {s}\n", .{ StrCol.red("Failed to disable"), service });
            }
        }
    }

    if (!conflicts_found) {
        std.debug.print("{s}\n", .{StrCol.green("No conflicting power management services found")});
    }

    return conflicts_found;
}
