// Fixed-capacity ring buffer.
const std = @import("std");

pub fn Ring(comptime T: type, comptime cap: usize) type {
    return struct {
        items: [cap]T = undefined,
        head: usize = 0,
        len: usize = 0,

        const Self = @This();

        pub fn push(self: *Self, value: T) void {
            self.items[(self.head + self.len) % cap] = value;
            if (self.len < cap) self.len += 1 else self.head = (self.head + 1) % cap;
        }
    };
}

pub fn main() !void {
    var ring = Ring(u32, 4){};
    ring.push(42);
    std.debug.print("len={d}\n", .{ring.len});
}
