package horizon.javalayer;

import io.papermc.paper.plugin.configuration.PluginMeta;
import io.papermc.paper.plugin.provider.classloader.ConfiguredPluginClassLoader;
import io.papermc.paper.plugin.provider.classloader.PluginClassLoaderGroup;
import io.papermc.paper.plugin.bootstrap.BootstrapContext;
import io.papermc.paper.plugin.bootstrap.PluginBootstrap;
import io.papermc.paper.plugin.bootstrap.PluginProviderContext;
import org.bukkit.configuration.file.YamlConfiguration;
import org.bukkit.permissions.Permission;
import org.bukkit.plugin.Plugin;
import org.bukkit.plugin.PluginDescriptionFile;
import org.bukkit.plugin.java.JavaPlugin;

import java.io.*;
import java.lang.reflect.InvocationHandler;
import java.lang.reflect.Method;
import java.lang.reflect.Proxy;
import java.nio.charset.StandardCharsets;
import java.net.URI;
import java.net.URL;
import java.net.URLClassLoader;
import java.nio.file.*;
import java.util.*;
import java.util.concurrent.CopyOnWriteArrayList;
import java.util.jar.JarFile;
import java.util.zip.ZipEntry;

/** Finds the jars' plugins, orders them as Paper does, loads each in its own class loader, enables them. */
public final class Loader {
    private Loader() {}

    static final List<JavaPlugin> PLUGINS = new CopyOnWriteArrayList<>();
    static final List<PluginLoaderImpl> LOADERS = new CopyOnWriteArrayList<>();
    static File pluginsDir;
    static File serverDir;

    static Plugin byName(String name) {
        if (Builtins.provides(name)) return Builtins.plugin(name);
        for (JavaPlugin p : PLUGINS) {
            if (p.getName().equalsIgnoreCase(name)) return p;
            if (p.getDescription().getProvides().stream().anyMatch(name::equalsIgnoreCase)) return p;
        }
        return null;
    }

    static List<JavaPlugin> all() { return PLUGINS; }

    /** One plugin's class loader: its jar, then the API, then (for softdepend APIs) the other plugins. */
    /** A plugin's class loader: Paper's org.bukkit.plugin.java.PluginClassLoader by class, fields and all (the layer's). */
    static final class PluginLoaderImpl extends org.bukkit.plugin.java.PluginClassLoader {
        static { ClassLoader.registerAsParallelCapable(); }

        PluginLoaderImpl(URL[] urls, ClassLoader parent, PluginDescriptionFile description, File file, File dataFolder) {
            super(description.getName(), urls, parent, description, file, dataFolder);
            // Into Paper's own class loader storage as a Spigot plugin's group (registerSpigotGroup, as Paper does):
            // libraries that look through it by reflection (gestalt, Paper's own groups) find every plugin.
            PluginClassLoaderGroup made = null;
            try {
                made = io.papermc.paper.plugin.provider.classloader.PaperClassLoaderStorage.instance().registerSpigotGroup(this);
            } catch (Throwable e) {
                Logs.warning("class loader group for " + description.getName() + ": " + e);
            }
            this.group = made;
        }

        private final PluginClassLoaderGroup group;

        @Override
        public Class<?> loadClass(String name, boolean resolve, boolean checkGlobal, boolean checkLibraries) throws ClassNotFoundException {
            try {
                return super.loadClass(name, resolve);
            } catch (ClassNotFoundException e) {
                if (checkGlobal) {
                    for (PluginLoaderImpl other : LOADERS) {
                        if (other == this) continue;
                        try {
                            return other.loadOwn(name);
                        } catch (ClassNotFoundException ignored) {}
                    }
                    return Builtins.standInClass(name);
                }
                throw e;
            }
        }

        @Override
        protected Class<?> loadClass(String name, boolean resolve) throws ClassNotFoundException {
            try {
                return super.loadClass(name, resolve);
            } catch (ClassNotFoundException e) {
                for (PluginLoaderImpl other : LOADERS) {
                    if (other == this) continue;
                    try {
                        return other.loadOwn(name);
                    } catch (ClassNotFoundException ignored) {}
                }
                return Builtins.standInClass(name);
            }
        }

