package com.comphenix.protocol;

import org.bukkit.plugin.Plugin;

/** ProtocolLib's entry point on Horizon: its packet API over the server's packet hooks (no NMS, no Netty). */
public final class ProtocolLibrary {
    private ProtocolLibrary() {}

    private static ProtocolManager manager;

    public static synchronized ProtocolManager getProtocolManager() {
        if (manager == null) {
            Object h = horizon.javalayer.Native.callStatic("ProtocolLibrary", "getProtocolManager", new Object[0]);
            manager = (ProtocolManager) horizon.javalayer.Conv.fromNative(h, ProtocolManager.class);
        }
        return manager;
    }

    public static Plugin getPlugin() { return horizon.javalayer.Builtins.plugin("ProtocolLib"); }

    public static boolean isFolia() { return false; }
}
