package horizon.javalayer;

import org.bukkit.Bukkit;
import org.bukkit.Server;
import org.bukkit.UnsafeValues;
import org.bukkit.inventory.ItemFactory;
import org.bukkit.inventory.ItemStack;
import org.bukkit.inventory.meta.ItemMeta;
import org.bukkit.Material;

import java.io.File;
import java.lang.reflect.InvocationHandler;
import java.lang.reflect.Method;
import java.lang.reflect.Proxy;
import java.util.*;
import java.util.logging.Logger;

/** What the Rust side calls: start, a tick, an event, a command. */
public final class Bridge {
    private Bridge() {}

    private static Server server;
    private static File serverDir;

    public static Server server() { return server; }

    // ---- the server's calls ----

    private static volatile Thread mainThread;

    /** The thread the server's ticks (and start) run on: Paper's server thread. */
    public static Thread mainThread() {
        return mainThread != null ? mainThread : Thread.currentThread();
    }

    public static void start(String serverPath, String pluginsPath, String[] jars) {
        mainThread = Thread.currentThread();
        // Paper's main thread is "Server thread" (plugins check Thread.currentThread().getName()); the engine's own
        // name for it on the Rust side stays as it is.
        try { mainThread.setName("Server thread"); } catch (SecurityException ignored) {}
        try {
            Logs.install();
            serverDir = new File(serverPath);
            if (server == null) {
                // CraftBukkit's name and package (org.bukkit.craftbukkit.CraftServer): plugins read the version layout from it.
                server = new org.bukkit.craftbukkit.CraftServer();
                Bukkit.setServer(server);
                // Items an earlier layer saved under its own class name (`==: horizon.javalayer.HItemStack`) still load.
                org.bukkit.configuration.serialization.ConfigurationSerialization.registerClass(org.bukkit.inventory.ItemStack.class, "horizon.javalayer.HItemStack");
                // CraftBukkit's "ItemMeta" (SerializableMeta): item meta saved in the format before item components.
                org.bukkit.configuration.serialization.ConfigurationSerialization.registerClass(LegacyMeta.class, "ItemMeta");
            }
            long started = System.nanoTime();
            Loader.loadAll(new File(pluginsPath), serverDir, jars);
            Loader.enableAll();
            // Paper's COMMANDS lifecycle handlers run now, with every plugin enabled.
            Brigadier.startupDone();
            // `/plugins` and `/pl` list the Java plugins beside the HorizonsX ones (the engine's own sees only JavaLayer).
            PluginsCommand.register();
            long ms = (System.nanoTime() - started) / 1_000_000;
            int enabled = 0;
            for (var p : Loader.all()) if (p.isEnabled()) enabled++;
            Logs.info(enabled + " of " + jars.length + " Java plugin(s) enabled in " + ms + " ms");
        } catch (Throwable e) {
            Logs.severe("[JavaLayer] start failed", e);
        }
    }

    public static void stop() {
        try {
            Loader.disableAll();
            horizon.javalayer.nms.Hollow.save();
            Scheduler.INSTANCE.shutdown();
            Loader.closeAll();
        } catch (Throwable e) {
            Logs.severe("[JavaLayer] stop failed", e);
        }
    }

    /** A packet the game sends a player, through their channel's handlers (nms.PacketBridge.out). */
    public static String packetOut(String uuid, String hex) {
        try {
            return horizon.javalayer.nms.PacketBridge.out(uuid, hex);
        } catch (Throwable e) {
            Logs.severe("[JavaLayer] a packet through the player's channel", e);
            return "";
        }
    }

    /** A packet a player sent, through their channel's handlers (nms.PacketBridge.in). */
    public static String packetIn(String uuid, String hex) {
        try {
            return horizon.javalayer.nms.PacketBridge.in(uuid, hex);
        } catch (Throwable e) {
            Logs.severe("[JavaLayer] a packet from the player through their channel", e);
            return "";
        }
    }