        /** A class of the plugin's jar, through Paper's legacy rewriter (as Paper's plugin class loader does). */
        @Override
        protected Class<?> findClass(String name) throws ClassNotFoundException {
            String path = name.replace('.', '/') + ".class";
            java.net.URL url = findResource(path);
            if (url == null) throw new ClassNotFoundException(name);
            byte[] bytes;
            try (InputStream in = url.openStream()) {
                bytes = in.readAllBytes();
            } catch (IOException e) {
                throw new ClassNotFoundException(name, e);
            }
            long t0 = System.nanoTime();
            // Only the plugin's own jar, as on Paper (its libraries: loaded as they are).
            if (url.toString().startsWith("jar:" + file.toURI())) bytes = Legacy.convert(bytes, description);
            long t1 = System.nanoTime();
            // Waits on futures: the main thread answers other threads' calls while it waits (Waits).
            bytes = Waits.rewrite(bytes);
            long t2 = System.nanoTime();
            int dot = name.lastIndexOf('.');
            if (dot > 0) {
                String pkg = name.substring(0, dot);
                if (getDefinedPackage(pkg) == null) {
                    try {
                        definePackage(pkg, null, null, null, null, null, null, null);
                    } catch (IllegalArgumentException ignored) {}
                }
            }
            java.security.CodeSource source;
            try {
                source = new java.security.CodeSource(file.toURI().toURL(), (java.security.CodeSigner[]) null);
            } catch (java.net.MalformedURLException e) {
                source = null;
            }
            Class<?> made = defineClass(name, bytes, 0, bytes.length, source);
            if (PROFILE) ClassTimes.add(description.getName(), t1 - t0, t2 - t1, System.nanoTime() - t2);
            return made;
        }

        Class<?> loadOwn(String name) throws ClassNotFoundException {
            synchronized (getClassLoadingLock(name)) {
                Class<?> c = findLoadedClass(name);
                if (c != null) return c;
                return findClass(name);
            }
        }

        @Override
        public void init(JavaPlugin javaPlugin) {
            this.plugin = javaPlugin;
            javaPlugin.init(Bridge.server(), description, dataFolder, file, this, description, Logs.plugin(description.getName()));
        }

        @Override
        public PluginClassLoaderGroup getGroup() {
            return group != null ? group : OPEN_GROUP;
        }
    }

    /** Paper's class loader group for Spigot plugins: every plugin's classes are open to every other (the layer's
     * loaders already look through each other), what PaperClassLoaderStorage gives for each kind of group. */
    static final PluginClassLoaderGroup OPEN_GROUP = new PluginClassLoaderGroup() {
        @Override
        public Class<?> getClassByName(String name, boolean resolve, io.papermc.paper.plugin.provider.classloader.ConfiguredPluginClassLoader requester) {
            for (PluginLoaderImpl other : LOADERS) {
                if (other == requester) continue;
                try {
                    return other.loadOwn(name);
                } catch (ClassNotFoundException ignored) {
                }
            }
            try {
                return Builtins.standInClass(name);
            } catch (ClassNotFoundException e) {
                return null;
            }
        }

        @Override public void remove(io.papermc.paper.plugin.provider.classloader.ConfiguredPluginClassLoader loader) {}
        @Override public void add(io.papermc.paper.plugin.provider.classloader.ConfiguredPluginClassLoader loader) {}
        @Override public io.papermc.paper.plugin.provider.classloader.ClassLoaderAccess getAccess() { return loader -> true; }
    };

    /** Paper plugins (paper-plugin.yml): the `loader` each names (its PluginLoader, which adds libraries), by jar. */
    static final Map<File, String> CLASSPATH_LOADERS = new HashMap<>();

    /**
     * A Paper plugin's PluginLoader.classloader(builder), run before its class loader is made, as Paper does: the
     * libraries it adds (MavenLibraryResolver from its repositories, JarLibrary) go on the plugin's class path.
     */
    static List<URL> loaderLibraries(PluginDescriptionFile d, File jar, File data) {
        List<URL> out = new ArrayList<>();
        String name = CLASSPATH_LOADERS.get(jar);
        if (name == null) return out;
        try (URLClassLoader scratch = new URLClassLoader(d.getName() + "-loader", new URL[]{jar.toURI().toURL()}, ServerClasses.parent())) {
            Object made = scratch.loadClass(name).getDeclaredConstructor().newInstance();
            if (!(made instanceof io.papermc.paper.plugin.loader.PluginLoader pluginLoader)) return out;
            io.papermc.paper.plugin.bootstrap.PluginProviderContext context = new io.papermc.paper.plugin.bootstrap.PluginProviderContext() {
                @Override public PluginMeta getConfiguration() { return d; }
                @Override public java.nio.file.Path getDataDirectory() { return data.toPath(); }
                @Override public net.kyori.adventure.text.logger.slf4j.ComponentLogger getLogger() { return net.kyori.adventure.text.logger.slf4j.ComponentLogger.logger(d.getName()); }
                @Override public java.nio.file.Path getPluginSource() { return jar.toPath(); }
            };
            io.papermc.paper.plugin.loader.library.LibraryStore store = path -> {
                try {
                    out.add(path.toUri().toURL());
                } catch (IOException e) {
                    Logs.warning(d.getName() + ": library " + path + ": " + e);
                }
            };
            pluginLoader.classloader(new io.papermc.paper.plugin.loader.PluginClasspathBuilder() {
                @Override
                public io.papermc.paper.plugin.loader.PluginClasspathBuilder addLibrary(io.papermc.paper.plugin.loader.library.ClassPathLibrary library) {
                    try {
                        library.register(store);
                    } catch (Throwable e) {
                        Logs.warning(d.getName() + ": its loader's library could not be loaded: " + e);
                    }
                    return this;
                }

                @Override public io.papermc.paper.plugin.bootstrap.PluginProviderContext getContext() { return context; }
            });
        } catch (Throwable e) {
            Logs.warning(d.getName() + ": its loader " + name + " failed: " + e);
        }
        return out;
    }

