package horizon.javalayer;

import org.bukkit.plugin.Plugin;
import org.bukkit.plugin.PluginDescriptionFile;

import java.io.File;
import java.lang.reflect.InvocationHandler;
import java.lang.reflect.Method;
import java.lang.reflect.Proxy;
import java.util.*;
import java.util.concurrent.ConcurrentHashMap;

/**
 * Plugins the layer is itself: ProtocolLib (its API here, over the server's packet hooks, since the real one needs
 * the Java server's internals). A plugin that depends on one finds it enabled; the real jar, if present, is not loaded.
 */
public final class Builtins {
    private Builtins() {}

    static final Map<String, String> VERSIONS = Map.of("ProtocolLib", "5.4.0");
    private static final Map<String, Plugin> MADE = new ConcurrentHashMap<>();
    /** Built-ins the server has the real jar of (plugins/ProtocolLib.jar): that jar runs, as on Paper. */
    private static final Set<String> REAL = ConcurrentHashMap.newKeySet();

    static boolean provides(String name) {
        for (String n : VERSIONS.keySet()) if (n.equalsIgnoreCase(name) && !REAL.contains(n)) return true;
        return false;
    }

    /** The server has this plugin's own jar: it is loaded like any plugin, and the built-in steps aside. */
    static void realJar(String name) {
        for (String n : VERSIONS.keySet()) if (n.equalsIgnoreCase(name)) REAL.add(n);
    }

    private static volatile ClassLoader standIn;

    /**
     * A class of the built-in ProtocolLib (lib/builtin/ProtocolLib.jar, beside the layer's jar and off the class path)
     * for a plugin's class loader, only while no real ProtocolLib jar is loaded: the real one's classes are its own.
     */
    static Class<?> standInClass(String name) throws ClassNotFoundException {
        if (!name.startsWith("com.comphenix.protocol.") || !provides("ProtocolLib")) throw new ClassNotFoundException(name);
        ClassLoader loader = standIn;
        if (loader == null) {
            synchronized (Builtins.class) {
                loader = standIn;
                if (loader == null) {
                    try {
                        File layerJar = new File(Builtins.class.getProtectionDomain().getCodeSource().getLocation().toURI());
                        File jar = new File(new File(layerJar.getParentFile(), "builtin"), "ProtocolLib.jar");
                        loader = new java.net.URLClassLoader("horizon-protocollib", new java.net.URL[]{jar.toURI().toURL()}, Builtins.class.getClassLoader());
                    } catch (Exception e) {
                        throw new ClassNotFoundException(name, e);
                    }
                    standIn = loader;
                }
            }
        }
        return loader.loadClass(name);
    }

    public static Plugin plugin(String name) {
        String real = VERSIONS.keySet().stream().filter(n -> n.equalsIgnoreCase(name)).findFirst().orElse(null);
        if (real == null) return null;
        return MADE.computeIfAbsent(real, n -> {
            PluginDescriptionFile description = new PluginDescriptionFile(n, VERSIONS.get(n), "horizon.javalayer.Builtins");
            InvocationHandler handler = (proxy, m, args) -> answer(proxy, m, args, n, description);
            return (Plugin) Proxy.newProxyInstance(Builtins.class.getClassLoader(), new Class<?>[]{Plugin.class}, handler);
        });
    }

    /** A plugin of the server's own (Rust), as Java sees it: its name and version from the server. */
    static Plugin serverPlugin(H h) {
        Object rawName = Native.tryCall(h, "getName", new Object[0]);
        String name = rawName == Native.MISSING || rawName == null ? "unknown" : String.valueOf(rawName);
        Plugin java = Loader.byName(name);
        if (java != null) {
            Native.release(h.id);
            return java;
        }
        return MADE.computeIfAbsent("rust:" + name, k -> {
            Object rawVersion = Native.tryCall(h, "getVersion", new Object[0]);
            String version = rawVersion == Native.MISSING || rawVersion == null ? "1.0" : String.valueOf(rawVersion);
            PluginDescriptionFile description = new PluginDescriptionFile(name, version, "horizon.rust." + name);
            InvocationHandler handler = (proxy, m, args) -> answer(proxy, m, args, name, description);
            return (Plugin) Proxy.newProxyInstance(Builtins.class.getClassLoader(), new Class<?>[]{Plugin.class}, handler);
        });
    }

    private static Object answer(Object proxy, Method m, Object[] args, String name, PluginDescriptionFile description) throws Throwable {
        switch (m.getName()) {
            case "getName": return name;
            case "isEnabled": return true;
            case "getDescription": case "getPluginMeta": return description;
            case "getServer": return Bridge.server();
            case "getLogger": return Logs.plugin(name);
            case "getDataFolder": return new File(Loader.pluginsDir, name);
            case "isNaggable": return false;
            case "toString": return description.getFullName();
            case "hashCode": return name.hashCode();
            case "equals": return proxy == args[0];
            default: break;
        }
        if (m.isDefault()) return InvocationHandler.invokeDefault(proxy, m, args);
        Class<?> r = m.getReturnType();
        if (r == void.class) return null;
        if (r.isPrimitive()) return Conv.zero(r);
        return null;
    }
}