    private static int ticks;
    private static boolean channelChecks = true;

    /**
     * Moves on with each tick and each event: what the game's thread read of the server (a player's world) holds
     * until then, as nothing in the game moves between (Players.invoke).
     */
    static volatile long epoch;

    /** The server's ticks so far (what is checked once a tick: a player's op status, Players.invoke). */
    static volatile long tickCount;

    /** Events fired into Java by class: [count, nanoseconds], for the 30 s report of the most fired (20 a second or more). */
    private static final java.util.Map<String, long[]> FIRED = new java.util.concurrent.ConcurrentHashMap<>();
    private static long firedSince = System.nanoTime();

    private static void reportFired() {
        firedSince = System.nanoTime();
        java.util.List<java.util.Map.Entry<String, long[]>> rows = new java.util.ArrayList<>(FIRED.entrySet());
        FIRED.clear();
        rows.sort((a, b) -> Long.compare(b.getValue()[0], a.getValue()[0]));
        for (int i = 0; i < Math.min(5, rows.size()); i++) {
            long[] row = rows.get(i).getValue();
            if (row[0] < 600) break;
            String name = rows.get(i).getKey();
            Native.log(0, "[events] " + name.substring(name.lastIndexOf('.') + 1) + ": " + row[0] + " in 30 s, " + row[1] / 1_000_000 + " ms in Java");
        }
    }

    public static void tick() {
        epoch++;
        tickCount++;
        Watchdog.enter("a server tick's tasks");
        try {
            if (System.nanoTime() - firedSince > 30_000_000_000L) reportFired();
            AsyncChunks.tick();
            // Seats a plugin's NMS entity took players onto (nms.NmsEntities), once the Java server's classes are up.
            if (channelChecks) {
                try {
                    if (horizon.javalayer.nms.Hollow.booted()) {
                        horizon.javalayer.nms.NmsEntities.tick();
                        horizon.javalayer.nms.PacketBridge.tick();
                    }
                } catch (LinkageError e) {
                    channelChecks = false;
                }
            }
            Scheduler.INSTANCE.heartbeat();
            // Each second: whether a plugin put handlers in the players' channels (nms.PacketBridge), only once the
            // Java server's classes are up (a plugin reached for them).
            if (channelChecks && ++ticks % 20 == 0) {
                try {
                    if (horizon.javalayer.nms.Hollow.booted()) horizon.javalayer.nms.PacketBridge.check();
                } catch (LinkageError e) {
                    channelChecks = false; // no Java server classes on this server (lib/server)
                }
            }
        } catch (Throwable e) {
            Logs.severe("[JavaLayer] tick", e);
        } finally {
            Watchdog.leave();
        }
    }

    public static void fire(String className, Object handle, int slot) {
        epoch++;
        Watchdog.enter("event " + className);
        long started = System.nanoTime();
        try {
            if (handle instanceof H h) Events.fire(className, h, slot);
        } finally {
            long[] row = FIRED.computeIfAbsent(className, k -> new long[2]);
            row[0]++;
            row[1] += System.nanoTime() - started;
            Watchdog.leave();
        }
    }

    public static boolean command(String owner, String name, Object sender, String label, String[] args) {
        Watchdog.enter("command /" + label);
        try {
            return Commands.MAP.execute(owner, name, sender, label, args);
        } catch (Throwable e) {
            Logs.severe("[JavaLayer] command " + label, e);
            return true;
        } finally {
            Watchdog.leave();
        }
    }

    public static String[] tab(String owner, String name, Object sender, String alias, String[] args) {
        try {
            return Commands.MAP.tab(owner, name, sender, alias, args).toArray(new String[0]);
        } catch (Throwable e) {
            Logs.severe("[JavaLayer] tab " + alias, e);
            return new String[0];
        }
    }

