package com.comphenix.protocol.events;

import com.comphenix.protocol.PacketType;
import com.comphenix.protocol.reflect.StructureModifier;
import com.comphenix.protocol.wrappers.BlockPosition;
import com.comphenix.protocol.wrappers.WrappedBlockData;
import horizon.javalayer.H;
import horizon.javalayer.Native;
import horizon.javalayer.Proxies;

import java.util.UUID;

/** A packet: its fields by kind, read and written through the server's bridge. */
public class PacketContainer implements Proxies.Handled {
    private final H h;

    protected PacketContainer(H h) { this.h = h; }

    public PacketContainer(PacketType type) {
        Object made = Native.construct("PacketContainer", new Object[]{type.horizonHandle()});
        if (!(made instanceof H handle)) throw new IllegalArgumentException("Horizon has no packet " + type);
        this.h = handle;
    }

    @Override public H horizonHandle() { return h; }

    private <T> StructureModifier<T> modifier(String name, Class<T> type) {
        return new StructureModifier<>((H) Native.call(h, name, new Object[0]), type);
    }

    public StructureModifier<Integer> getIntegers() { return modifier("getIntegers", Integer.class); }
    public StructureModifier<Short> getShorts() { return modifier("getShorts", Short.class); }
    public StructureModifier<Byte> getBytes() { return modifier("getBytes", Byte.class); }
    public StructureModifier<Boolean> getBooleans() { return modifier("getBooleans", Boolean.class); }
    public StructureModifier<Float> getFloat() { return modifier("getFloat", Float.class); }
    public StructureModifier<Double> getDoubles() { return modifier("getDoubles", Double.class); }
    public StructureModifier<Long> getLongs() { return modifier("getLongs", Long.class); }
    public StructureModifier<String> getStrings() { return modifier("getStrings", String.class); }
    public StructureModifier<UUID> getUUIDs() { return modifier("getUUIDs", UUID.class); }
    public StructureModifier<BlockPosition> getBlockPositionModifier() { return modifier("getBlockPositionModifier", BlockPosition.class); }
    public StructureModifier<BlockPosition> getSectionPositions() { return modifier("getSectionPositions", BlockPosition.class); }
    public StructureModifier<WrappedBlockData> getBlockData() { return modifier("getBlockData", WrappedBlockData.class); }
    public StructureModifier<Object> getModifier() { return modifier("getModifier", Object.class); }

    public PacketType getType() {
        Object raw = Native.call(h, "getType", new Object[0]);
        String name = String.valueOf(Native.call(raw, "name", new Object[0]));
        boolean server = Boolean.TRUE.equals(Native.call(raw, "isServer", new Object[0]));
        PacketType known = PacketType.fromName(name);
        return known != null ? known : new PacketType(server, name);
    }

    public Object getHandle() { return this; }
    public PacketContainer shallowClone() { return this; }
    public PacketContainer deepClone() { return this; }
    @Override public String toString() { return String.valueOf(Native.call(h, "toString", new Object[0])); }
}