    /** Paper plugins (paper-plugin.yml): the bootstrapper each names, by jar. */
    static final Map<File, String> BOOTSTRAPPERS = new HashMap<>();

    static PluginDescriptionFile describe(File jar) throws Exception {
        try (JarFile file = new JarFile(jar)) {
            // Paper reads paper-plugin.yml when a jar has one; its plugin.yml (if any) is for Spigot and Bukkit.
            ZipEntry entry = file.getEntry("paper-plugin.yml");
            boolean paper = entry != null;
            if (entry == null) entry = file.getEntry("plugin.yml");
            if (entry == null) return null;
            byte[] bytes;
            try (InputStream in = file.getInputStream(entry)) {
                bytes = in.readAllBytes();
            }
            if (paper) {
                YamlConfiguration yaml = YamlConfiguration.loadConfiguration(new InputStreamReader(new ByteArrayInputStream(bytes), StandardCharsets.UTF_8));
                String bootstrapper = yaml.getString("bootstrapper");
                if (bootstrapper != null && !bootstrapper.isBlank()) BOOTSTRAPPERS.put(jar, bootstrapper.trim());
                String classpathLoader = yaml.getString("loader");
                if (classpathLoader != null && !classpathLoader.isBlank()) CLASSPATH_LOADERS.put(jar, classpathLoader.trim());
                // Paper's `dependencies: server: X: {load: BEFORE|AFTER|OMIT, required: true}` as Bukkit's lists: a
                // dependency loaded BEFORE this one is a depend (soft when not required), AFTER makes this a loadbefore.
                org.bukkit.configuration.ConfigurationSection server = yaml.getConfigurationSection("dependencies.server");
                if (server != null) {
                    List<String> depend = new ArrayList<>(yaml.getStringList("depend")), soft = new ArrayList<>(yaml.getStringList("softdepend")), before = new ArrayList<>(yaml.getStringList("loadbefore"));
                    for (String dep : server.getKeys(false)) {
                        org.bukkit.configuration.ConfigurationSection s = server.getConfigurationSection(dep);
                        boolean required = s == null || s.getBoolean("required", true);
                        String load = s == null ? "OMIT" : s.getString("load", "OMIT").toUpperCase(Locale.ROOT);
                        if (load.equals("AFTER")) before.add(dep);
                        else if (required) depend.add(dep);
                        else soft.add(dep);
                    }
                    yaml.set("depend", depend);
                    yaml.set("softdepend", soft);
                    yaml.set("loadbefore", before);
                    yaml.set("dependencies", null);
                    bytes = yaml.saveToString().getBytes(StandardCharsets.UTF_8);
                }
            }
            return new PluginDescriptionFile(new ByteArrayInputStream(bytes));
        }
    }

    /** A Paper plugin's lifecycle registrations made while it bootstraps (before the plugin object exists), run
     * through the plugin's own lifecycle manager once it does. */
    static final class Deferred {
        final List<Object[]> calls = new ArrayList<>();
        Class<?> type;

        Object manager(Class<?> type) {
            this.type = type;
            return Proxy.newProxyInstance(Loader.class.getClassLoader(), new Class<?>[]{type}, (proxy, m, args) -> {
                switch (m.getName()) {
                    case "toString": return "LifecycleEventManager[bootstrap]";
                    case "hashCode": return System.identityHashCode(proxy);
                    case "equals": return proxy == args[0];
                    case "registerEventHandler":
                        calls.add(args == null ? new Object[0] : args.clone());
                        return null;
                    default: break;
                }
                if (m.isDefault()) return InvocationHandler.invokeDefault(proxy, m, args);
                return null;
            });
        }

        void replay(Plugin plugin) {
            if (type == null || calls.isEmpty()) return;
            Object real = Brigadier.lifecycleManager(type, plugin);
            for (Object[] args : calls) {
                for (Method m : type.getMethods()) {
                    if (!m.getName().equals("registerEventHandler") || m.getParameterCount() != args.length) continue;
                    try {
                        m.invoke(real, args);
                    } catch (Throwable e) {
                        Logs.warning(plugin.getName() + ": a lifecycle handler from its bootstrap failed: " + e);
                    }
                    break;
                }
            }
        }
    }

