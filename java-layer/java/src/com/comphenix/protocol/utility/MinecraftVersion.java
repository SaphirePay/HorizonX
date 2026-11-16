package com.comphenix.protocol.utility;

import org.bukkit.Server;

import java.io.Serializable;
import java.util.Objects;
import java.util.regex.Matcher;
import java.util.regex.Pattern;

/**
 * ProtocolLib's MinecraftVersion on Horizon (its public API, ProtocolLib 5.4.0's): a game version, the named releases,
 * and the server's own (26.3 here, so every named one is at or below it).
 */
public final class MinecraftVersion implements Comparable<MinecraftVersion>, Serializable {
    private static final long serialVersionUID = 1L;
    private static final Pattern VERSION = Pattern.compile(".*\\(.*MC.\\s*([a-zA-Z0-9\\-.]+).*");

    public static final MinecraftVersion v1_21_6 = new MinecraftVersion("1.21.6");
    public static final MinecraftVersion v1_21_5 = new MinecraftVersion("1.21.5");
    public static final MinecraftVersion v1_21_4 = new MinecraftVersion("1.21.4");
    public static final MinecraftVersion v1_21_2 = new MinecraftVersion("1.21.2");
    public static final MinecraftVersion v1_21_0 = new MinecraftVersion("1.21.0");
    public static final MinecraftVersion v1_20_5 = new MinecraftVersion("1.20.5");
    public static final MinecraftVersion v1_20_4 = new MinecraftVersion("1.20.4");
    public static final MinecraftVersion CONFIG_PHASE_PROTOCOL_UPDATE = new MinecraftVersion("1.20.2");
    public static final MinecraftVersion TRAILS_AND_TAILS = new MinecraftVersion("1.20");
    public static final MinecraftVersion FEATURE_PREVIEW_2 = new MinecraftVersion("1.19.4");
    public static final MinecraftVersion FEATURE_PREVIEW_UPDATE = new MinecraftVersion("1.19.3");
    public static final MinecraftVersion WILD_UPDATE = new MinecraftVersion("1.19");
    public static final MinecraftVersion CAVES_CLIFFS_2 = new MinecraftVersion("1.18");
    public static final MinecraftVersion CAVES_CLIFFS_1 = new MinecraftVersion("1.17");
    public static final MinecraftVersion NETHER_UPDATE_4 = new MinecraftVersion("1.16.4");
    public static final MinecraftVersion NETHER_UPDATE_2 = new MinecraftVersion("1.16.2");
    public static final MinecraftVersion NETHER_UPDATE = new MinecraftVersion("1.16");
    public static final MinecraftVersion BEE_UPDATE = new MinecraftVersion("1.15");
    public static final MinecraftVersion VILLAGE_UPDATE = new MinecraftVersion("1.14");
    public static final MinecraftVersion AQUATIC_UPDATE = new MinecraftVersion("1.13");
    public static final MinecraftVersion COLOR_UPDATE = new MinecraftVersion("1.12");
    public static final MinecraftVersion EXPLORATION_UPDATE = new MinecraftVersion("1.11");
    public static final MinecraftVersion FROSTBURN_UPDATE = new MinecraftVersion("1.10");
    public static final MinecraftVersion COMBAT_UPDATE = new MinecraftVersion("1.9");
    public static final MinecraftVersion BOUNTIFUL_UPDATE = new MinecraftVersion("1.8");
    public static final MinecraftVersion SKIN_UPDATE = new MinecraftVersion("1.7.8");
    public static final MinecraftVersion WORLD_UPDATE = new MinecraftVersion("1.7.2");
    public static final MinecraftVersion HORSE_UPDATE = new MinecraftVersion("1.6.1");
    public static final MinecraftVersion REDSTONE_UPDATE = new MinecraftVersion("1.5.0");
    public static final MinecraftVersion SCARY_UPDATE = new MinecraftVersion("1.4.2");
    public static final MinecraftVersion LATEST = v1_21_6;

    private static volatile MinecraftVersion current;

    private final int major;
    private final int minor;
    private final int build;
    private final String development;

    public MinecraftVersion(Server server) {
        this(extractVersion(server.getVersion()));
    }

    public MinecraftVersion(String version) {
        String[] parts = version.split("-", 2);
        int[] numbers = new int[3];
        String[] elements = parts[0].split("\\.");
        for (int i = 0; i < Math.min(3, elements.length); i++) {
            try {
                numbers[i] = Integer.parseInt(elements[i].trim());
            } catch (NumberFormatException e) {
                throw new IllegalStateException("Cannot parse " + version, e);
            }
        }
        this.major = numbers[0];
        this.minor = numbers[1];
        this.build = numbers[2];
        this.development = parts.length > 1 ? parts[1] : null;
    }

    public MinecraftVersion(int major, int minor, int build) {
        this(major, minor, build, null);
    }

    public MinecraftVersion(int major, int minor, int build, String development) {
        this.major = major;
        this.minor = minor;
        this.build = build;
        this.development = development;
    }

    /** The game version in a server's version text (`git-Paper-235 (MC: 26.3)` gives `26.3`). */
    public static String extractVersion(String text) {
        Matcher m = VERSION.matcher(text == null ? "" : text);
        if (m.matches() && m.group(1) != null) return m.group(1);
        throw new IllegalStateException("Cannot parse version String '" + text + "'");
    }

    public static MinecraftVersion fromServerVersion(String serverVersion) {
        return new MinecraftVersion(extractVersion(serverVersion));
    }

    public static MinecraftVersion getCurrentVersion() {
        MinecraftVersion v = current;
        if (v == null) {
            try {
                v = fromServerVersion(org.bukkit.Bukkit.getVersion());
            } catch (RuntimeException e) {
                v = new MinecraftVersion(org.bukkit.Bukkit.getMinecraftVersion());
            }
            current = v;
        }
        return v;
    }

    public static void setCurrentVersion(MinecraftVersion version) {
        current = version;
    }

    public int getMajor() { return major; }
    public int getMinor() { return minor; }
    public int getBuild() { return build; }
    public String getDevelopmentStage() { return development; }
    public SnapshotVersion getSnapshot() { return null; }
    public boolean isSnapshot() { return false; }

    /** The server runs this version or a later one. */
    public boolean atOrAbove() {
        return getCurrentVersion().isAtLeast(this);
    }

    public String getVersion() {
        return development == null ? major + "." + minor + "." + build : major + "." + minor + "." + build + "-" + development;
    }

    @Override
    public int compareTo(MinecraftVersion o) {
        if (o == null) return 1;
        int c = Integer.compare(major, o.major);
        if (c == 0) c = Integer.compare(minor, o.minor);
        if (c == 0) c = Integer.compare(build, o.build);
        return c;
    }

    public boolean isAtLeast(MinecraftVersion other) {
        return other != null && compareTo(other) >= 0;
    }

    @Override
    public boolean equals(Object o) {
        return o instanceof MinecraftVersion v && v.major == major && v.minor == minor && v.build == build && Objects.equals(v.development, development);
    }

    @Override
    public int hashCode() {
        return Objects.hash(major, minor, build);
    }

    @Override
    public String toString() {
        return "(MC: " + getVersion() + ")";
    }
}
