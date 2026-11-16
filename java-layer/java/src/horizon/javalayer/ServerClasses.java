package horizon.javalayer;

import java.io.File;
import java.net.URL;
import java.net.URLClassLoader;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.List;

/**
 * The Java server's own classes (net.minecraft, CraftBukkit, Paper's internals) from java-layer/lib/server: one class
 * loader over them, whose parent is the layer's. Parent-first, so the layer's CraftServer, CraftPlayer and API stay
 * the ones every plugin sees; everything else the server has is found here. Plugins' class loaders use it as their
 * parent. Nothing in it runs until a plugin reaches for it (its classes load on first use).
 */
public final class ServerClasses {
    private ServerClasses() {}

    private static volatile ClassLoader loader;
    private static volatile boolean looked;

    /** The folder java-layer/lib (where horizon-javalayer.jar is). */
    static File libDir() {
        for (String part : System.getProperty("java.class.path", "").split(File.pathSeparator)) {
            File f = new File(part);
            if (f.getName().startsWith("horizon-javalayer")) return f.getAbsoluteFile().getParentFile();
        }
        return null;
    }

    /** The loader over lib/server, or null when this server has no copy of the server classes. */
    static ClassLoader loader() {
        if (looked) return loader;
        synchronized (ServerClasses.class) {
            if (looked) return loader;
            try {
                File lib = libDir();
                File dir = lib == null ? null : new File(lib, "server");
                File[] jars = dir == null ? null : dir.listFiles((d, n) -> n.endsWith(".jar"));
                if (jars != null && jars.length > 0) {
                    Arrays.sort(jars);
                    List<URL> urls = new ArrayList<>();
                    for (File jar : jars) urls.add(jar.toURI().toURL());
                    loader = new URLClassLoader("paper-server", urls.toArray(new URL[0]), ServerClasses.class.getClassLoader());
                    Logs.info("the Java server's classes are available to plugins (" + jars.length + " jars in " + dir + ")");
                }
            } catch (Exception e) {
                Logs.warning("lib/server could not be opened: " + e);
            }
            looked = true;
            return loader;
        }
    }

    /** The parent for plugins' class loaders. */
    static ClassLoader parent() {
        ClassLoader l = loader();
        return l != null ? l : ServerClasses.class.getClassLoader();
    }

    static Class<?> find(String name) throws ClassNotFoundException {
        return Class.forName(name, false, parent());
    }
}
