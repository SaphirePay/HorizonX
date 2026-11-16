package com.comphenix.protocol.utility;

import java.io.Serializable;

/** ProtocolLib's snapshot version (MinecraftVersion.getSnapshot()): Horizon runs releases, so there is none. */
public final class SnapshotVersion implements Comparable<SnapshotVersion>, Serializable {
    private static final long serialVersionUID = 1L;
    private final String raw;

    public SnapshotVersion(String version) {
        this.raw = version;
    }

    public String getSnapshotString() {
        return raw;
    }

    @Override
    public int compareTo(SnapshotVersion o) {
        return o == null ? 1 : String.valueOf(raw).compareTo(String.valueOf(o.raw));
    }

    @Override
    public String toString() {
        return "Snapshot " + raw;
    }
}
