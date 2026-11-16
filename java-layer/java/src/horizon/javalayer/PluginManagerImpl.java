package horizon.javalayer;

import io.papermc.paper.plugin.PermissionManager;
import io.papermc.paper.plugin.configuration.PluginMeta;
import org.bukkit.event.Event;
import org.bukkit.event.EventPriority;
import org.bukkit.event.Listener;
import org.bukkit.permissions.Permissible;
import org.bukkit.permissions.Permission;
import org.bukkit.plugin.*;

import java.io.File;
import java.util.*;
import java.util.concurrent.ConcurrentHashMap;

/** Bukkit's plugin manager for the Java plugins (and the server's own, by name). */
public final class PluginManagerImpl implements PluginManager {
    static final PluginManagerImpl INSTANCE = new PluginManagerImpl();
    private static PluginManager facing;
    private static SimplePluginManager simple;

    /**
     * What `getPluginManager()` hands out: Bukkit's SimplePluginManager (LuckPerms and others check for it and hook its
     * permission maps), which hands every call to this one, as Paper's hands them to PaperPluginManagerImpl.
     */
    static synchronized PluginManager bukkitFacing() {
        if (facing == null) {
            try {
                org.bukkit.command.SimpleCommandMap map = new org.bukkit.command.SimpleCommandMap(Bridge.server(), new HashMap<>());
                SimplePluginManager made = new SimplePluginManager(Bridge.server(), map);
                made.paperPluginManager = INSTANCE;
                simple = made;
                facing = made;
            } catch (Throwable e) {
                Logs.warning("SimplePluginManager unavailable (" + e + "): plugins see the layer's own");
                facing = INSTANCE;
            }
        }
        return facing;
    }

    private final Map<String, Permission> permissions = new ConcurrentHashMap<>();

    @Override public void registerInterface(Class<? extends PluginLoader> loader) {}

    @Override
    public Plugin getPlugin(String name) {
        return Loader.byName(name);
    }

    @Override public Plugin[] getPlugins() { return Loader.all().toArray(new Plugin[0]); }

    @Override
    public boolean isPluginEnabled(String name) {
        Plugin p = getPlugin(name);
        if (p != null) return p.isEnabled();
        // A plugin of the server's own (a Rust one): enabled when the server knows it.
        Object raw = Native.tryStatic("PluginManager", "isPluginEnabled", new Object[]{name});
        return raw instanceof Boolean b && b;
    }

    @Override public boolean isPluginEnabled(Plugin plugin) { return plugin != null && plugin.isEnabled(); }

    /** A jar loaded after start (a plugin manager, a library a plugin downloads): loaded, not enabled, as on Paper. */
    @Override public Plugin loadPlugin(File file) throws InvalidPluginException, org.bukkit.plugin.UnknownDependencyException {
        return Loader.loadLate(file);
    }

    @Override public Plugin[] loadPlugins(File directory) {
        File[] jars = directory.listFiles((dir, name) -> name.endsWith(".jar"));
        return loadPlugins(jars == null ? new File[0] : jars);
    }

    @Override public Plugin[] loadPlugins(File[] files) {
        java.util.List<Plugin> out = new java.util.ArrayList<>();
        for (File f : files) {
            try {
                out.add(loadPlugin(f));
            } catch (Exception e) {
                Logs.severe("Could not load '" + f.getPath() + "'", e);
            }
        }
        return out.toArray(new Plugin[0]);
    }
    @Override public void disablePlugins() { Loader.disableAll(); }
    @Override public void clearPlugins() { Loader.disableAll(); }

    @Override public void callEvent(Event event) { Events.call(event); }

    @Override public void registerEvents(Listener listener, Plugin plugin) {
        if (!plugin.isEnabled()) throw new IllegalPluginAccessException("Plugin attempted to register " + listener + " while not enabled");
        Events.register(listener, plugin);
    }

    @Override
    public void registerEvent(Class<? extends Event> event, Listener listener, EventPriority priority, EventExecutor executor, Plugin plugin) {
        registerEvent(event, listener, priority, executor, plugin, false);
    }

    @Override
    public void registerEvent(Class<? extends Event> event, Listener listener, EventPriority priority, EventExecutor executor, Plugin plugin, boolean ignoreCancelled) {
        if (!plugin.isEnabled()) throw new IllegalPluginAccessException("Plugin attempted to register " + event + " while not enabled");
        Events.add(new Events.Reg(plugin, listener, event, priority, ignoreCancelled, executor));
    }

    @Override public void enablePlugin(Plugin plugin) { Loader.enable(plugin); }
    @Override public void disablePlugin(Plugin plugin) { Loader.disable(plugin); }

    /** Permissions live in SimplePluginManager's own maps (LuckPerms swaps in its own and reads them there). */
    private Map<String, Permission> permissions() {
        bukkitFacing();
        return simple != null ? simple.permissions : permissions;
    }