    /** The SDK calling a Java object it was handed: `name` ("*": its one abstract method), or `$instanceof`. */
    public static Object invokeRef(long id, String name, Object[] args) throws Throwable {
        Object target = Conv.refObject(id);
        if (target == null) throw new IllegalStateException("Java object #" + id + " is gone");
        if (name.equals("$instanceof")) return Calls.isA(target.getClass(), String.valueOf(args[0]));
        // A world generator, biome provider or populator, as the server's generator calls it.
        if (name.equals("$gen")) return Generators.call(target, args);
        // A Java MapRenderer, drawn by the SDK's map loop (MapRenderers).
        if (name.equals("$map") && target instanceof org.bukkit.map.MapRenderer r) return MapRenderers.render(r, args);
        // A mob goal's key, entity class and types, read once as it is added (Paper's MobGoals does too).
        if (name.equals("$goal") && target instanceof com.destroystokyo.paper.entity.ai.Goal<?> g) {
            java.util.List<String> types = new java.util.ArrayList<>();
            for (Object t : g.getTypes()) types.add(((Enum<?>) t).name());
            return Conv.toNative(java.util.List.of(g.getKey().getNamespacedKey().toString(), g.getKey().getEntityClass().getName(), types));
        }
        java.lang.reflect.Method m = Calls.find(target, name, args == null ? 0 : args.length);
        if (m == null) throw new NoSuchMethodException(target.getClass().getName() + "." + name + "/" + (args == null ? 0 : args.length));
        java.lang.reflect.Type[] types = m.getGenericParameterTypes();
        Object[] jargs = new Object[types.length];
        for (int i = 0; i < types.length; i++) jargs[i] = Conv.fromNative(args[i], types[i]);
        try {
            return Conv.toNative(m.invoke(target, jargs));
        } catch (java.lang.reflect.InvocationTargetException e) {
            throw e.getCause();
        }
    }

    // ---- Bukkit.getServer() ----

    // Paper 26.3 build 235's own strings (CraftServer.getVersion / getBukkitVersion): plugins parse them for version checks.
    static final String VERSION = Services.MC + "-235-bb10cf0 (MC: " + Services.MC + ")";
    static final String BUKKIT_VERSION = Services.MC + ".build.235-stable";
    private static final ServerHandler SERVER_HANDLER = new ServerHandler();

    /** A server call answered the usual way (the SDK's Bukkit/Server function), raw: for handlers that add to it. */
    static Object nativeServer(String name, Object[] args) {
        Object[] nargs = Conv.args(args == null ? new Object[0] : args);
        Object raw = Native.tryStatic("Bukkit", name, nargs);
        if (raw == Native.MISSING) raw = Native.tryStatic("Server", name, nargs);
        return raw == Native.MISSING ? null : raw;
    }

    /** CraftServer's methods. */
    public static Object serverCall(Object self, Method m, Object[] args) {
        try {
            return SERVER_HANDLER.invoke(self, m, args);
        } catch (RuntimeException | Error e) {
            throw e;
        } catch (Throwable t) {
            throw Players.sneaky(t);
        }
    }

    /** `Server.spigot()`: spigot.yml as its configuration (read-only, as plugins use it). */
    static final Server.Spigot SPIGOT = new Server.Spigot() {
        private org.bukkit.configuration.file.YamlConfiguration config;

        @Override
        public org.bukkit.configuration.file.YamlConfiguration getConfig() {
            if (config == null) {
                java.io.File file = new java.io.File(serverDir == null ? new java.io.File(".") : serverDir, "spigot.yml");
                config = file.isFile() ? org.bukkit.configuration.file.YamlConfiguration.loadConfiguration(file) : new org.bukkit.configuration.file.YamlConfiguration();
                config.addDefault("settings.bungeecord", false);
                config.addDefault("settings.restart-on-crash", false);
                config.addDefault("settings.timeout-time", 60);
                config.addDefault("messages.unknown-command", "Unknown command. Type \"/help\" for help.");
                config.addDefault("world-settings.default.entity-tracking-range.players", 48);
                config.options().copyDefaults(true);
            }
            return config;
        }

        @Override public org.bukkit.configuration.file.YamlConfiguration getBukkitConfig() { return getConfig(); }
        @Override public org.bukkit.configuration.file.YamlConfiguration getSpigotConfig() { return getConfig(); }
        @Override public org.bukkit.configuration.file.YamlConfiguration getPaperConfig() { return new org.bukkit.configuration.file.YamlConfiguration(); }
        @Override public void broadcast(net.md_5.bungee.api.chat.BaseComponent component) { org.bukkit.Bukkit.broadcastMessage(component.toLegacyText()); }
        @Override public void broadcast(net.md_5.bungee.api.chat.BaseComponent... components) { org.bukkit.Bukkit.broadcastMessage(net.md_5.bungee.api.chat.BaseComponent.toLegacyText(components)); }
        @Override public void restart() {}
    };

