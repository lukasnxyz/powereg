const std = @import("std");
const builtin = @import("builtin");
const l_cpu_states = @import("cpu_states.zig");
const l_battery_states = @import("battery_states.zig");
const l_utils = @import("utils.zig");

const mem = std.mem;
const CpuStates = l_cpu_states.CpuStates;
const CpuType = l_cpu_states.CpuType;
const ScalingGoverner = l_cpu_states.ScalingGoverner;
const AmdEPP = l_cpu_states.AmdEPP;
const PlatformProfile = l_battery_states.PlatformProfile;
const BatteryStates = l_battery_states.BatteryStates;
const AcpiType = l_battery_states.AcpiType;
const ChargingStatus = l_battery_states.ChargingStatus;
const PersFd = l_utils.PersFd;

pub const State = enum { Powersave, Balanced, Performance };
pub const SystemStateError = error { InvalidAcpiType };
pub const SystemState = struct {
    linux: bool,
    cpu_type: CpuType,
    acpi_type: AcpiType,

    cpu_states: CpuStates,
    battery_states: BatteryStates,

    state: State,

    pub fn init() !@This() {
        const cpu_type = SystemState.detect_cpu_type();
        return .{
            .linux = SystemState.detect_linux(),
            .cpu_type = cpu_type,
            .acpi_type = try SystemState.detect_acpi_type(),
            .cpu_states = try CpuStates.init(cpu_type),
            .battery_states = try BatteryStates.init(),
            .state = State.Powersave,
        };
    }

    pub fn post_init(self: *@This()) !void {
        const status = try self.battery_states.read_charging_status();
        switch (status) {
            ChargingStatus.Charging => return self.set_performance_mode(),
            ChargingStatus.DisCharging => return self.set_powersave_mode(),
            ChargingStatus.Unknown => return self.set_balanced_mode(),
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

    pub fn set_powersave_mode(self: *@This()) !void {
        try self.cpu_states.set_scaling_governer(ScalingGoverner.Powersave);
        try self.cpu_states.set_amd_epp(AmdEPP.Power);
        try self.battery_states.set_platform_profile(PlatformProfile.LowPower);
        try self.cpu_states.set_cpu_turbo_boost(0);
    }

    pub fn set_balanced_mode(self: *@This()) !void {
        if (try self.battery_states.read_charging_status() != ChargingStatus.Charging) {
            try self.cpu_states.set_scaling_governer(ScalingGoverner.Powersave);
            try self.cpu_states.set_amd_epp(AmdEPP.Power);
            try self.battery_states.set_platform_profile(PlatformProfile.LowPower);
        } else {
            try self.cpu_states.set_scaling_governer(ScalingGoverner.Powersave);
            try self.cpu_states.set_amd_epp(AmdEPP.BalancePower);
            try self.battery_states.set_platform_profile(PlatformProfile.Balanced);
        }

        try self.cpu_states.set_cpu_turbo_boost(0);
    }

    pub fn set_performance_mode(self: *@This()) !void {
        if (try self.battery_states.read_charging_status() == ChargingStatus.DisCharging)
            return;

        try self.cpu_states.set_scaling_governer(ScalingGoverner.Performance);
        try self.cpu_states.set_amd_epp(AmdEPP.Performance);
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
