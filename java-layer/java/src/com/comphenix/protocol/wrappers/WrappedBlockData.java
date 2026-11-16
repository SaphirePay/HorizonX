package com.comphenix.protocol.wrappers;

import horizon.javalayer.H;
import horizon.javalayer.Native;
import horizon.javalayer.Proxies;
import org.bukkit.Material;

/** A block state in a packet. */
public class WrappedBlockData implements Proxies.Handled {
    private final H h;

    protected WrappedBlockData(H h) { this.h = h; }

    @Override public H horizonHandle() { return h; }

    public Material getType() { return Material.matchMaterial(String.valueOf(Native.call(h, "getType", new Object[0]))); }
    public int getData() { return 0; }
    @Override public String toString() { return String.valueOf(Native.call(h, "toString", new Object[0])); }
}
