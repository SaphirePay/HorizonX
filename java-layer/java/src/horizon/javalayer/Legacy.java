package horizon.javalayer;

import org.bukkit.plugin.PluginDescriptionFile;

import java.lang.invoke.MethodHandle;
import java.lang.invoke.MethodHandles;
import java.lang.invoke.MethodType;
import java.util.Set;
import java.util.function.Predicate;

/**
 * Paper's own bytecode rewriter (CraftBukkit's Commodore), run on every plugin class as Paper's plugin class loader
 * does: old field names (Enchantment.DURABILITY, Material.GRASS, Attribute.GENERIC_*), enums that became
 * interfaces (Sound, Particle...), rerouted methods. Without it a plugin built for an older API fails where Paper runs
 * it. Needs lib/server (the rewriter is part of the server); without it classes load as they are.
 */
public final class Legacy {
    private Legacy() {}

    private static volatile boolean tried;
    private static Object commodore;
    private static MethodHandle convert;
    private static MethodHandle versionOf;
    private static Object none;

    private static synchronized boolean ready() {
        if (tried) return convert != null;
        tried = true;
        ClassLoader server = ServerClasses.loader();
        if (server == null) return false;
        try {
            Class<?> commodoreType = Class.forName("org.bukkit.craftbukkit.util.Commodore", true, server);
            Class<?> apiVersion = Class.forName("org.bukkit.craftbukkit.util.ApiVersion", true, server);
            MethodHandles.Lookup lookup = MethodHandles.publicLookup();
            Predicate<String> compatibilities = s -> false;
            commodore = commodoreType.getConstructor(Predicate.class).newInstance(compatibilities);
            convert = lookup.findVirtual(commodoreType, "convert", MethodType.methodType(byte[].class, byte[].class, String.class, apiVersion, Set.class));
            versionOf = lookup.findStatic(apiVersion, "getOrCreateVersion", MethodType.methodType(apiVersion, String.class));
            none = apiVersion.getField("NONE").get(null);
            return true;
        } catch (Throwable e) {
            Logs.warning("Paper's legacy rewriter is not available: " + e);
            convert = null;
            return false;
        }
    }

    /** UnsafeValues.get(Class, String): an old constant name as today's (Paper's FieldRename, as Paper does). */
    static String renamedField(Class<?> type, String value) {
        if (type != org.bukkit.enchantments.Enchantment.class) return value;
        try {
            Class<?> rename = Class.forName("org.bukkit.craftbukkit.legacy.FieldRename");
            Class<?> api = Class.forName("org.bukkit.craftbukkit.util.ApiVersion");
            Object current = api.getField("CURRENT").get(null);
            return (String) rename.getMethod("convertEnchantmentName", api, String.class).invoke(null, current, value);
        } catch (ReflectiveOperationException | LinkageError e) {
            return value;
        }
    }

    /**
     * UnsafeValues.get(type, name): a registry value by its old enum constant name (Sound.ENTITY_PLAYER_LEVELUP,
     * Particle.REDSTONE...): the API class's own constant of that name, else the registry entry whose key matches.
     */
    static Object registryValue(Class<?> type, String value) {
        try {
            java.lang.reflect.Field f = type.getField(value);
            if (java.lang.reflect.Modifier.isStatic(f.getModifiers()) && type.isAssignableFrom(f.getType())) return f.get(null);
        } catch (ReflectiveOperationException | RuntimeException ignored) {}
        try {
            @SuppressWarnings({"unchecked", "rawtypes"})
            org.bukkit.Registry<org.bukkit.Keyed> registry = (org.bukkit.Registry) org.bukkit.Bukkit.getRegistry((Class) type);
            if (registry == null) return null;
            String lower = value.toLowerCase(java.util.Locale.ROOT);
            for (String k : new String[]{lower, lower.replace('_', '.')}) {
                org.bukkit.NamespacedKey key = org.bukkit.NamespacedKey.fromString(k.contains(":") ? k : "minecraft:" + k);
                if (key == null) continue;
                org.bukkit.Keyed found = registry.get(key);
                if (found != null) return found;
            }
        } catch (RuntimeException ignored) {}
        return null;
    }

    /** A plugin class's bytes as Paper would load them. */
    static byte[] convert(byte[] bytes, PluginDescriptionFile d) {
        if (!ready()) return bytes;
        try {
            String api = d.getAPIVersion();
            Object version = api == null ? none : versionOf.invoke(api);
            return (byte[]) convert.invoke(commodore, bytes, d.getName(), version, Set.of());
        } catch (Throwable e) {
            return bytes;
        }
    }
}