    /** What a Paper plugin's bootstrapper is given: its metadata, folders, logger and lifecycle manager. */
    static BootstrapContext bootContext(PluginDescriptionFile d, File jar, File data, Deferred deferred) {
        return (BootstrapContext) Proxy.newProxyInstance(Loader.class.getClassLoader(), new Class<?>[]{BootstrapContext.class}, (proxy, m, args) -> {
            switch (m.getName()) {
                case "getConfiguration":
                case "getPluginMeta": return d;
                case "getDataDirectory": return data.toPath();
                case "getPluginSource": return jar.toPath();
                case "getLogger": return net.kyori.adventure.text.logger.slf4j.ComponentLogger.logger(d.getName());
                case "getLifecycleManager": return deferred.manager(m.getReturnType());
                case "toString": return "BootstrapContext[" + d.getName() + "]";
                case "hashCode": return System.identityHashCode(proxy);
                case "equals": return proxy == args[0];
                default: break;
            }
            if (m.isDefault()) return InvocationHandler.invokeDefault(proxy, m, args);
            Class<?> r = m.getReturnType();
            if (r == void.class) return null;
            if (r.isPrimitive()) return Conv.zero(r);
            if (r.isInterface()) return Services.inert(r, "BootstrapContext." + m.getName());
            return null;
        });
    }

    /** Whether the bootstrapper makes the plugin object itself (overrides PluginBootstrap.createPlugin). */
    static boolean makesPlugin(Class<?> bootstrapper) {
        try {
            return bootstrapper.getMethod("createPlugin", PluginProviderContext.class).getDeclaringClass() != PluginBootstrap.class;
        } catch (NoSuchMethodException e) {
            return false;
        }
    }

    /**
     * Paper's start order, as its ModernPluginLoadingStrategy + LoadOrderTree + TopographicGraphSorter make it (read
     * from Paper 26.3's classes): a Guava directed graph of plugin names in the order the plugins were found, an edge
     * from each plugin to every depend/softdepend that is there (loadAfter), from every loadbefore that is there to the
     * plugin, and from each `provides` name to it; Kahn's sort (FIFO from the in-degree-0 nodes), reversed. The same
     * Guava graph class is used, so ties come out exactly as on Paper (SimpleEconomy before ChestShop, both after Vault).
     * On a cycle Paper refuses to start; here the plugins in it go last in the order found.
     */
    static List<Map.Entry<PluginDescriptionFile, File>> order(Map<String, Map.Entry<PluginDescriptionFile, File>> byName) {
        Map<String, Map.Entry<PluginDescriptionFile, File>> byExact = new HashMap<>();
        for (Map.Entry<PluginDescriptionFile, File> e : byName.values()) byExact.put(e.getKey().getName(), e);
        for (Map.Entry<PluginDescriptionFile, File> e : byName.values()) for (String p : e.getKey().getProvides()) byExact.putIfAbsent(p, e);
        com.google.common.graph.MutableGraph<String> graph = com.google.common.graph.GraphBuilder.directed().build();
        for (Map.Entry<PluginDescriptionFile, File> e : byName.values()) {
            PluginDescriptionFile d = e.getKey();
            String name = d.getName();
            List<String> loadAfter = new ArrayList<>(d.getDepend());
            loadAfter.addAll(d.getSoftDepend());
            for (String dep : loadAfter) if (byExact.containsKey(dep)) graph.putEdge(name, dep);
            for (String before : d.getLoadBefore()) if (byExact.containsKey(before)) graph.putEdge(before, name);
            graph.addNode(name);
            for (String p : d.getProvides()) graph.putEdge(p, name);
        }
        // Test runs only (the sweep's groups of unrelated plugins): HORIZON_JAVA_LOAD_FIRST=SimpleEconomy loads that
        // plugin before every other one that isn't its own dependency, as a server owner's working economy would be.
        // Unset on servers: Paper's order exactly.
        String first = System.getenv("HORIZON_JAVA_LOAD_FIRST");
        if (first != null && byExact.containsKey(first)) {
            // Everything it already loads after (its dependencies, theirs, and what must come before them by
            // loadbefore - LuckPerms before Vault) stays before it: no cycle.
            Set<String> own = new HashSet<>(com.google.common.graph.Graphs.reachableNodes(graph, first));
            for (Map.Entry<PluginDescriptionFile, File> e : byName.values()) {
                String other = e.getKey().getName();
                if (other.equals(first) || own.contains(other)) continue;
                graph.putEdge(other, first);
            }
        }
        List<String> sorted = new ArrayList<>();
        java.util.ArrayDeque<String> roots = new java.util.ArrayDeque<>();
        Map<String, Integer> nonRoots = new HashMap<>();
        for (String n : graph.nodes()) {
            int degree = graph.inDegree(n);
            if (degree == 0) roots.add(n);
            else nonRoots.put(n, degree);
        }
        String current;
        while ((current = roots.poll()) != null) {
            for (String next : graph.successors(current)) {
                Integer left = nonRoots.remove(next);
                int now = (left == null ? 0 : left) - 1;
                if (now == 0) roots.add(next);
                else nonRoots.put(next, now);
            }
            sorted.add(current);
        }
        java.util.Collections.reverse(sorted);
        List<Map.Entry<PluginDescriptionFile, File>> out = new ArrayList<>();
        Set<Map.Entry<PluginDescriptionFile, File>> done = java.util.Collections.newSetFromMap(new java.util.IdentityHashMap<>());
        for (String n : sorted) {
            Map.Entry<PluginDescriptionFile, File> e = byExact.get(n);
            // A `provides` name maps to its plugin: that plugin comes where its own name sorts.
            if (e != null && e.getKey().getName().equals(n) && done.add(e)) out.add(e);
        }
        if (!nonRoots.isEmpty()) Logs.warning("Plugins in a load-order cycle (Paper would not start them): " + nonRoots.keySet());
        for (Map.Entry<PluginDescriptionFile, File> e : byName.values()) if (done.add(e)) out.add(e);
        return out;
    }