    private Set<Permission> defaults(boolean op) {
        bukkitFacing();
        if (simple == null) return new LinkedHashSet<>();
        return simple.defaultPerms.computeIfAbsent(op, k -> new LinkedHashSet<>());
    }

    @Override public Permission getPermission(String name) { return permissions().get(name.toLowerCase(Locale.ROOT)); }

    /** Moves on when the registered permissions change: the players' default permissions are worked out again. */
    static volatile int permissionsVersion;

    @Override
    public void addPermission(Permission permission) {
        String name = permission.getName().toLowerCase(Locale.ROOT);
        if (permissions().containsKey(name)) throw new IllegalArgumentException("The permission " + name + " is already defined!");
        permissions().put(name, permission);
        permissionsVersion++;
        if (permission.getDefault().getValue(true)) defaults(true).add(permission);
        if (permission.getDefault().getValue(false)) defaults(false).add(permission);
        Native.addPermission(permission.getName(), permission.getDefault().name().toLowerCase(Locale.ROOT).replace("not_op", "not op"), permission.getDescription());
        for (Map.Entry<String, Boolean> child : permission.getChildren().entrySet()) {
            if (child.getValue() && !permissions.containsKey(child.getKey().toLowerCase(Locale.ROOT))) {
                Native.addPermission(child.getKey(), permission.getDefault().name().toLowerCase(Locale.ROOT).replace("not_op", "not op"), "");
            }
        }
    }

    /** Paper 26.3's own permissions (bukkit.*, minecraft.command.*, paper's; resources/horizon/core-permissions.txt,
     *  read from a real Paper run: `name default child=value,...`), each kept as the plugin's where one has it. */
    void registerCorePermissions() {
        try (var in = PluginManagerImpl.class.getResourceAsStream("/horizon/core-permissions.txt")) {
            if (in == null) return;
            for (String line : new String(in.readAllBytes(), java.nio.charset.StandardCharsets.UTF_8).split("\\R")) {
                String[] parts = line.trim().split(" ");
                if (parts.length < 2 || parts[0].isEmpty()) continue;
                String name = parts[0].toLowerCase(Locale.ROOT);
                if (permissions().containsKey(name)) continue;
                Map<String, Boolean> children = new LinkedHashMap<>();
                if (parts.length > 2) for (String c : parts[2].split(",")) {
                    int eq = c.indexOf('=');
                    if (eq > 0) children.put(c.substring(0, eq), Boolean.parseBoolean(c.substring(eq + 1)));
                }
                org.bukkit.permissions.PermissionDefault def = org.bukkit.permissions.PermissionDefault.getByName(parts[1]);
                Permission permission = new Permission(name, def == null ? org.bukkit.permissions.PermissionDefault.OP : def, children);
                permissions().put(name, permission);
                if (permission.getDefault().getValue(true)) defaults(true).add(permission);
                if (permission.getDefault().getValue(false)) defaults(false).add(permission);
            }
        } catch (java.io.IOException ignored) {
        }
    }

    @Override public void removePermission(Permission permission) { removePermission(permission.getName()); }

    @Override
    public void removePermission(String name) {
        Permission gone = permissions().remove(name.toLowerCase(Locale.ROOT));
        permissionsVersion++;
        if (gone != null) {
            defaults(true).remove(gone);
            defaults(false).remove(gone);
        }
    }

    @Override
    public Set<Permission> getDefaultPermissions(boolean op) {
        return new LinkedHashSet<>(defaults(op));
    }

    @Override public void recalculatePermissionDefaults(Permission permission) {}
    @Override public void subscribeToPermission(String permission, Permissible permissible) {}
    @Override public void unsubscribeFromPermission(String permission, Permissible permissible) {}
    @Override public Set<Permissible> getPermissionSubscriptions(String permission) { return new HashSet<>(); }
    @Override public void subscribeToDefaultPerms(boolean op, Permissible permissible) {}
    @Override public void unsubscribeFromDefaultPerms(boolean op, Permissible permissible) {}
    @Override public Set<Permissible> getDefaultPermSubscriptions(boolean op) { return new HashSet<>(); }
    @Override public Set<Permission> getPermissions() { return new LinkedHashSet<>(permissions().values()); }
    @Override public boolean useTimings() { return false; }
    @Override public boolean isTransitiveDependency(PluginMeta a, PluginMeta b) { return a.getPluginDependencies().contains(b.getName()); }
    @Override public void clearPermissions() { permissions().clear(); permissionsVersion++; }
    @Override public void addPermissions(List<Permission> list) { for (Permission p : list) addPermission(p); }
    @Override public void overridePermissionManager(Plugin plugin, PermissionManager manager) {}
}