    static final class ServerHandler implements InvocationHandler {
        private final Logger logger = Logger.getLogger("Minecraft");
        // A proxy, not a class named CraftMagicNumbers: Paper's own (getItem, getBlock...) stays for plugins that call it.
        private final Object unsafe = Proxy.newProxyInstance(Bridge.class.getClassLoader(), new Class<?>[]{UnsafeValues.class}, new UnsafeHandler());
        private final Object itemFactory = Proxy.newProxyInstance(Bridge.class.getClassLoader(), new Class<?>[]{ItemFactory.class}, new FactoryHandler());
        private final Object messenger = Services.inert(org.bukkit.plugin.messaging.Messenger.class, "Messenger");
        private final Object helpMap = HelpMapImpl.INSTANCE;

        @Override
        public Object invoke(Object proxy, Method m, Object[] args) throws Throwable {
            String name = m.getName();
            int n = args == null ? 0 : args.length;
            // Paper's STARTUP plugins enable before the worlds are loaded (Loader.startupPhase).
            if (Loader.startupPhase) {
                if (name.equals("getWorlds") && n == 0) return new java.util.ArrayList<org.bukkit.World>();
                if (name.equals("getWorld") && n == 1) return null;
            }
            switch (name) {
                case "equals": return proxy == args[0];
                case "hashCode": return System.identityHashCode(proxy);
                case "toString": return "CraftServer{serverName=Horizon,serverVersion=" + VERSION + ",minecraftVersion=" + Services.MC + "}";
                case "getName": return "Paper";
                case "getVersion": return VERSION;
                case "getBukkitVersion": return BUKKIT_VERSION;
                case "getMinecraftVersion": return Services.MC;
                case "getLogger": return logger;
                case "getPluginManager": return PluginManagerImpl.bukkitFacing();
                case "getScheduler": return Scheduler.INSTANCE;
                case "getServicesManager": return ServicesImpl.INSTANCE;
                case "getUnsafe": return unsafe;
                case "getItemFactory": return itemFactory;
                case "getMessenger": return messenger;
                case "getHelpMap": return helpMap;
                case "getCommandMap": return Commands.MAP;
                case "getPluginCommand": return n == 1 ? Commands.MAP.pluginCommand((String) args[0]) : null;
                case "isPrimaryThread": return Native.isMainThread();
                case "getPluginsFolder": return Loader.pluginsDir;
                case "getUpdateFolder": return "update";
                case "getUpdateFolderFile": return new File(Loader.pluginsDir, "update");
                case "getWorldContainer": if (n == 0) return serverDir; break;
                case "isStopping": return false;
                case "reload": case "reloadData": case "reloadPermissions": case "reloadCommandAliases": return null;
                case "getRegistry": if (n == 1) return new Registries().getRegistry((Class) args[0]); break;
                case "spigot": return SPIGOT;
                case "addRecipe": if (n >= 1 && args[0] instanceof org.bukkit.inventory.Recipe r) return Recipes.add(r); break;
                // The plugins' own recipes are the server's too (CraftServer lists what addRecipe added).
                case "recipeIterator":
                    if (n == 0) return Recipes.iterator((java.util.Iterator<org.bukkit.inventory.Recipe>) Conv.fromNative(nativeServer(name, args), m.getGenericReturnType()));
                    break;
                case "getRecipe":
                    if (n == 1 && args[0] instanceof org.bukkit.NamespacedKey key && Recipes.added(key) != null) return Recipes.added(key);
                    break;
                case "getRecipesFor":
                    if (n == 1 && args[0] instanceof org.bukkit.inventory.ItemStack stack) return Recipes.forResult((List<org.bukkit.inventory.Recipe>) Conv.fromNative(nativeServer(name, args), m.getGenericReturnType()), stack);
                    break;
                case "removeRecipe":
                    if (n >= 1 && args[0] instanceof org.bukkit.NamespacedKey key) Recipes.forget(key);
                    break;
                case "resetRecipes":
                case "clearRecipes":
                    Recipes.forgetAll();
                    break;
                case "suggestPlayerNamesWhenNullTabCompletions": return true;
                case "getPermissionMessage": return "§cI'm sorry, but you do not have permission to perform this command. Please contact the server administrators if you believe that this is in error.";
                case "permissionMessage": return net.kyori.adventure.text.Component.text("I'm sorry, but you do not have permission to perform this command. Please contact the server administrators if you believe that this is in error.", net.kyori.adventure.text.format.NamedTextColor.RED);
                case "getCurrentTick": return (int) Scheduler.INSTANCE.currentTick();
                case "createProfile":
                case "createPlayerProfile":
                    if (n == 2) {
                        Object raw = Native.tryStatic("Bukkit", name, Conv.args(args));
                        if (raw == Native.MISSING) raw = Native.tryStatic("Bukkit", name, Conv.args(new Object[]{args[0] != null ? args[0] : args[1]}));
                        if (raw != Native.MISSING) return Conv.fromNative(raw, m.getGenericReturnType());
                    }
                    break;
                case "getTag": if (n == 3) return Tags.get(String.valueOf(args[0]), (org.bukkit.NamespacedKey) args[1], (Class) args[2]); break;
                case "getTags": if (n == 2) return Tags.all(String.valueOf(args[0]), (Class) args[1]); break;
                case "getGlobalRegionScheduler": return Folia.GLOBAL;
                case "getRegionScheduler": return Folia.REGION;
                case "getAsyncScheduler": return Folia.ASYNC;
                case "isOwnedByCurrentRegion": return Native.isMainThread();
                case "isGlobalTickThread": return Native.isMainThread();
                default: break;
            }
            Object shaped = Shapes.server(name, args, m.getReturnType());
            if (shaped != Shapes.NONE) return shaped;
            Object[] nargs = Conv.args(args);
            Object raw = Native.tryStatic("Bukkit", name, nargs);
            if (raw == Native.MISSING) raw = Native.tryStatic("Server", name, nargs);
            Object[] text = raw == Native.MISSING ? Conv.keysAsText(nargs) : null;
            if (text != null) {
                raw = Native.tryStatic("Bukkit", name, text);
                if (raw == Native.MISSING) raw = Native.tryStatic("Server", name, text);
            }
            if (raw == Native.MISSING) {
                if (m.isDefault() && Proxy.isProxyClass(proxy.getClass())) return InvocationHandler.invokeDefault(proxy, m, args);
                String where = "Server." + name + "/" + n;
                NativeHandler.missing(where);
                throw Report.cant(where);
            }
            return Conv.fromNative(raw, m.getGenericReturnType());
        }
    }