    /** The jars in the order Paper finds them: its DirectoryProviderSource walks the plugins folder (Files.walk, the
     *  file system's own order), not a sorted list. Jars outside the folder keep their place after those in it. */
    static String[] folderOrder(File plugins, String[] jars) {
        Map<String, Integer> place = new HashMap<>();
        try (java.util.stream.Stream<java.nio.file.Path> walk = java.nio.file.Files.walk(plugins.toPath(), 1)) {
            int[] i = {0};
            walk.forEach(p -> place.putIfAbsent(p.toAbsolutePath().normalize().toString().toLowerCase(Locale.ROOT), i[0]++));
        } catch (IOException | RuntimeException e) {
            return jars;
        }
        Integer[] idx = new Integer[jars.length];
        for (int i = 0; i < jars.length; i++) idx[i] = i;
        java.util.Arrays.sort(idx, java.util.Comparator.comparingInt(i -> place.getOrDefault(new File(jars[i]).toPath().toAbsolutePath().normalize().toString().toLowerCase(Locale.ROOT), Integer.MAX_VALUE)));
        String[] out = new String[jars.length];
        for (int i = 0; i < jars.length; i++) out[i] = jars[idx[i]];
        return out;
    }

    /** plugin.yml's `libraries:` with their own dependencies, as Paper resolves them (Libraries), from java-layer/libraries. */
    static List<URL> libraries(PluginDescriptionFile d) {
        List<URL> out = new ArrayList<>();
        if (d.getLibraries() == null || d.getLibraries().isEmpty()) return out;
        for (File jar : Libraries.resolve(d.getLibraries(), new File(serverDir, "java-layer/libraries"), d.getName())) {
            try {
                out.add(jar.toURI().toURL());
            } catch (IOException e) {
                Logs.warning(d.getName() + ": library " + jar + ": " + e);
            }
        }
        return out;
    }

    static void loadAll(File plugins, File server, String[] jars) {
        pluginsDir = plugins;
        serverDir = server;
        // Paper's config folder (paper-global.yml and the rest): there on every Paper server, and plugins write
        // their own files into it (Streamotes' config/streamotes.json5).
        new File(server, "config").mkdirs();
        for (String name : new String[]{"paper-global.yml", "paper-world-defaults.yml"}) {
            File to = new File(server, "config/" + name);
            if (to.isFile()) continue;
            try (InputStream in = Loader.class.getResourceAsStream("/paper-defaults/" + name)) {
                if (in != null) java.nio.file.Files.copy(in, to.toPath());
            } catch (IOException e) {
                Logs.warning("config/" + name + ": " + e);
            }
        }
        Map<String, Map.Entry<PluginDescriptionFile, File>> byName = new LinkedHashMap<>();
        for (String path : folderOrder(plugins, jars)) {
            File jar = new File(path);
            try {
                PluginDescriptionFile d = describe(jar);
                if (d == null) {
                    Logs.warning(jar.getName() + " has no plugin.yml: not a Bukkit plugin (a library, a mod?)");
                    continue;
                }
                if (Builtins.provides(d.getName())) {
                    // The real jar runs, as on Paper (its internals, async listeners, NMS packets); the built-in only
                    // stands in when there is none.
                    Builtins.realJar(d.getName());
                    Logs.info(jar.getName() + ": the real " + d.getName() + " is loaded (Horizon's built-in steps aside)");
                }
                String key = d.getName().toLowerCase(Locale.ROOT);
                if (byName.containsKey(key)) {
                    Logs.warning("Ambiguous plugin name `" + d.getName() + "' for files `" + jar.getName() + "' and `" + byName.get(key).getValue().getName() + "': keeping the first");
                    continue;
                }
                byName.put(key, Map.entry(d, jar));
                Report.found(d.getName(), jar, d.getVersion());
            } catch (Throwable e) {
                Logs.severe("Could not read plugin.yml of " + jar.getName(), e);
            }
        }
        bootIfUsed(byName.values());
        // As Paper's CraftServer.loadPlugins: every plugin made first, then each one's onLoad, in order (ViaVersion
        // runs its add-ons' listeners in its onLoad: ViaBackwards registers them when it is made).
        List<Map.Entry<JavaPlugin, File>> made = new ArrayList<>();
        for (Map.Entry<PluginDescriptionFile, File> entry : order(byName)) {
            JavaPlugin plugin = loadOne(entry.getKey(), entry.getValue(), byName);
            if (plugin != null) made.add(Map.entry(plugin, entry.getValue()));
        }
        for (Map.Entry<JavaPlugin, File> e : made) {
            onLoad(e.getKey(), e.getValue());
            // Their async threads read the server too: answered between plugins, not after the whole start.
            Native.pumpQueries();
        }
    }

