const std = @import("std");

const mem = std.mem;
const OpenFlags = std.fs.File.OpenFlags;

pub const PersFdError = error { InvalidFilePermsErr };
pub const PersFd = struct {
    file: std.fs.File,
    write: bool,
    buffer: [512]u8 = undefined,

    pub fn init(path: []const u8, write: bool) !@This() {
        const flags = if (write) OpenFlags { .mode = .read_write } else OpenFlags { .mode = .read_only };
        const file = try std.fs.cwd().openFile(path, flags);
        return PersFd{
            .file = file,
            .write = write,
        };
    }

    pub fn deinit(self: *@This()) void {
        @memset(&self.buffer, 0);
        self.file.close();
    }

    pub fn read_value(self: *@This()) ![]const u8 {
        // TODO: readAll here is very dangerous with buffer[512] for /proc/stat
        try self.file.seekTo(0);
        @memset(&self.buffer, 0);
        const bytes_read = try self.file.readAll(&self.buffer);
        const raw_content = self.buffer[0..bytes_read];
        return mem.trim(u8, raw_content, &std.ascii.whitespace);
    }

    pub fn set_value(self: @This(), value: []const u8) !void {
        if (!self.write) {
            return error.InvalidFilePermsErr;
        }

        try self.file.setEndPos(0);
        try self.file.writeAll(value);
        try self.file.sync();
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
