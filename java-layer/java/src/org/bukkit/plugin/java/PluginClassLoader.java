package org.bukkit.plugin.java;

import io.papermc.paper.plugin.configuration.PluginMeta;
import io.papermc.paper.plugin.provider.classloader.ConfiguredPluginClassLoader;
import org.bukkit.plugin.PluginDescriptionFile;

import java.io.File;
import java.net.URL;
import java.net.URLClassLoader;
import java.util.Collection;
import java.util.Map;
import java.util.concurrent.ConcurrentHashMap;

/**
 * The class every Bukkit plugin's classes are loaded by on Paper, by name: libraries look for it
 * (`Class.forName("org.bukkit.plugin.java.PluginClassLoader").isAssignableFrom(loader.getClass())`) and read its
 * fields by reflection (`description`, `dataFolder`, `file`, `plugin`), as zapper and libby do to find a plugin's
 * folder and libraries. Paper's is final and built by its plugin loader; the layer's loader (Loader.PluginLoaderImpl)
 * is this, with the same fields. The build takes Paper's class out of the API jar.
 */
public abstract class PluginClassLoader extends URLClassLoader implements ConfiguredPluginClassLoader {
    static {
        ClassLoader.registerAsParallelCapable();
    }

    private final JavaPluginLoader loader = null;
    private final Map<String, Class<?>> classes = new ConcurrentHashMap<>();
    public final PluginDescriptionFile description;
    public final File dataFolder;
    public final File file;
    private final URL url;
    private final ClassLoader libraryLoader = null;
    public JavaPlugin plugin;
    private JavaPlugin pluginInit;
    /** Paper's: which plugins this one depends on (depend and softdepend, through their own), read by Paper's
     *  SpigotPluginClassLoaderGroup to decide whose libraries this plugin sees. */
    public final io.papermc.paper.plugin.provider.entrypoint.DependencyContext dependencyContext = new io.papermc.paper.plugin.provider.entrypoint.DependencyContext() {
        @Override
        public boolean isTransitiveDependency(PluginMeta plugin, PluginMeta depend) {
            return transitive(plugin, depend.getName(), new java.util.HashSet<>());
        }

        @Override
        public boolean hasDependency(String name) {
            return org.bukkit.Bukkit.getPluginManager().getPlugin(name) != null;
        }
    };

    private static boolean transitive(PluginMeta plugin, String target, java.util.Set<String> seen) {
        if (plugin == null || !seen.add(plugin.getName())) return false;
        java.util.List<String> deps = new java.util.ArrayList<>(plugin.getPluginDependencies());
        deps.addAll(plugin.getPluginSoftDependencies());
        for (String dep : deps) {
            if (dep.equalsIgnoreCase(target)) return true;
            org.bukkit.plugin.Plugin other = org.bukkit.Bukkit.getPluginManager().getPlugin(dep);
            if (other != null && transitive(other.getPluginMeta(), target, seen)) return true;
        }
        return false;
    }

    protected PluginClassLoader(String name, URL[] urls, ClassLoader parent, PluginDescriptionFile description, File file, File dataFolder) {
        super(name, urls, parent);
        this.description = description;
        this.file = file;
        this.dataFolder = dataFolder;
        URL made = null;
        try {
            made = file.toURI().toURL();
        } catch (Exception ignored) {
        }
        this.url = made;
    }

    @Override
    public PluginMeta getConfiguration() {
        return description;
    }

    @Override
    public JavaPlugin getPlugin() {
        return plugin;
    }

    /** Paper's: the plugin made, before JavaPlugin's constructor ends. */
    public synchronized void initialize(JavaPlugin javaPlugin) {
        this.pluginInit = javaPlugin;
        init(javaPlugin);
    }

    Collection<Class<?>> getClasses() {
        return classes.values();
    }

    void setClass(String name, Class<?> type) {
        classes.putIfAbsent(name, type);
    }

    Class<?> loadClass0(String name, boolean resolve, boolean checkGlobal, boolean checkLibraries) throws ClassNotFoundException {
        return loadClass(name, resolve, checkGlobal, checkLibraries);
    }

    @Override
    public String toString() {
        return "PluginClassLoader{plugin=" + plugin + ", pluginEnabled=" + (plugin != null && plugin.isEnabled()) + ", url=" + file + "}";
    }
}