    /** A made plugin's onLoad, as CraftServer.loadPlugins calls it ("Loading X", then onLoad). */
    static void onLoad(JavaPlugin plugin, File jar) {
        String name = plugin.getName();
        Report.CURRENT.set(name);
        try {
            Logs.info("Loading " + plugin.getDescription().getFullName());
            plugin.onLoad();
        } catch (Throwable e) {
            Logs.severe("Error initializing plugin '" + jar.getName() + "'", e);
            Report.state(name, "onload-failed", String.valueOf(e));
        } finally {
            Report.CURRENT.remove();
        }
    }

    /** PluginManager.loadPlugin: a jar after start, with Paper's errors (no plugin.yml, already loaded, a depend missing). */
    static JavaPlugin loadLate(File jar) throws org.bukkit.plugin.InvalidPluginException {
        PluginDescriptionFile d;
        try {
            d = describe(jar);
        } catch (Exception e) {
            throw new org.bukkit.plugin.InvalidPluginException(e);
        }
        if (d == null) throw new org.bukkit.plugin.InvalidPluginException(new java.io.FileNotFoundException("Jar does not contain plugin.yml"));
        if (byName(d.getName()) != null) throw new org.bukkit.plugin.InvalidPluginException("Plugin " + d.getName() + " is already loaded");
        List<String> missing = new ArrayList<>();
        for (String dep : d.getDepend()) if (byName(dep) == null && !serverHas(dep)) missing.add(dep);
        if (!missing.isEmpty()) throw new org.bukkit.plugin.UnknownDependencyException(missing, d.getFullName());
        if (namesServerCode(jar)) bootIfUsed(List.of(Map.entry(d, jar)));
        Report.found(d.getName(), jar, d.getVersion());
        JavaPlugin plugin = loadOne(d, jar, new LinkedHashMap<>());
        if (plugin == null) throw new org.bukkit.plugin.InvalidPluginException("Could not load " + jar.getName() + " (see the log above)");
        return plugin;
    }

    /** One plugin's jar: its loader, bootstrapper or main class, onLoad. Null when it can't load (said why). Start-up
     * loads each this way, and PluginManager.loadPlugin does too for a jar that comes later (as Paper's does). */
    static JavaPlugin loadOne(PluginDescriptionFile d, File jar, Map<String, Map.Entry<PluginDescriptionFile, File>> byName) {
        File plugins = pluginsDir;
        List<String> missing = new ArrayList<>();
        for (String dep : d.getDepend()) if (byName(dep) == null && !byName.containsKey(dep.toLowerCase(Locale.ROOT)) && !serverHas(dep)) missing.add(dep);
        if (!missing.isEmpty()) {
            Logs.severe("Could not load '" + jar.getName() + "': unknown dependency " + String.join(", ", missing), null);
            Report.state(d.getName(), "missing-dependency", String.join(", ", missing));
            return null;
        }
        Report.CURRENT.set(d.getName());
        try {
            List<URL> urls = new ArrayList<>();
            urls.add(jar.toURI().toURL());
            urls.addAll(libraries(d));
            File data = new File(plugins, d.getName());
            urls.addAll(loaderLibraries(d, jar, data));
            PluginLoaderImpl loader = new PluginLoaderImpl(urls.toArray(new URL[0]), ServerClasses.parent(), d, jar, data);
            LOADERS.add(loader);
            // A Paper plugin's bootstrapper runs first (PluginBootstrap.bootstrap), and may make the plugin itself.
            JavaPlugin plugin = null;
            Deferred deferred = null;
            String bootstrapper = BOOTSTRAPPERS.get(jar);
            if (bootstrapper != null) {
                deferred = new Deferred();
                BootstrapContext context = bootContext(d, jar, data, deferred);
                Class<?> type = Class.forName(bootstrapper, true, loader);
                // Paper makes the bootstrapper and main class whatever their constructor's access (ProviderUtil).
                java.lang.reflect.Constructor<?> made = type.getDeclaredConstructor();
                made.setAccessible(true);
                PluginBootstrap bootstrap = (PluginBootstrap) made.newInstance();
                bootstrap.bootstrap(context);
                Logs.info("Bootstrapped " + d.getFullName());
                if (makesPlugin(type)) plugin = bootstrap.createPlugin(context);
            }
            if (plugin == null) {
                Class<?> main = Class.forName(d.getMain(), true, loader);
                if (!JavaPlugin.class.isAssignableFrom(main)) {
                    Logs.severe("Could not load '" + jar.getName() + "': main class `" + d.getMain() + "' does not extend JavaPlugin", null);
                    Report.state(d.getName(), "load-failed", "main class does not extend JavaPlugin");
                    LOADERS.remove(loader);
                    return null;
                }
                java.lang.reflect.Constructor<?> made = main.getDeclaredConstructor();
                made.setAccessible(true);
                plugin = (JavaPlugin) made.newInstance();
            }
            if (loader.plugin == null) loader.init(plugin);
            if (deferred != null) deferred.replay(plugin);
            PLUGINS.add(plugin);
            Report.state(d.getName(), "loaded", null);
            return plugin;
        } catch (Throwable e) {
            Throwable cause = e instanceof java.lang.reflect.InvocationTargetException ite && ite.getCause() != null ? ite.getCause() : e;
            Logs.severe("Could not load '" + jar.getName() + "' in folder '" + plugins.getName() + "'", cause);
            Report.state(d.getName(), "load-failed", String.valueOf(cause));
        } finally {
            Report.CURRENT.remove();
        }
        return null;
    }