    /** `Bukkit.getUnsafe()`: versions and legacy material conversion. */
    static final class UnsafeHandler implements InvocationHandler {
        @Override
        public Object invoke(Object proxy, Method m, Object[] args) throws Throwable {
            String name = m.getName();
            int n = args == null ? 0 : args.length;
            switch (name) {
                case "equals": return proxy == args[0];
                case "hashCode": return System.identityHashCode(proxy);
                case "toString": return "org.bukkit.craftbukkit.util.CraftMagicNumbers@" + Integer.toHexString(System.identityHashCode(proxy));
                case "toLegacy": case "fromLegacy":
                    if (n >= 1 && args[0] instanceof Material mat) {
                        if (m.getReturnType() == org.bukkit.block.data.BlockData.class) return mat.createBlockData();
                        if (m.getReturnType() == org.bukkit.material.MaterialData.class) return new org.bukkit.material.MaterialData(mat);
                        return mat;
                    }
                    if (n >= 1 && args[0] instanceof org.bukkit.material.MaterialData data) {
                        if (m.getReturnType() == org.bukkit.block.data.BlockData.class) return data.getItemType().createBlockData();
                        return data.getItemType();
                    }
                    break;
                case "isSupportedApiVersion": return true;
                case "checkSupported": return null;
                case "getDataVersion": {
                    Object raw = Native.tryStatic("Bukkit", "getDataVersion", new Object[0]);
                    if (raw instanceof Number i) return i.intValue();
                    // The server's own world data version (SharedConstants' DATA_VERSION).
                    java.util.regex.Matcher dm = java.util.regex.Pattern.compile("\"data_version\":(\\d+)").matcher(String.valueOf(Native.query("{\"op\":\"version\"}")));
                    return dm.find() ? Integer.parseInt(dm.group(1)) : 5023;
                }
                case "getProtocolVersion": {
                    Object raw = Native.tryStatic("Bukkit", "getProtocolVersion", new Object[0]);
                    if (raw instanceof Number i) return i.intValue();
                    // The server's own (26.3: 777), as CraftMagicNumbers gives SharedConstants' protocol version.
                    java.util.regex.Matcher pm = java.util.regex.Pattern.compile("\"protocol_version\":(\\d+)").matcher(String.valueOf(Native.query("{\"op\":\"version\"}")));
                    return pm.find() ? Integer.parseInt(pm.group(1)) : 777;
                }
                case "getMainLevelName": return "world";
                case "serializeItemAsJson":
                    // Gson's JsonObject, as Paper gives it: the item's codec form and the data version it was saved at.
                    if (n == 1 && args[0] instanceof org.bukkit.inventory.ItemStack stack) {
                        // ItemStack.CODEC's JSON: id, count, and each component in the game's JSON form.
                        com.google.gson.JsonObject out = new com.google.gson.JsonObject();
                        out.addProperty("id", stack.getType().getKey().toString());
                        out.addProperty("count", stack.getAmount());
                        Object h = Conv.toNative(stack);
                        String all = h instanceof H ? Native.itemToJson(h) : null;
                        com.google.gson.JsonObject components = new com.google.gson.JsonObject();
                        com.google.gson.JsonElement listed = all == null ? null : com.google.gson.JsonParser.parseString(all).getAsJsonObject().get("components");
                        if (listed != null && listed.isJsonObject()) for (String id : listed.getAsJsonObject().keySet()) {
                            String one = Native.componentJson(h, id);
                            if (one != null) components.add(id, PaperMeta.componentValue(id, com.google.gson.JsonParser.parseString(one)));
                        }
                        if (!components.isEmpty()) out.add("components", components);
                        String version = Native.query("{\"op\":\"items_data_version\"}");
                        try {
                            out.addProperty("DataVersion", Integer.parseInt(version.trim()));
                        } catch (RuntimeException e) {
                            out.addProperty("DataVersion", Bukkit.getUnsafe().getDataVersion());
                        }
                        return out;
                    }
                    break;
                case "deserializeItemFromJson":
                    if (n == 1 && args[0] instanceof com.google.gson.JsonObject json) {
                        if (!json.has("id")) throw new IllegalArgumentException("Not a valid item: " + json);
                        org.bukkit.Material type = org.bukkit.Material.matchMaterial(json.get("id").getAsString());
                        if (type == null) throw new IllegalArgumentException("Unknown item: " + json.get("id"));
                        org.bukkit.inventory.ItemStack made = new org.bukkit.inventory.ItemStack(type, json.has("count") ? json.get("count").getAsInt() : 1);
                        Object h = Conv.toNative(made);
                        if (json.has("components") && json.get("components").isJsonObject() && h instanceof H) {
                            for (var e : json.getAsJsonObject("components").entrySet()) Native.setComponentJson(h, e.getKey(), e.getValue().toString());
                        }
                        return made;
                    }
                    break;
                case "get":
                    // Paper's enum compatibility (Commodore turns Sound.valueOf("X") and the like into this).
                    if (n == 2 && args[0] instanceof Class<?> type && args[1] instanceof String value) return Legacy.renamedField(type, value);
                    if (n == 2 && args[0] instanceof io.papermc.paper.registry.RegistryKey<?> key && args[1] instanceof org.bukkit.NamespacedKey id) {
                        return io.papermc.paper.registry.RegistryAccess.registryAccess().getRegistry((io.papermc.paper.registry.RegistryKey) key).get(id);
                    }
                    break;
                default: break;
            }
            Object raw = Native.tryStatic("UnsafeValues", name, Conv.args(args));
            if (raw == Native.MISSING) {
                // The server's own UnsafeValues object answers what has no static form (deserializeStack...).
                Object nativeUnsafe = Native.tryStatic("Bukkit", "getUnsafe", new Object[0]);
                if (nativeUnsafe != Native.MISSING && nativeUnsafe != null) raw = Native.tryCall(nativeUnsafe, name, Conv.args(args));
            }
            if (raw == Native.MISSING) {
                if (m.isDefault()) return InvocationHandler.invokeDefault(proxy, m, args);
                String where = "UnsafeValues." + name + "/" + n;
                NativeHandler.missing(where);
                throw Report.cant(where);
            }
            return Conv.fromNative(raw, m.getGenericReturnType());
        }
    }

