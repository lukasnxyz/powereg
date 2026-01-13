const std = @import("std");
const builtin = @import("builtin");
const l_root = @import("root.zig");
const l_cpu_states = @import("cpu_states.zig");
const l_battery_states = @import("battery_states.zig");

const mem = std.mem;
const CpuStates = l_cpu_states.CpuStates;
const CpuType = l_cpu_states.CpuType;
const ScalingGoverner = l_cpu_states.ScalingGoverner;
const EPP = l_cpu_states.EPP;
const PlatformProfile = l_battery_states.PlatformProfile;
const BatteryStates = l_battery_states.BatteryStates;
const AcpiType = l_battery_states.AcpiType;
const ChargingStatus = l_battery_states.ChargingStatus;
const PersFd = l_root.PersFd;

pub const State = enum { Powersave, Balanced, Performance };
pub const SystemStateError = error{InvalidAcpiType};
pub const SystemState = struct {
    linux: bool,
    cpu_type: CpuType,
    acpi_type: AcpiType,

    cpu_states: CpuStates,
    battery_states: BatteryStates,

    state: State,

    pub fn init() !@This() {
        const cpu_type = SystemState.detectCpuType();
        return .{
            .linux = SystemState.detectLinux(),
            .cpu_type = cpu_type,
            .acpi_type = try SystemState.detectAcpiType(),
            .cpu_states = try CpuStates.init(cpu_type),
            .battery_states = try BatteryStates.init(),
            .state = .Powersave,
        };
    }

    pub fn postInit(self: *@This()) !void {
        const status = try self.battery_states.readChargingStatus();
        switch (status) {
            ChargingStatus.Charging, ChargingStatus.NotCharging => {
                try self.setPerformanceMode(false);
                self.state = .Performance;
            },
            ChargingStatus.DisCharging => {
                self.setPowersaveMode();
                self.state = .Powersave;
            },
            ChargingStatus.Unknown => {
                self.setPowersaveMode();
                self.state = .Powersave;
            },
        }
    }

    pub fn deinit(self: *@This()) void {
        self.battery_states.deinit();
        self.cpu_states.deinit();
    }

    pub fn print(self: *@This()) !void {
        try self.cpu_states.print();
        try self.battery_states.print();
        std.debug.print("State: {any}\n", .{self.state});
    }

    pub fn setPowersaveMode(self: *@This()) void {
        self.cpu_states.setScalingGoverner(ScalingGoverner.Powersave) catch |err| {
            std.log.warn("Failed to set scaling governor to Powersave: {}", .{err});
        };
        self.cpu_states.setEPP(EPP.Power) catch |err| {
            std.log.warn("Failed to set EPP to Power: {}", .{err});
        };
        self.battery_states.setPlatformProfile(PlatformProfile.LowPower) catch |err| {
            std.log.warn("Failed to set platform profile to LowPower: {}", .{err});
        };
        self.cpu_states.setCpuBoost(false) catch |err| {
            std.log.warn("Failed to disable CPU boost: {}", .{err});
        };
    }

    // for now, only for high cpu temp situations when charging
    pub fn setBalancedMode(self: *@This()) void {
        self.cpu_states.setScalingGoverner(ScalingGoverner.Powersave) catch |err| {
            std.log.warn("Failed to set scaling governor to Powersave: {}", .{err});
        };
        self.cpu_states.setEPP(EPP.BalancePower) catch |err| {
            std.log.warn("Failed to set EPP to BalancePower: {}", .{err});
        };
        self.battery_states.setPlatformProfile(PlatformProfile.Balanced) catch |err| {
            std.log.warn("Failed to set platform profile to Balanced: {}", .{err});
        };
        self.cpu_states.setCpuBoost(false) catch |err| {
            std.log.warn("Failed to disable CPU boost: {}", .{err});
        };
    }

    pub fn setPerformanceMode(self: *@This(), enable_boost: bool) !void {
        if (try self.battery_states.readChargingStatus() == ChargingStatus.DisCharging)
            return;

        self.cpu_states.setScalingGoverner(ScalingGoverner.Performance) catch |err| {
            std.log.warn("Failed to set scaling governor to Performance: {}", .{err});
        };
        self.cpu_states.setEPP(EPP.Performance) catch |err| {
            std.log.warn("Failed to set EPP to Performance: {}", .{err});
        };
        self.battery_states.setPlatformProfile(PlatformProfile.Performance) catch |err| {
            std.log.warn("Failed to set platform profile to Performance: {}", .{err});
        };
        self.cpu_states.setCpuBoost(enable_boost) catch |err| {
            std.log.warn("Failed to set CPU boost to {}: {}", .{enable_boost, err});
        };
    }

    fn detectLinux() bool {
        const compile_time = if (builtin.os.tag == .linux) true else false;
        const proc_exists = if (std.fs.cwd().access("/proc", .{})) true else |_| false;
        const sys_exists = if (std.fs.cwd().access("/sys", .{})) true else |_| false;
        const etc_exists = if (std.fs.cwd().access("/etc", .{})) true else |_| false;

        const etc_os_release = if (std.fs.cwd().access("/etc/os-release", .{})) true else |_| false;
        const usr_os_release = if (std.fs.cwd().access("/usr/lib/os-release", .{})) true else |_| false;
        const has_os_release = etc_os_release or usr_os_release;

        return compile_time or (proc_exists and sys_exists) or (proc_exists and sys_exists and etc_exists and has_os_release);
    }

    fn detectCpuType() CpuType {
        var fd = PersFd.init("/proc/cpuinfo", false) catch
            return CpuType.Unknown;
        defer fd.close();

        const val = fd.readValue() catch
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

        if (detectCpuViaCpuid()) |cpu_type|
            return cpu_type;

        return CpuType.Unknown;
    }

    fn detectCpuViaCpuid() ?CpuType {
        if (builtin.cpu.arch != .x86 and builtin.cpu.arch != .x86_64)
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
            : .{ .memory = true });

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

    fn detectAcpiType() !AcpiType {
        const thinkpad = "thinkpad";
        const ideapad = "ideapad";

        var pv = try PersFd.init("/sys/class/dmi/id/product_version", false);
        defer pv.close();
        if (pv.readValue()) |product_version| {
            const trimmed = mem.trim(u8, product_version, &std.ascii.whitespace);
            var lowered: [pv.buffer.len]u8 = undefined;
            _ = std.ascii.lowerString(&lowered, trimmed);

            if (mem.indexOf(u8, &lowered, thinkpad) != null) return AcpiType.ThinkPad;
            if (mem.indexOf(u8, &lowered, ideapad) != null) return AcpiType.IdeaPad;
        } else |_| {}

        var pn = try PersFd.init("/sys/class/dmi/id/product_name", false);
        defer pn.close();
        if (pn.readValue()) |product_name| {
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