    /**
     * Vanilla's classes need vanilla booted (registries, version) before their first use, as on Paper. Done before
     * any plugin loads, and only when one of the jars names the Java server's own code (NMS, CraftBukkit internals):
     * other servers never pay for it.
     */
    static void bootIfUsed(Collection<Map.Entry<PluginDescriptionFile, File>> jars) {
        String who = null;
        for (Map.Entry<PluginDescriptionFile, File> e : jars) {
            if (namesServerCode(e.getValue())) {
                who = e.getKey().getName();
                break;
            }
        }
        if (who == null) return;
        Logs.info(who + " uses the Java server's own classes: starting vanilla's registries for it");
        try {
            horizon.javalayer.nms.Hollow.boot();
        } catch (Throwable t) {
            Logs.severe("[JavaLayer] vanilla could not start (is java-layer/lib/server there?)", t);
        }
    }

    private static final byte[][] SERVER_NAMES = {"net/minecraft/".getBytes(StandardCharsets.ISO_8859_1), "net.minecraft.".getBytes(StandardCharsets.ISO_8859_1),
            "org/bukkit/craftbukkit".getBytes(StandardCharsets.ISO_8859_1), "org.bukkit.craftbukkit".getBytes(StandardCharsets.ISO_8859_1)};

    static boolean namesServerCode(File jar) {
        try (JarFile j = new JarFile(jar)) {
            for (Enumeration<java.util.jar.JarEntry> en = j.entries(); en.hasMoreElements(); ) {
                java.util.jar.JarEntry entry = en.nextElement();
                if (!entry.getName().endsWith(".class")) continue;
                byte[] b;
                try (InputStream in = j.getInputStream(entry)) {
                    b = in.readAllBytes();
                }
                for (byte[] needle : SERVER_NAMES) if (indexOf(b, needle) >= 0) return true;
            }
        } catch (IOException ignored) {}
        return false;
    }

    private static int indexOf(byte[] hay, byte[] needle) {
        outer:
        for (int i = 0, n = hay.length - needle.length; i <= n; i++) {
            if (hay[i] != needle[0]) continue;
            for (int k = 1; k < needle.length; k++) if (hay[i + k] != needle[k]) continue outer;
            return i;
        }
        return -1;
    }

    /** A hard dependency the Rust server itself has (a native plugin of the same name). */
    static boolean serverHas(String name) {
        if (Builtins.provides(name)) return true;
        Object raw = Native.tryStatic("PluginManager", "isPluginEnabled", new Object[]{name});
        if (raw instanceof Boolean b) return b;
        Object plugin = Native.tryStatic("Bukkit", "getPlugin", new Object[]{name});
        return plugin != null && plugin != Native.MISSING;
    }

    /**
     * True while the `load: STARTUP` plugins enable: as on Paper, the server has no worlds yet (getWorlds() empty,
     * getWorld null). They hear each world's WorldInitEvent and WorldLoadEvent after (worldsLoaded), as Paper loads
     * its worlds between enablePlugins(STARTUP) and enablePlugins(POSTWORLD): WorldEdit sets itself up that way.
     */
    static volatile boolean startupPhase;

    static void enableAll() {
        // CraftServer.enablePlugins(STARTUP), the worlds, then enablePlugins(POSTWORLD): every STARTUP plugin first, in
        // load order; Bukkit's and the game's own permissions are registered after them and before the rest
        // (DefaultPermissions/CraftDefaultPermissions.registerCorePermissions).
        startupPhase = true;
        try {
            for (JavaPlugin p : PLUGINS) {
                if (p.getDescription().getLoad() != org.bukkit.plugin.PluginLoadOrder.STARTUP) continue;
                enable(p);
                Native.pumpQueries();
            }
        } finally {
            worldsLoaded();
        }
        PluginManagerImpl.INSTANCE.registerCorePermissions();
        for (JavaPlugin p : PLUGINS) {
            if (p.getDescription().getLoad() == org.bukkit.plugin.PluginLoadOrder.STARTUP) continue;
            enable(p);
            Native.pumpQueries();
        }
    }