    /** `Bukkit.getItemFactory()`: metas come from a server item of that material. */
    static final class FactoryHandler implements InvocationHandler {
        @Override
        public Object invoke(Object proxy, Method m, Object[] args) throws Throwable {
            String name = m.getName();
            int n = args == null ? 0 : args.length;
            switch (name) {
                case "equals":
                    if (n == 1) return proxy == args[0];
                    if (n == 2) return Objects.equals(args[0], args[1]);
                    break;
                case "hashCode": return System.identityHashCode(proxy);
                case "toString": return "ItemFactory";
                case "getItemMeta":
                    if (n == 1 && args[0] instanceof Material mat) return Items.create(mat, 1).getItemMeta();
                    break;
                case "isApplicable": return true;
                case "asMetaFor":
                    if (n == 2 && args[1] instanceof ItemStack stack) {
                        ItemStack copy = stack.clone();
                        copy.setItemMeta((ItemMeta) args[0]);
                        return copy.getItemMeta();
                    }
                    if (n == 2 && args[1] instanceof Material mat) {
                        ItemStack copy = Items.create(mat, 1);
                        copy.setItemMeta((ItemMeta) args[0]);
                        return copy.getItemMeta();
                    }
                    break;
                case "getDefaultLeatherColor": return org.bukkit.Color.fromRGB(0xA06540);
                case "ensureServerConversions": return args[0];
                default: break;
            }
            Object raw = Native.tryStatic("ItemFactory", name, Conv.args(args));
            if (raw == Native.MISSING) {
                if (m.isDefault()) return InvocationHandler.invokeDefault(proxy, m, args);
                String where = "ItemFactory." + name + "/" + n;
                NativeHandler.missing(where);
                throw Report.cant(where);
            }
            return Conv.fromNative(raw, m.getGenericReturnType());
        }
    }
}
