package com.comphenix.protocol.wrappers;

import horizon.javalayer.H;
import horizon.javalayer.Native;
import horizon.javalayer.Proxies;
import org.bukkit.Location;
import org.bukkit.World;
import org.bukkit.util.Vector;

/** A block position; goes to the server as the bridge's position object. */
public class BlockPosition implements Proxies.Handled {
    public static final BlockPosition ORIGIN = new BlockPosition(0, 0, 0);
    protected final int x, y, z;

    public BlockPosition(int x, int y, int z) {
        this.x = x;
        this.y = y;
        this.z = z;
    }

    public BlockPosition(Vector v) { this(v.getBlockX(), v.getBlockY(), v.getBlockZ()); }

    protected BlockPosition(H h) {
        this(((Number) Native.call(h, "getX", new Object[0])).intValue(), ((Number) Native.call(h, "getY", new Object[0])).intValue(), ((Number) Native.call(h, "getZ", new Object[0])).intValue());
        Native.release(h.id);
    }

    @Override public H horizonHandle() { return (H) Native.construct("BlockPosition", new Object[]{x, y, z}); }

    public int getX() { return x; }
    public int getY() { return y; }
    public int getZ() { return z; }
    public Location toLocation(World world) { return new Location(world, x, y, z); }
    public Vector toVector() { return new Vector(x, y, z); }
    public BlockPosition add(BlockPosition o) { return new BlockPosition(x + o.x, y + o.y, z + o.z); }
    public BlockPosition subtract(BlockPosition o) { return new BlockPosition(x - o.x, y - o.y, z - o.z); }
    public BlockPosition multiply(int f) { return new BlockPosition(x * f, y * f, z * f); }
    public static BlockPosition getBlockPosition(Vector v) { return new BlockPosition(v); }
    @Override public boolean equals(Object o) { return o instanceof BlockPosition p && p.x == x && p.y == y && p.z == z; }
    @Override public int hashCode() { return (x * 31 + y) * 31 + z; }
    @Override public String toString() { return "BlockPosition [x=" + x + ", y=" + y + ", z=" + z + "]"; }
}