    /** The worlds are there: WorldInitEvent and WorldLoadEvent for each, heard by the STARTUP plugins (Paper's order). */
    private static void worldsLoaded() {
        startupPhase = false;
        boolean any = false;
        for (JavaPlugin p : PLUGINS) any |= p.isEnabled();
        if (!any) return;
        for (org.bukkit.World world : org.bukkit.Bukkit.getWorlds()) {
            try {
                Events.call(new org.bukkit.event.world.WorldInitEvent(world));
                Events.call(new org.bukkit.event.world.WorldLoadEvent(world));
            } catch (RuntimeException e) {
                Logs.severe("[JavaLayer] world events for " + world.getName(), e);
            }
        }
    }

    /** HORIZON_JAVA_PROFILE set: class loading and onEnable timed per plugin (logged as `[profile]`, see jcalls.py). */
    static final boolean PROFILE = System.getenv("HORIZON_JAVA_PROFILE") != null;

    static final class ClassTimes {
        private static final Map<String, long[]> BY = new java.util.concurrent.ConcurrentHashMap<>();

        static void add(String plugin, long legacy, long waits, long define) {
            long[] row = BY.computeIfAbsent(plugin, k -> new long[4]);
            synchronized (row) {
                row[0]++;
                row[1] += legacy;
                row[2] += waits;
                row[3] += define;
            }
        }

        static String line(String plugin) {
            long[] row = BY.getOrDefault(plugin, new long[4]);
            return row[0] + " classes so far: legacy rewrite " + row[1] / 1_000_000 + " ms, waits rewrite " + row[2] / 1_000_000 + " ms, define " + row[3] / 1_000_000 + " ms";
        }
    }

    static void enable(Plugin plugin) {
        if (!(plugin instanceof JavaPlugin p) || p.isEnabled()) return;
        Logs.info("Enabling " + p.getDescription().getFullName());
        Report.CURRENT.set(p.getName());
        try {
            for (Permission permission : p.getDescription().getPermissions()) {
                try {
                    PluginManagerImpl.INSTANCE.addPermission(permission);
                } catch (IllegalArgumentException e) {
                    Logs.warning("Plugin " + p.getDescription().getFullName() + " tried to register permission '" + permission.getName() + "' but it's already registered");
                }
            }
            Commands.MAP.fromDescription(p);
            long enableStart = System.nanoTime();
            p.setEnabled(true);
            if (PROFILE) Logs.info("[profile] " + p.getName() + " onEnable " + (System.nanoTime() - enableStart) / 1_000_000 + " ms; " + ClassTimes.line(p.getName()));
            // A listener it put straight on a HandlerList (MCCoroutine): the server fires that event for it too.
            Events.mapOwners();
            Events.syncExternal();
            if (p.isEnabled()) Events.call(new org.bukkit.event.server.PluginEnableEvent(p));
            Report.state(p.getName(), p.isEnabled() ? "enabled" : "disabled-itself", null);
        } catch (Throwable e) {
            Logs.severe("Error occurred while enabling " + p.getDescription().getFullName() + " (Is it up to date?)", e);
            Synth.exception(new com.destroystokyo.paper.exception.ServerPluginEnableDisableException("Error occurred while enabling " + p.getDescription().getFullName() + " (Is it up to date?)", e, p));
            Report.state(p.getName(), "enable-failed", String.valueOf(e));
            disable(p);
        } finally {
            Report.CURRENT.remove();
        }
    }

    static void disable(Plugin plugin) {
        if (!(plugin instanceof JavaPlugin p)) return;
        if (p.isEnabled()) {
            Logs.info("Disabling " + p.getDescription().getFullName());
            try {
                p.setEnabled(false);
                Events.call(new org.bukkit.event.server.PluginDisableEvent(p));
            } catch (Throwable e) {
                Logs.severe("Error occurred while disabling " + p.getDescription().getFullName(), e);
            }
        }
        Scheduler.INSTANCE.cancelTasks(p);
        ServicesImpl.INSTANCE.unregisterAll(p);
        Events.removeAll(p);
        Commands.MAP.removeAll(p);
    }

    static void disableAll() {
        List<JavaPlugin> reversed = new ArrayList<>(PLUGINS);
        Collections.reverse(reversed);
        for (JavaPlugin p : reversed) disable(p);
    }

    static void closeAll() {
        for (PluginLoaderImpl loader : LOADERS) {
            try { loader.close(); } catch (IOException ignored) {}
        }
        LOADERS.clear();
        PLUGINS.clear();
    }
}
