package com.comphenix.protocol.reflect;

import horizon.javalayer.Conv;
import horizon.javalayer.H;
import horizon.javalayer.Native;

import java.util.ArrayList;
import java.util.List;

/** The fields of one kind of a packet, by index. */
public class StructureModifier<T> {
    private final H h;
    private final Class<T> type;

    public StructureModifier(H h, Class<T> type) {
        this.h = h;
        this.type = type;
    }

    @SuppressWarnings("unchecked")
    public T read(int index) { return (T) Conv.fromNative(Native.call(h, "read", new Object[]{index}), type); }

    @SuppressWarnings("unchecked")
    public T readSafely(int index) { return (T) Conv.fromNative(Native.call(h, "readSafely", new Object[]{index}), type); }

    public StructureModifier<T> write(int index, T value) {
        Native.call(h, "write", new Object[]{index, Conv.toNative(value)});
        return this;
    }

    public StructureModifier<T> writeSafely(int index, T value) {
        Native.call(h, "writeSafely", new Object[]{index, Conv.toNative(value)});
        return this;
    }

    public StructureModifier<T> modify(int index, java.util.function.UnaryOperator<T> select) { return write(index, select.apply(read(index))); }
    public int size() { return ((Number) Native.call(h, "size", new Object[0])).intValue(); }

    public List<T> getValues() {
        List<T> out = new ArrayList<>();
        for (int i = 0; i < size(); i++) out.add(read(i));
        return out;
    }

    public StructureModifier<T> writeDefaults() { return this; }
    public Class<T> getFieldType() { return type; }
}
