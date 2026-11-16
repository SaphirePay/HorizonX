package horizon.javalayer.nms;

import horizon.javalayer.Craft;
import io.netty.buffer.ByteBuf;
import io.netty.buffer.Unpooled;
import net.minecraft.SharedConstants;
import net.minecraft.core.LayeredRegistryAccess;
import net.minecraft.core.RegistryAccess;
import net.minecraft.network.Connection;
import net.minecraft.network.ProtocolInfo;
import net.minecraft.network.RegistryFriendlyByteBuf;
import net.minecraft.network.codec.StreamCodec;
import net.minecraft.network.protocol.BundlePacket;
import net.minecraft.network.protocol.Packet;
import net.minecraft.network.protocol.game.ClientGamePacketListener;
import net.minecraft.network.protocol.game.ClientboundBundleDelimiterPacket;
import net.minecraft.network.protocol.game.GameProtocols;
import net.minecraft.resources.RegistryDataLoader;
import net.minecraft.server.Bootstrap;
import net.minecraft.server.MinecraftServer;
import net.minecraft.server.RegistryLayer;
import net.minecraft.server.dedicated.DedicatedServer;
import net.minecraft.server.level.ServerLevel;
import net.minecraft.server.level.ServerPlayer;
import net.minecraft.server.network.ServerCommonPacketListenerImpl;
import net.minecraft.server.network.ServerGamePacketListenerImpl;
import net.minecraft.server.packs.repository.ServerPacksSource;
import net.minecraft.world.entity.Entity;
import net.minecraft.world.phys.Vec3;
import org.bukkit.craftbukkit.entity.CraftPlayer;

import java.util.Map;
import java.util.UUID;
import java.util.concurrent.ConcurrentHashMap;
import java.util.stream.Stream;

/**
 * The Java server's own objects for plugins that reach past the API (NMS): vanilla bootstrapped in this JVM with
 * its real registries, a DedicatedServer with no game behind it (MinecraftServer.getServer(), registryAccess()),
 * and for each player a ServerPlayer whose connection encodes packets with vanilla's own codecs and hands the
 * bytes to the Rust server for that player. Paid only when a plugin first uses one (about 5 s and 180 MB).
 */
public final class Hollow {
    private Hollow() {}

    private static volatile boolean booted;
    private static io.papermc.paper.configuration.PaperConfigurations paperConfigs;
    private static RegistryAccess.Frozen access;
    private static DedicatedServer server;
    private static ProtocolInfo<ClientGamePacketListener> clientbound;
    private static ProtocolInfo<net.minecraft.network.protocol.game.ServerGamePacketListener> serverbound;

    /** The game's play protocol to players (what vanilla's PacketEncoder in a player's channel holds). */
    static ProtocolInfo<ClientGamePacketListener> clientbound() {
        boot();
        return clientbound;
    }

    /**
     * The game's play protocol from players (what vanilla's PacketDecoder holds). Creative-only packets are read for
     * everyone: the Rust server has let the packet through already.
     */
    static ProtocolInfo<net.minecraft.network.protocol.game.ServerGamePacketListener> serverbound() {
        boot();
        return serverbound;
    }
    private static final Map<UUID, ServerPlayer> PLAYERS = new ConcurrentHashMap<>();
    private static final Map<String, ServerLevel> LEVELS = new ConcurrentHashMap<>();

    public static boolean booted() { return booted; }

    /** The game's registries, as the hollow server has them (an entity's saved tree reads its item and data registries). */
    public static RegistryAccess.Frozen registries() {
        boot();
        return access;
    }

    public static synchronized void boot() {
        if (booted) return;
        long t = System.nanoTime();
        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();
        // Paper's settings at their defaults (paper-global.yml as a new server writes it): vanilla's classes read them.
        // After the bootstrap: some sections' defaults name registry entries.
        try {
            java.lang.reflect.Method set = io.papermc.paper.configuration.GlobalConfiguration.class.getDeclaredMethod("set", io.papermc.paper.configuration.GlobalConfiguration.class);
            set.setAccessible(true);
            set.invoke(null, defaults(new io.papermc.paper.configuration.GlobalConfiguration()));
        } catch (ReflectiveOperationException | RuntimeException | LinkageError e) {
            horizon.javalayer.Native.log(1, "[JavaLayer] Paper's global settings could not be made: " + e);
        }
        try {
            var vanilla = ServerPacksSource.createVanillaPackSource();
            var rm = new net.minecraft.server.packs.resources.MultiPackResourceManager(net.minecraft.server.packs.PackType.SERVER_DATA, java.util.List.of(vanilla.fullResources()));
            LayeredRegistryAccess<RegistryLayer> layers = RegistryLayer.createRegistryAccess();
            var pending = net.minecraft.tags.TagLoader.loadTagsForExistingRegistries(rm, layers.getLayer(RegistryLayer.STATIC));
            var lookups = net.minecraft.tags.TagLoader.buildUpdatedLookups(layers.getAccessForLoading(RegistryLayer.WORLD), pending);
            var world = RegistryDataLoader.load(rm, lookups, RegistryDataLoader.WORLD_REGISTRIES, Runnable::run).get();
            var lookups2 = Stream.concat(lookups.stream(), world.listRegistries()).toList();
            var dims = RegistryDataLoader.load(rm, lookups2, RegistryDataLoader.DIMENSION_REGISTRIES, Runnable::run).get();
            layers = layers.replaceFrom(RegistryLayer.WORLD, world, dims);
            pending.forEach(net.minecraft.core.Registry.PendingTags::apply);
            access = layers.compositeAccess();
            // Items' (and others') default data components, as ReloadableServerResources binds them: without them
            // ItemStack.CODEC refuses every item ("does not have components yet"), so NMS item copies were air.
            try {
                for (var components : net.minecraft.core.registries.BuiltInRegistries.DATA_COMPONENT_INITIALIZERS.build(access)) components.apply();
            } catch (RuntimeException | LinkageError e) {
                horizon.javalayer.Native.log(1, "[JavaLayer] vanilla's default item components could not be bound: " + e);
            }
            DedicatedServer s = Craft.allocate(DedicatedServer.class);
            Craft.set(s, MinecraftServer.class, "registries", layers);
            Craft.set(s, MinecraftServer.class, "server", horizon.javalayer.Bridge.server());
            Craft.set(null, MinecraftServer.class, "SERVER", s);
            // The server thread (isSameThread, getRunningThread): the one the layer's ticks run on.
            Craft.set(s, MinecraftServer.class, "serverThread", horizon.javalayer.Bridge.mainThread());
            server = s;
            fill(s);
            // Its network side as packet libraries look it up (PacketEvents: the first List field of
            // getConnection()): a listener with vanilla's lists, empty, no channel of its own.
            try {
                var listener = Craft.allocate(net.minecraft.server.network.ServerConnectionListener.class);
                Craft.set(listener, net.minecraft.server.network.ServerConnectionListener.class, "server", s);
                // One listening channel's future, as Paper has its socket's: PacketEvents finds the list by the
                // ChannelFuture in it and adds its handler to that channel (an in-memory one: it accepts nobody).
                java.util.List<io.netty.channel.ChannelFuture> channels = java.util.Collections.synchronizedList(new java.util.ArrayList<>());
                try {
                    channels.add(new io.netty.channel.embedded.EmbeddedChannel().newSucceededFuture());
                } catch (RuntimeException | LinkageError e) {
                    horizon.javalayer.Native.log(1, "[JavaLayer] no in-memory channel for the hollow server: " + e);
                }
                Craft.set(listener, net.minecraft.server.network.ServerConnectionListener.class, "channels", channels);
                Craft.set(listener, net.minecraft.server.network.ServerConnectionListener.class, "connections", java.util.Collections.synchronizedList(new java.util.ArrayList<>()));
                Craft.set(listener, net.minecraft.server.network.ServerConnectionListener.class, "pendingConnections", new java.util.concurrent.ConcurrentLinkedQueue<>());
                Craft.set(s, MinecraftServer.class, "connection", listener);
            } catch (RuntimeException | LinkageError e) {
                horizon.javalayer.Native.log(1, "[JavaLayer] the hollow server's connection listener could not be made: " + e);
            }
            clientbound = GameProtocols.CLIENTBOUND_TEMPLATE.bind(RegistryFriendlyByteBuf.decorator(access));
            serverbound = GameProtocols.SERVERBOUND_TEMPLATE.bind(RegistryFriendlyByteBuf.decorator(access), new GameProtocols.Context() {
                @Override public boolean hasInfiniteMaterials() { return true; }
                @Override public boolean canUseCommandBlocks() { return true; }
            });
            // Entities a plugin makes itself (fake players, holograms) take ids far above the server's own.
            try {
                java.util.concurrent.atomic.AtomicInteger counter = (java.util.concurrent.atomic.AtomicInteger) Craft.get(null, Entity.class, "ENTITY_COUNTER");
                if (counter != null && counter.get() < 1_000_000_000) counter.set(1_000_000_000);
            } catch (RuntimeException ignored) {}
        } catch (Exception e) {
            throw new IllegalStateException("vanilla's registries could not load", e);
        }
        // CraftServer.console, as Paper has it (PacketEvents reads the vanilla server from it by type).
        try {
            if (horizon.javalayer.Bridge.server() instanceof org.bukkit.craftbukkit.CraftServer cs) cs.console = (DedicatedServer) server;
        } catch (RuntimeException | LinkageError ignored) {
        }
        // The game's command dispatcher, for plugins that register their commands on it (BetterHud).
        horizon.javalayer.VanillaCommands.install(server, access);
        // Spigot's settings as Paper loads them (SpigotConfig.config, the server's spigot.yml): WorldEdit's version
        // adapter writes one of its keys when it loads.
        try {
            if (org.spigotmc.SpigotConfig.config == null) {
                java.io.File file = new java.io.File(org.bukkit.Bukkit.getWorldContainer(), "spigot.yml");
                org.spigotmc.SpigotConfig.config = org.bukkit.configuration.file.YamlConfiguration.loadConfiguration(file);
                // Where it's saved (Spigot writes the defaults a world's settings read back, as a Paper server does).
                Craft.set(null, org.spigotmc.SpigotConfig.class, "CONFIG_FILE", file);
            }
        } catch (RuntimeException | LinkageError e) {
            horizon.javalayer.Native.log(1, "[JavaLayer] Spigot's settings could not be read: " + e);
        }
        // Paper's own settings as its start makes them (config/paper-global.yml, paper-world-defaults.yml, which the
        // layer writes): what the game's classes read of them (item obfuscation when a packet is written, a new
        // entity's settings) is there, as on Paper.
        try {
            // The pool Paper's chat-threads setting sizes (its settings' post-processing reaches for it).
            Craft.set(server, MinecraftServer.class, "chatExecutor", new java.util.concurrent.ThreadPoolExecutor(0, 2, 60, java.util.concurrent.TimeUnit.SECONDS, new java.util.concurrent.LinkedBlockingQueue<>(), r -> {
                Thread thread = new Thread(r, "Async Chat Thread (hollow)");
                thread.setDaemon(true);
                return thread;
            }));
            java.nio.file.Path root = org.bukkit.Bukkit.getWorldContainer().getAbsoluteFile().toPath();
            paperConfigs = io.papermc.paper.configuration.PaperConfigurations.setup(root.resolve("paper.yml"), root.resolve("config"), root, root.resolve("spigot.yml").toFile());
            paperConfigs.initializeGlobalConfiguration(access);
            paperConfigs.initializeWorldDefaultsConfiguration(access);
        } catch (Exception | LinkageError e) {
            paperConfigs = null;
            Throwable root = e;
            while (root.getCause() != null && root.getCause() != root) root = root.getCause();
            horizon.javalayer.Native.log(1, "[JavaLayer] Paper's settings could not be read: " + e + (root != e ? " (" + root + " at " + (root.getStackTrace().length > 0 ? root.getStackTrace()[0] : "?") + ")" : ""));
        }
        booted = true;
        // The server's levels and storage, as plugins reach them (getAllLevels, getWorldPath: VexBot, VoxyServerSide).
        try {
            Craft.set(server, MinecraftServer.class, "levels", new java.util.concurrent.ConcurrentSkipListMap<Object, ServerLevel>(java.util.Comparator.comparing(String::valueOf)));
            java.nio.file.Path root = java.nio.file.Path.of(org.bukkit.Bukkit.getWorldContainer().getAbsolutePath(), "world");
            String levelId = "world";
            if (!org.bukkit.Bukkit.getWorlds().isEmpty()) {
                java.io.File overworld = org.bukkit.Bukkit.getWorlds().get(0).getWorldFolder().getAbsoluteFile();
                root = overworld.toPath().normalize().getParent().getParent().getParent();
                levelId = root.getFileName().toString();
            }
            var storage = Craft.allocate(net.minecraft.world.level.storage.LevelStorageSource.LevelStorageAccess.class);
            Craft.set(storage, net.minecraft.world.level.storage.LevelStorageSource.LevelStorageAccess.class, "levelDirectory", new net.minecraft.world.level.storage.LevelStorageSource.LevelDirectory(root));
            Craft.set(storage, net.minecraft.world.level.storage.LevelStorageSource.LevelStorageAccess.class, "levelId", levelId);
            Craft.set(storage, net.minecraft.world.level.storage.LevelStorageSource.LevelStorageAccess.class, "resources", new java.util.concurrent.ConcurrentHashMap<>());
            Craft.set(server, MinecraftServer.class, "storageSource", storage);
            // The server's saved data (getDataStorage(): <world>/data, as vanilla keeps it; journeymap's world id).
            Craft.set(server, MinecraftServer.class, "savedDataStorage", new net.minecraft.world.level.storage.SavedDataStorage(root.resolve("data"), net.minecraft.util.datafix.DataFixers.getDataFixer(), access));
            for (org.bukkit.World w : org.bukkit.Bukkit.getWorlds()) level(w);
            // The server's player list (getPlayerList(): VexBot, journeymap walk its players each tick).
            Craft.set(server, MinecraftServer.class, "playerList", playerList());
        } catch (RuntimeException | LinkageError e) {
            horizon.javalayer.Native.log(1, "[JavaLayer] the hollow server's levels and storage could not be made: " + e);
        }
        horizon.javalayer.Native.log(0, String.format("[JavaLayer] the Java server's classes are ready for plugins that use them (NMS): %.1f s", (System.nanoTime() - t) / 1e9));
    }

    /**
     * Paper's settings objects with every section made (Paper's config loader normally makes them from the YAML):
     * each field whose type is a ConfigurationPart is created with its defaults, recursively.
     */
    static <T> T defaults(T part) {
        for (Class<?> c = part.getClass(); c != null && c != Object.class; c = c.getSuperclass()) {
            for (java.lang.reflect.Field f : c.getDeclaredFields()) {
                if (java.lang.reflect.Modifier.isStatic(f.getModifiers())) continue;
                Class<?> t = f.getType();
                if (!io.papermc.paper.configuration.ConfigurationPart.class.isAssignableFrom(t)) continue;
                try {
                    f.setAccessible(true);
                    if (f.get(part) != null) {
                        defaults(f.get(part));
                        continue;
                    }
                    Object section;
                    if (t.getEnclosingClass() != null && !java.lang.reflect.Modifier.isStatic(t.getModifiers())) {
                        java.lang.reflect.Constructor<?> k = t.getDeclaredConstructor(t.getEnclosingClass());
                        k.setAccessible(true);
                        section = k.newInstance(part);
                    } else {
                        java.lang.reflect.Constructor<?> k = t.getDeclaredConstructor();
                        k.setAccessible(true);
                        section = k.newInstance();
                    }
                    f.set(part, defaults(section));
                } catch (ReflectiveOperationException | RuntimeException e) {
                    // left empty: this section's readers fail as before
                }
            }
        }
        return part;
    }

    /** What plugins read from the server beyond its registries: settings, world data, CraftRegistry's access. */
    private static void fill(DedicatedServer s) {
        try {
            org.bukkit.craftbukkit.CraftRegistry.setMinecraftRegistry(access);
        } catch (RuntimeException | LinkageError e) {
            horizon.javalayer.Native.log(1, "[JavaLayer] CraftRegistry: " + e);
        }
        String levelName = "world";
        try {
            java.io.File dir = new java.io.File(System.getProperty("user.dir"));
            java.nio.file.Path file = dir.toPath().resolve("server.properties");
            var props = net.minecraft.server.dedicated.DedicatedServerProperties.fromFile(file, new joptsimple.OptionParser().parse());
            var settings = Craft.allocate(net.minecraft.server.dedicated.DedicatedServerSettings.class);
            Craft.set(settings, net.minecraft.server.dedicated.DedicatedServerSettings.class, "source", file);
            Craft.set(settings, net.minecraft.server.dedicated.DedicatedServerSettings.class, "properties", props);
            s.settings = settings;
            levelName = props.levelName;
        } catch (RuntimeException | LinkageError e) {
            horizon.javalayer.Native.log(1, "[JavaLayer] server.properties for vanilla: " + e);
        }
        try {
            var level = new net.minecraft.world.level.LevelSettings(levelName, net.minecraft.world.level.GameType.SURVIVAL,
                    net.minecraft.world.level.LevelSettings.DifficultySettings.DEFAULT, false, net.minecraft.world.level.WorldDataConfiguration.DEFAULT);
            var data = new net.minecraft.world.level.storage.PrimaryLevelData(level, net.minecraft.world.level.storage.PrimaryLevelData.SpecialWorldProperty.NONE, com.mojang.serialization.Lifecycle.stable());
            Craft.set(s, MinecraftServer.class, "worldData", data);
        } catch (RuntimeException | LinkageError e) {
            horizon.javalayer.Native.log(1, "[JavaLayer] world data for vanilla: " + e);
        }
    }

    public static DedicatedServer server() {
        boot();
        return server;
    }

    public static RegistryAccess.Frozen registryAccess() {
        boot();
        return access;
    }

    /** A player's ServerPlayer: made once, its position brought up to date each time it is asked for. */
    public static ServerPlayer player(CraftPlayer p) {
        boot();
        UUID id = p.getUniqueId();
        ServerPlayer sp = PLAYERS.computeIfAbsent(id, k -> make(p));
        sync(sp, p);
        return sp;
    }

    public static void forget(UUID id) {
        PLAYERS.remove(id);
    }

    /** A player left: their handle goes, and its channel closes. */
    public static void left(UUID id) {
        ServerPlayer sp = PLAYERS.remove(id);
        PacketBridge.forget(id);
        if (sp != null && sp.connection != null && sp.connection.connection != null && sp.connection.connection.channel != null) {
            sp.connection.connection.channel.close();
        }
    }

    private static ServerPlayer make(CraftPlayer p) {
        ServerPlayer sp = Craft.allocate(Rider.class);
        Craft.set(sp, Entity.class, "id", p.getEntityId());
        Craft.set(sp, Entity.class, "uuid", p.getUniqueId());
        Craft.set(sp, Entity.class, "stringUUID", p.getUniqueId().toString());
        Craft.set(sp, Entity.class, "type", net.minecraft.world.entity.EntityTypes.PLAYER);
        Craft.set(sp, Entity.class, "bukkitEntity", p);
        Craft.set(sp, Entity.class, "random", net.minecraft.util.RandomSource.create());
        Craft.set(sp, net.minecraft.world.entity.player.Player.class, "gameProfile", new com.mojang.authlib.GameProfile(p.getUniqueId(), p.getName()));
        Craft.set(sp, ServerPlayer.class, "server", server);
        // Its synced data as vanilla's constructors make it (GSit's lay/crawl poses copy the player's: skin parts...).
        try {
            Craft.set(sp, Entity.class, "entityData", entityData(sp));
            // Its interpolation, as Entity's constructor makes it (vanilla's teleport through the player's connection
            // reads it: an NMS player a plugin seats, GSit's legs-up pose).
            try {
                Craft.set(sp, Entity.class, "interpolationHandler", net.minecraft.world.entity.InterpolationHandler.NO_OP);
            } catch (RuntimeException | LinkageError e) {
                horizon.javalayer.Native.log(1, "[JavaLayer] the hollow player's interpolation could not be made: " + e);
            }
            // Its effects: the player's potion effects as they are when read (GSit checks for invisibility).
            Craft.set(sp, net.minecraft.world.entity.LivingEntity.class, "activeEffects", new LiveEffects(p));
            // Its equipment: what the player wears and holds as it is when read (GSit's pose bodies show it).
            Craft.set(sp, net.minecraft.world.entity.LivingEntity.class, "equipment", new LiveEquipment(p));
            // Its attributes, a player's defaults (an NMS copy of the player sends them: GSit's pose bodies).
            Craft.set(sp, net.minecraft.world.entity.LivingEntity.class, "attributes", new net.minecraft.world.entity.ai.attributes.AttributeMap(
                    net.minecraft.world.entity.ai.attributes.DefaultAttributes.getSupplier(net.minecraft.world.entity.EntityTypes.PLAYER)));
            // Their client's settings as the game keeps them from the login (ServerPlayer.updateOptionsNoEvents): the
            // language, view distance, skin parts and main hand an NMS copy of the player takes (GSit's pose bodies).
            int raw = 127;
            try {
                com.destroystokyo.paper.SkinParts parts = ((org.bukkit.entity.Player) p).getClientOption(com.destroystokyo.paper.ClientOption.SKIN_PARTS);
                // PlayerModelPart's bits: cape, jacket, sleeves, pants legs, hat.
                raw = (parts.hasCapeEnabled() ? 1 : 0) | (parts.hasJacketEnabled() ? 2 : 0) | (parts.hasLeftSleeveEnabled() ? 4 : 0)
                        | (parts.hasRightSleeveEnabled() ? 8 : 0) | (parts.hasLeftPantsEnabled() ? 16 : 0) | (parts.hasRightPantsEnabled() ? 32 : 0)
                        | (parts.hasHatsEnabled() ? 64 : 0);
            } catch (RuntimeException ignored) {}
            String locale = "en_us";
            try {
                String l = ((org.bukkit.entity.Player) p).getLocale();
                if (l != null && !l.isEmpty()) locale = l;
            } catch (RuntimeException ignored) {}
            boolean left = false;
            try {
                left = ((org.bukkit.entity.Player) p).getMainHand() == org.bukkit.inventory.MainHand.LEFT;
            } catch (RuntimeException ignored) {}
            sp.updateOptionsNoEvents(new net.minecraft.server.level.ClientInformation(locale, org.bukkit.Bukkit.getViewDistance(),
                    net.minecraft.world.entity.player.ChatVisiblity.FULL, true, raw,
                    left ? net.minecraft.world.entity.HumanoidArm.LEFT : net.minecraft.world.entity.HumanoidArm.RIGHT, false, true,
                    net.minecraft.server.level.ParticleStatus.ALL));
        } catch (ReflectiveOperationException | RuntimeException | LinkageError e) {
            horizon.javalayer.Native.log(1, "[JavaLayer] the hollow player's synced data could not be made: " + e);
        }
        // The rest of what the constructors make, where a default is plain (locks, empty lists, zero vectors): vanilla
        // code run on the hollow player (a plugin seating or teleporting it through its connection) reads them.
        fillDefaults(sp);
        ServerGamePacketListenerImpl listener = Craft.allocate(ServerGamePacketListenerImpl.class);
        Wire wire = Craft.allocate(Wire.class);
        wire.player = p.getUniqueId();
        // The player's Netty channel as packet libraries look it up (PacketEvents, ProtocolLib: handle -> connection ->
        // channel): an embedded one, which they treat as a player without a socket of its own (as Paper's NPC players)
        // instead of kicking the player for a channel they could not find.
        wire.channel = PacketBridge.channel(p.getUniqueId());
        Craft.set(listener, ServerCommonPacketListenerImpl.class, "connection", wire);
        Craft.set(listener, ServerCommonPacketListenerImpl.class, "server", server);
        // Its CraftServer, which vanilla's teleport fires PlayerTeleportEvent through.
        try {
            Craft.set(listener, ServerCommonPacketListenerImpl.class, "cserver", horizon.javalayer.Bridge.server());
        } catch (RuntimeException | LinkageError ignored) {}
        Craft.set(listener, ServerGamePacketListenerImpl.class, "player", sp);
        sp.connection = listener;
        return sp;
    }

    /** Null fields of an allocated entity given the plain value vanilla's constructors start them with, by type. */
    static void fillDefaults(Entity entity) {
        for (Class<?> c = entity.getClass(); c != null && c != Object.class; c = c.getSuperclass()) {
            for (java.lang.reflect.Field f : c.getDeclaredFields()) {
                if (java.lang.reflect.Modifier.isStatic(f.getModifiers()) || f.getType().isPrimitive()) continue;
                try {
                    f.setAccessible(true);
                    if (f.get(entity) != null) continue;
                    Class<?> t = f.getType();
                    Object value = null;
                    if (t == Object.class && f.getName().toLowerCase(java.util.Locale.ROOT).contains("lock")) value = new Object();
                    else if (t == Vec3.class) value = Vec3.ZERO;
                    else if (t == net.minecraft.world.phys.AABB.class) value = new net.minecraft.world.phys.AABB(0, 0, 0, 0, 0, 0);
                    else if (t == com.google.common.collect.ImmutableList.class) value = com.google.common.collect.ImmutableList.of();
                    else if (t == java.util.List.class) value = new java.util.ArrayList<>();
                    else if (t == java.util.Set.class) value = new java.util.HashSet<>();
                    else if (t == java.util.Map.class) value = new java.util.HashMap<>();
                    else if (t == java.util.Optional.class) value = java.util.Optional.empty();
                    else if (t == net.kyori.adventure.util.TriState.class) value = net.kyori.adventure.util.TriState.NOT_SET;
                    else if (t == net.minecraft.core.BlockPos.class) value = net.minecraft.core.BlockPos.ZERO;
                    else if (t == net.minecraft.world.item.ItemStack.class) value = net.minecraft.world.item.ItemStack.EMPTY;
                    else if (t == net.minecraft.world.entity.player.Abilities.class) value = new net.minecraft.world.entity.player.Abilities();
                    else if (t == net.minecraft.world.food.FoodData.class) value = new net.minecraft.world.food.FoodData();
                    else if (t == net.minecraft.world.entity.WalkAnimationState.class) value = new net.minecraft.world.entity.WalkAnimationState();
                    else if (t == net.minecraft.world.level.ChunkPos.class) value = new net.minecraft.world.level.ChunkPos(0, 0);
                    else if (t == net.minecraft.world.entity.EntityDimensions.class) value = entity.getType().getDimensions();
                    else if (t == net.minecraft.world.level.entity.EntityInLevelCallback.class) value = net.minecraft.world.level.entity.EntityInLevelCallback.NULL;
                    else if ((t.getName().startsWith("java.util.") || t.getName().startsWith("it.unimi.dsi.fastutil."))
                            && !t.isInterface() && !java.lang.reflect.Modifier.isAbstract(t.getModifiers())) {
                        // A concrete collection: an empty one, as its initializer makes it.
                        try {
                            value = t.getDeclaredConstructor().newInstance();
                        } catch (ReflectiveOperationException ignored) {}
                    }
                    if (value != null) Craft.set(entity, c, f.getName(), value);
                } catch (ReflectiveOperationException | RuntimeException | LinkageError ignored) {
                    // left as it is
                }
            }
        }
    }

    /** An allocated entity's SynchedEntityData: Entity's own fields (its constructor's), then defineSynchedData's. */
    static net.minecraft.network.syncher.SynchedEntityData entityData(Entity entity) throws ReflectiveOperationException {
        var builder = new net.minecraft.network.syncher.SynchedEntityData.Builder(entity);
        Object[][] own = {
            {"DATA_SHARED_FLAGS_ID", (byte) 0}, {"DATA_AIR_SUPPLY_ID", 300}, {"DATA_CUSTOM_NAME_VISIBLE", false},
            {"DATA_CUSTOM_NAME", java.util.Optional.empty()}, {"DATA_SILENT", false}, {"DATA_NO_GRAVITY", false},
            {"DATA_POSE", net.minecraft.world.entity.Pose.STANDING}, {"DATA_TICKS_FROZEN", 0},
        };
        for (Object[] row : own) {
            var f = Entity.class.getDeclaredField((String) row[0]);
            f.setAccessible(true);
            @SuppressWarnings("unchecked") var key = (net.minecraft.network.syncher.EntityDataAccessor<Object>) f.get(null);
            builder.define(key, row[1]);
        }
        var define = Entity.class.getDeclaredMethod("defineSynchedData", net.minecraft.network.syncher.SynchedEntityData.Builder.class);
        define.setAccessible(true);
        define.invoke(entity, builder);
        return builder.build();
    }

    /** A hollow player's active effects: the player's potion effects, read each time the map is walked. */
    static final class LiveEffects extends java.util.AbstractMap<net.minecraft.core.Holder<net.minecraft.world.effect.MobEffect>, net.minecraft.world.effect.MobEffectInstance> {
        private final org.bukkit.entity.Player player;

        LiveEffects(org.bukkit.entity.Player player) { this.player = player; }

        @Override
        public java.util.Set<Entry<net.minecraft.core.Holder<net.minecraft.world.effect.MobEffect>, net.minecraft.world.effect.MobEffectInstance>> entrySet() {
            java.util.Map<net.minecraft.core.Holder<net.minecraft.world.effect.MobEffect>, net.minecraft.world.effect.MobEffectInstance> now = new java.util.LinkedHashMap<>();
            try {
                for (org.bukkit.potion.PotionEffect e : player.getActivePotionEffects()) {
                    var id = net.minecraft.resources.Identifier.parse(e.getType().getKey().toString());
                    net.minecraft.core.registries.BuiltInRegistries.MOB_EFFECT.getOptional(id).ifPresent(effect -> {
                        var holder = net.minecraft.core.registries.BuiltInRegistries.MOB_EFFECT.wrapAsHolder(effect);
                        now.put(holder, new net.minecraft.world.effect.MobEffectInstance(holder, e.getDuration(), e.getAmplifier(), e.isAmbient(), e.hasParticles(), e.hasIcon()));
                    });
                }
            } catch (RuntimeException ignored) {}
            return now.entrySet();
        }
    }

    /** A hollow player's equipment: the player's, read on each get (a set goes to the player too). */
    static final class LiveEquipment extends net.minecraft.world.entity.EntityEquipment {
        private final org.bukkit.entity.Player player;

        LiveEquipment(org.bukkit.entity.Player player) { this.player = player; }

        private static org.bukkit.inventory.EquipmentSlot bukkit(net.minecraft.world.entity.EquipmentSlot slot) {
            return switch (slot) {
                case MAINHAND -> org.bukkit.inventory.EquipmentSlot.HAND;
                case OFFHAND -> org.bukkit.inventory.EquipmentSlot.OFF_HAND;
                case FEET -> org.bukkit.inventory.EquipmentSlot.FEET;
                case LEGS -> org.bukkit.inventory.EquipmentSlot.LEGS;
                case CHEST -> org.bukkit.inventory.EquipmentSlot.CHEST;
                case HEAD -> org.bukkit.inventory.EquipmentSlot.HEAD;
                default -> null;
            };
        }

        @Override
        public net.minecraft.world.item.ItemStack get(net.minecraft.world.entity.EquipmentSlot slot) {
            org.bukkit.inventory.EquipmentSlot at = bukkit(slot);
            if (at == null) return net.minecraft.world.item.ItemStack.EMPTY;
            try {
                org.bukkit.inventory.ItemStack item = player.getInventory().getItem(at);
                return item == null ? net.minecraft.world.item.ItemStack.EMPTY : org.bukkit.craftbukkit.inventory.CraftItemStack.asNMSCopy(item);
            } catch (RuntimeException e) {
                return net.minecraft.world.item.ItemStack.EMPTY;
            }
        }

        @Override
        public net.minecraft.world.item.ItemStack set(net.minecraft.world.entity.EquipmentSlot slot, net.minecraft.world.item.ItemStack stack) {
            net.minecraft.world.item.ItemStack old = get(slot);
            org.bukkit.inventory.EquipmentSlot at = bukkit(slot);
            if (at != null) player.getInventory().setItem(at, org.bukkit.craftbukkit.inventory.CraftItemStack.asBukkitCopy(stack));
            return old;
        }

        @Override
        public boolean has(net.minecraft.world.entity.EquipmentSlot slot) { return !get(slot).isEmpty(); }

        @Override
        public boolean isEmpty() {
            for (net.minecraft.world.entity.EquipmentSlot slot : net.minecraft.world.entity.EquipmentSlot.values()) if (has(slot)) return false;
            return true;
        }
    }

    /** A hollow level's game events: none dispatched here (the Rust server has its own sculk sensors and vibrations). */
    public static final class NoGameEvents extends net.minecraft.world.level.gameevent.GameEventDispatcher {
        private NoGameEvents() { super(null); }
        @Override public void post(net.minecraft.core.Holder<net.minecraft.world.level.gameevent.GameEvent> event, Vec3 at, net.minecraft.world.level.gameevent.GameEvent.Context context) {}
    }

    private static void sync(ServerPlayer sp, CraftPlayer p) {
        try {
            org.bukkit.Location l = p.getLocation();
            Craft.set(sp, Entity.class, "position", new Vec3(l.getX(), l.getY(), l.getZ()));
            Craft.set(sp, Entity.class, "blockPosition", new net.minecraft.core.BlockPos(l.getBlockX(), l.getBlockY(), l.getBlockZ()));
            Craft.set(sp, Entity.class, "chunkPosition", new net.minecraft.world.level.ChunkPos(l.getBlockX() >> 4, l.getBlockZ() >> 4));
            Craft.set(sp, Entity.class, "yRot", l.getYaw());
            Craft.set(sp, Entity.class, "xRot", l.getPitch());
            if (l.getWorld() != null) Craft.set(sp, Entity.class, "level", level(l.getWorld()));
        } catch (RuntimeException ignored) {}
    }

    private static net.minecraft.server.dedicated.DedicatedPlayerList playerList;

    /**
     * CraftServer.getHandle(): the server's player list, hollow (no game behind it), its players the online ones'
     * hollow ServerPlayers, refreshed on each call (InvSeePlusPlus, MythicMobs read them).
     */
    public static synchronized net.minecraft.server.dedicated.DedicatedPlayerList playerList() {
        boot();
        if (playerList == null) {
            var list = Craft.allocate(net.minecraft.server.dedicated.DedicatedPlayerList.class);
            Craft.set(list, net.minecraft.server.players.PlayerList.class, "server", server);
            // The online players as they are each time the list is walked (getPlayers(): VexBot, journeymap per tick).
            Craft.set(list, net.minecraft.server.players.PlayerList.class, "players", new LevelPlayers(null));
            Craft.set(list, net.minecraft.server.players.PlayerList.class, "playersByUUID", new java.util.concurrent.ConcurrentHashMap<UUID, ServerPlayer>());
            Craft.set(list, net.minecraft.server.players.PlayerList.class, "playersByName", new java.util.concurrent.ConcurrentHashMap<String, ServerPlayer>());
            Craft.set(list, net.minecraft.server.players.PlayerList.class, "registries", Craft.get(server, MinecraftServer.class, "registries"));
            Craft.set(list, net.minecraft.server.players.PlayerList.class, "cserver", horizon.javalayer.Bridge.server());
            Craft.set(list, net.minecraft.server.players.PlayerList.class, "viewDistance", org.bukkit.Bukkit.getViewDistance());
            Craft.set(list, net.minecraft.server.players.PlayerList.class, "simulationDistance", org.bukkit.Bukkit.getSimulationDistance());
            // Where the game saves players (<level>/players/data, LevelResource.PLAYER_DATA_DIR): the main world's
            // folder is <level>/dimensions/minecraft/overworld.
            try {
                java.io.File overworld = org.bukkit.Bukkit.getWorlds().get(0).getWorldFolder();
                java.io.File level = overworld.getAbsoluteFile().getParentFile().getParentFile().getParentFile();
                java.io.File dir = new java.io.File(level, "players/data");
                dir.mkdirs();
                var io = Craft.allocate(net.minecraft.world.level.storage.PlayerDataStorage.class);
                Craft.set(io, net.minecraft.world.level.storage.PlayerDataStorage.class, "playerDir", dir);
                Craft.set(io, net.minecraft.world.level.storage.PlayerDataStorage.class, "fixerUpper", net.minecraft.util.datafix.DataFixers.getDataFixer());
                Craft.set(list, net.minecraft.server.players.PlayerList.class, "playerIo", io);
            } catch (RuntimeException | LinkageError e) {
                horizon.javalayer.Native.log(1, "[JavaLayer] the hollow player list has no player data folder: " + e);
            }
            playerList = list;
            try {
                Craft.set(server, MinecraftServer.class, "playerList", list);
            } catch (RuntimeException ignored) {
                // a server without the field
            }
        }
        @SuppressWarnings("unchecked") java.util.Map<UUID, ServerPlayer> byUuid = (java.util.Map<UUID, ServerPlayer>) Craft.get(playerList, net.minecraft.server.players.PlayerList.class, "playersByUUID");
        @SuppressWarnings("unchecked") java.util.Map<String, ServerPlayer> byName = (java.util.Map<String, ServerPlayer>) Craft.get(playerList, net.minecraft.server.players.PlayerList.class, "playersByName");
        java.util.List<ServerPlayer> now = new java.util.ArrayList<>();
        for (org.bukkit.entity.Player p : org.bukkit.Bukkit.getOnlinePlayers()) {
            if (p instanceof CraftPlayer cp) now.add(player(cp));
        }
        byUuid.clear();
        byName.clear();
        for (ServerPlayer sp : now) {
            byUuid.put(sp.getUUID(), sp);
            byName.put(sp.getGameProfile().name().toLowerCase(java.util.Locale.ROOT), sp);
        }
        return playerList;
    }

    /** A world's ServerLevel: no game behind it, but its registries, dimension and Bukkit world are there. */
    public static ServerLevel level(org.bukkit.World world) {
        boot();
        ServerLevel level = made(world);
        border(level, world);
        return level;
    }

    /** A level's world border brought to the world's (center and size), when they differ. */
    private static void border(ServerLevel level, org.bukkit.World world) {
        try {
            var storage = level.getChunkSource() == null ? null : level.getChunkSource().getDataStorage();
            var border = storage == null ? null : storage.get(net.minecraft.world.level.border.WorldBorder.TYPE);
            if (border == null) return;
            org.bukkit.WorldBorder live = world.getWorldBorder();
            org.bukkit.Location c = live.getCenter();
            if (border.getCenterX() != c.getX() || border.getCenterZ() != c.getZ()) border.setCenter(c.getX(), c.getZ());
            if (border.getSize() != live.getSize()) border.setSize(live.getSize());
        } catch (RuntimeException | LinkageError ignored) {
        }
    }

    private static ServerLevel made(org.bukkit.World world) {
        return LEVELS.computeIfAbsent(world.getName(), k -> {
            // Live: block reads and writes are the Rust server's (LiveChunks: WorldEdit's adapter, AntiXray).
            ServerLevel level = Craft.allocate(LiveChunks.Level.class);
            Craft.set(level, ServerLevel.class, "server", server);
            // Which world it is: its dimension key and type, and Paper's CraftWorld behind it (getWorld()).
            try {
                var dimension = net.minecraft.resources.ResourceKey.create(net.minecraft.core.registries.Registries.DIMENSION, net.minecraft.resources.Identifier.parse(world.getKey().toString()));
                Craft.set(level, net.minecraft.world.level.Level.class, "dimension", dimension);
                // The game's registries, as the level reaches them (registryAccess(): Pl3xMap's biomes).
                Craft.set(level, net.minecraft.world.level.Level.class, "registryAccess", access);
                var typeKey = switch (world.getEnvironment()) {
                    case NETHER -> net.minecraft.world.level.dimension.BuiltinDimensionTypes.NETHER;
                    case THE_END -> net.minecraft.world.level.dimension.BuiltinDimensionTypes.END;
                    default -> net.minecraft.world.level.dimension.BuiltinDimensionTypes.OVERWORLD;
                };
                Craft.set(level, net.minecraft.world.level.Level.class, "dimensionTypeRegistration", access.lookupOrThrow(net.minecraft.core.registries.Registries.DIMENSION_TYPE).getOrThrow(typeKey));
                // Its height, as Level's constructor keeps it from the dimension type (getSectionsCount, getMinY...).
                var type = access.lookupOrThrow(net.minecraft.core.registries.Registries.DIMENSION_TYPE).getOrThrow(typeKey).value();
                int minY = type.minY(), height = type.height(), maxY = minY + height - 1;
                Craft.set(level, net.minecraft.world.level.Level.class, "minY", minY);
                Craft.set(level, net.minecraft.world.level.Level.class, "height", height);
                Craft.set(level, net.minecraft.world.level.Level.class, "maxY", maxY);
                Craft.set(level, net.minecraft.world.level.Level.class, "minSectionY", minY >> 4);
                Craft.set(level, net.minecraft.world.level.Level.class, "maxSectionY", maxY >> 4);
                Craft.set(level, net.minecraft.world.level.Level.class, "sectionsCount", (maxY >> 4) - (minY >> 4) + 1);
                // Its palettes (palettedContainerFactory(): plugins make chunk sections with it, AntiXray).
                Craft.set(level, net.minecraft.world.level.Level.class, "palettedContainerFactory", LiveChunks.palettes());
                if (world instanceof org.bukkit.craftbukkit.CraftWorld craft) Craft.set(level, net.minecraft.world.level.Level.class, "world", craft);
                // Its entity lookup: an NMS entity a plugin adds goes to the server, one it discards may go (NmsEntities).
                level.moonrise$setEntityLookup(NmsEntities.Getter.of(level));
                // Its game events (an NMS entity a plugin adds or moves posts some: GSit's poses).
                Craft.set(level, ServerLevel.class, "gameEventDispatcher", Craft.allocate(NoGameEvents.class));
                // Its game rules (the game's defaults: the server keeps its own), which its waypoints read.
                try {
                    Craft.set(level, ServerLevel.class, "gameRules", new net.minecraft.world.level.gamerules.GameRules(net.minecraft.world.flag.FeatureFlags.DEFAULT_FLAGS));
                } catch (RuntimeException | LinkageError e) {
                    horizon.javalayer.Native.log(1, "[JavaLayer] the hollow level's game rules could not be made: " + e);
                }
                // Its waypoints (none tracked: the server's), which an NMS living entity leaves as it's discarded.
                try {
                    Craft.set(level, ServerLevel.class, "waypointManager", new net.minecraft.server.waypoints.ServerWaypointManager(level));
                } catch (RuntimeException | LinkageError e) {
                    horizon.javalayer.Native.log(1, "[JavaLayer] the hollow level's waypoints could not be made: " + e);
                }
                // Spigot's settings for the world (spigot.yml's world section): what a new NMS entity reads as it's made
                // (its activation range: GSit's seat).
                try {
                    var spigot = new org.spigotmc.SpigotWorldConfig(world.getName(), world.getKey());
                    Craft.set(level, net.minecraft.world.level.Level.class, "spigotConfig", spigot);
                    // Paper's settings for the world (paperConfig(): a new entity reads its entity settings).
                    if (paperConfigs != null) {
                        var context = io.papermc.paper.configuration.PaperConfigurations.createWorldContextMap(world.getWorldFolder().toPath(), net.minecraft.resources.Identifier.parse(world.getKey().toString()), spigot, access, new net.minecraft.world.level.gamerules.GameRules(net.minecraft.world.flag.FeatureFlags.DEFAULT_FLAGS));
                        Craft.set(level, net.minecraft.world.level.Level.class, "paperConfig", paperConfigs.createWorldConfig(context));
                    }
                } catch (Exception | LinkageError e) {
                    horizon.javalayer.Native.log(1, "[JavaLayer] the hollow level's Spigot and Paper settings could not be made: " + e);
                }
                // Its seed, as ServerLevel.getSeed() reads it (Pl3xMap).
                var options = new net.minecraft.world.level.levelgen.WorldOptions(world.getSeed(), world.canGenerateStructures(), false);
                Craft.set(level, ServerLevel.class, "worldGenSettings", new net.minecraft.world.level.levelgen.WorldGenSettings(options, null));
                // Its level data (getLevelData(): spawn, time, difficulty), read from the world (Pl3xMap).
                LiveLevelData data = new LiveLevelData(world, dimension);
                Craft.set(level, net.minecraft.world.level.Level.class, "levelData", data);
                // Paper's per-world overrides over the server's level data (serverLevelData: getLevelName(), which a new
                // NMS entity's tracking reads: GSit's seat), made from the world as it is now.
                try {
                    var difficulty = net.minecraft.world.Difficulty.byId(world.getDifficulty().ordinal());
                    var settings = new net.minecraft.world.level.LevelSettings(world.getName(), net.minecraft.world.level.GameType.SURVIVAL,
                            new net.minecraft.world.level.LevelSettings.DifficultySettings(difficulty, world.isHardcore(), false), true,
                            net.minecraft.world.level.WorldDataConfiguration.DEFAULT);
                    var root = new net.minecraft.world.level.storage.PrimaryLevelData(settings,
                            net.minecraft.world.level.storage.PrimaryLevelData.SpecialWorldProperty.NONE, com.mojang.serialization.Lifecycle.stable());
                    var overrides = io.papermc.paper.world.saveddata.PaperLevelOverrides.createFromLiveLevelData(root).attach(root, dimension);
                    Craft.set(level, ServerLevel.class, "serverLevelData", overrides);
                } catch (RuntimeException | LinkageError e) {
                    horizon.javalayer.Native.log(1, "[JavaLayer] the hollow level's Paper level data could not be made: " + e);
                }
                // Its players (players(): VexBot walks them each tick), the world's online ones as they are now.
                Craft.set(level, ServerLevel.class, "players", new LevelPlayers(world));
                // Its chunk source, hollow: the chunk map and light engine plugins reach every tick (AxiomPaper relights
                // what it queued; with nothing queued, the light engine's world is all it reads).
                try {
                    var starlight = Craft.allocate(ca.spottedleaf.moonrise.patches.starlight.light.StarLightInterface.class);
                    Craft.set(starlight, ca.spottedleaf.moonrise.patches.starlight.light.StarLightInterface.class, "world", level);
                    var light = Craft.allocate(LiveChunks.Light.class);
                    Craft.set(light, net.minecraft.world.level.lighting.LevelLightEngine.class, "lightEngine", starlight);
                    var chunkMap = Craft.allocate(net.minecraft.server.level.ChunkMap.class);
                    Craft.set(chunkMap, net.minecraft.server.level.ChunkMap.class, "level", level);
                    // Its tracked entities: none (the server tracks them), but there to ask, as a new NMS entity's id
                    // does (Entity.<init> -> ServerLevel.getNextEntityId: GSit's seat).
                    Craft.set(chunkMap, net.minecraft.server.level.ChunkMap.class, "entityMap", new it.unimi.dsi.fastutil.ints.Int2ObjectOpenHashMap<>());
                    var chunks = LiveChunks.Cache.make(level);
                    Craft.set(chunks, net.minecraft.server.level.ServerChunkCache.class, "level", level);
                    Craft.set(chunks, net.minecraft.server.level.ServerChunkCache.class, "lightEngine", light);
                    Craft.set(chunks, net.minecraft.server.level.ServerChunkCache.class, "chunkMap", chunkMap);
                    // The level's saved data (<world folder>/data, read, never written: the world's files are the Rust
                    // server's), its world border the world's own (getWorldBorder(): Pl3xMap's map bounds).
                    var levelData = new net.minecraft.world.level.storage.SavedDataStorage(world.getWorldFolder().toPath().resolve("data"), net.minecraft.util.datafix.DataFixers.getDataFixer(), access);
                    levelData.set(net.minecraft.world.level.border.WorldBorder.TYPE, new net.minecraft.world.level.border.WorldBorder());
                    Craft.set(chunks, net.minecraft.server.level.ServerChunkCache.class, "savedDataStorage", levelData);
                    Craft.set(level, ServerLevel.class, "chunkSource", chunks);
                } catch (RuntimeException | LinkageError e) {
                    horizon.javalayer.Native.log(1, "[JavaLayer] a hollow level's chunk source could not be made: " + e);
                }
                // The server's respawn point (ServerLevel.getRespawnData() asks the server): the main world's spawn.
                if (!org.bukkit.Bukkit.getWorlds().isEmpty() && org.bukkit.Bukkit.getWorlds().get(0).getName().equals(world.getName())) {
                    Craft.set(server, MinecraftServer.class, "effectiveRespawnData", data.getRespawnData());
                }
                // The server's levels (getAllLevels, levelKeys: VexBot, AxiomPaper).
                @SuppressWarnings("unchecked") java.util.Map<Object, ServerLevel> levels = (java.util.Map<Object, ServerLevel>) Craft.get(server, MinecraftServer.class, "levels");
                if (levels != null) levels.put(dimension, level);
            } catch (RuntimeException | LinkageError ignored) {
                // a level without them, as before
            }
            return level;
        });
    }

    /** Packets to a player through the Rust server: vanilla's encoding, the bytes framed and sent as they are. */
    public static void send(UUID player, Packet<?> packet) {
        boot();
        StringBuilder hex = new StringBuilder();
        if (packet instanceof BundlePacket<?> && clientbound.bundlerInfo() != null) {
            // As vanilla's PacketBundleUnpacker: the protocol's own delimiter (its codec takes only that instance),
            // the packets, the delimiter again.
            clientbound.bundlerInfo().unbundlePacket(packet, p -> frame(hex, p));
        } else if (packet instanceof BundlePacket<?> bundle) {
            frame(hex, ClientboundBundleDelimiterPacket.class.cast(delimiter()));
            for (Packet<?> p : bundle.subPackets()) frame(hex, p);
            frame(hex, ClientboundBundleDelimiterPacket.class.cast(delimiter()));
        } else {
            frame(hex, packet);
        }
        // The server names players by 32 hex digits, no dashes (a dashed UUID matched nobody: every packet was dropped).
        horizon.javalayer.Native.query("{\"op\":\"packets_send\",\"packets\":[{\"to\":[\"" + player.toString().replace("-", "") + "\"],\"raw\":\"" + hex + "\"}]}");
    }

    private static Object delimiter;

    private static Object delimiter() {
        if (delimiter == null) delimiter = Craft.allocate(ClientboundBundleDelimiterPacket.class);
        return delimiter;
    }

    @SuppressWarnings("unchecked")
    private static void frame(StringBuilder hex, Packet<?> packet) {
        ByteBuf body = Unpooled.buffer();
        try {
            ((StreamCodec<ByteBuf, Packet<? super ClientGamePacketListener>>) (Object) clientbound.codec()).encode(body, (Packet<? super ClientGamePacketListener>) packet);
            int length = body.readableBytes();
            int v = length;
            while ((v & ~0x7F) != 0) {
                hexByte(hex, (v & 0x7F) | 0x80);
                v >>>= 7;
            }
            hexByte(hex, v);
            for (int i = 0; i < length; i++) hexByte(hex, body.getByte(i));
        } finally {
            body.release();
        }
    }

    private static final char[] DIGITS = "0123456789abcdef".toCharArray();

    private static void hexByte(StringBuilder hex, int b) {
        hex.append(DIGITS[(b >>> 4) & 0xF]).append(DIGITS[b & 0xF]);
    }

    /** A packet to a player from its body (id and fields), as vanilla's PacketDecoder reads it; null if it can't. */
    @SuppressWarnings("unchecked")
    public static Packet<?> decodeBody(ByteBuf body) {
        boot();
        try {
            return ((StreamCodec<ByteBuf, Packet<? super ClientGamePacketListener>>) (Object) clientbound.codec()).decode(body);
        } catch (RuntimeException e) {
            return null;
        }
    }

    /** A packet to a player as its body (id and fields), as vanilla's PacketEncoder writes it. */
    @SuppressWarnings("unchecked")
    public static void encodeBody(Packet<?> packet, ByteBuf out) {
        boot();
        ((StreamCodec<ByteBuf, Packet<? super ClientGamePacketListener>>) (Object) clientbound.codec()).encode(out, (Packet<? super ClientGamePacketListener>) packet);
    }

    /** A packet as if the player had sent it now: to the game, past the packet listeners. */
    static void receive(UUID player, Packet<?> packet) {
        ByteBuf body = Unpooled.buffer();
        try {
            encodeServerbound(packet, body);
            StringBuilder hex = new StringBuilder();
            int length = body.readableBytes();
            int v = length;
            while ((v & ~0x7F) != 0) {
                hexByte(hex, (v & 0x7F) | 0x80);
                v >>>= 7;
            }
            hexByte(hex, v);
            for (int i = 0; i < length; i++) hexByte(hex, body.getByte(body.readerIndex() + i));
            horizon.javalayer.Native.query("{\"op\":\"packets_receive\",\"packets\":[{\"from\":\"" + player.toString().replace("-", "") + "\",\"raw\":\"" + hex + "\"}]}");
        } catch (RuntimeException e) {
            horizon.javalayer.Native.log(1, "[JavaLayer] a packet for " + player + " to the game could not be written: " + e);
        } finally {
            body.release();
        }
    }

    /** A player's packet as its body (id and fields), as the client's encoder writes it. */
    @SuppressWarnings("unchecked")
    static void encodeServerbound(Packet<?> packet, ByteBuf out) {
        boot();
        ((StreamCodec<ByteBuf, Packet<? super net.minecraft.network.protocol.game.ServerGamePacketListener>>) (Object) serverbound.codec()).encode(out, (Packet<? super net.minecraft.network.protocol.game.ServerGamePacketListener>) packet);
    }

    /** A packet's body (id and fields, as a Netty pipeline has it before the length prefix) to a player, framed. */
    static void sendBody(UUID player, ByteBuf body) {
        StringBuilder hex = new StringBuilder();
        int length = body.readableBytes();
        int v = length;
        while ((v & ~0x7F) != 0) {
            hexByte(hex, (v & 0x7F) | 0x80);
            v >>>= 7;
        }
        hexByte(hex, v);
        int at = body.readerIndex();
        for (int i = 0; i < length; i++) hexByte(hex, body.getByte(at + i));
        horizon.javalayer.Native.query("{\"op\":\"packets_send\",\"packets\":[{\"to\":[\"" + player.toString().replace("-", "") + "\"],\"raw\":\"" + hex + "\"}]}");
    }

    /** Saves what plugins put in the server's saved data (Bridge.stop, after the plugins are disabled). */
    public static void save() {
        if (server == null) return;
        try {
            var storage = server.getDataStorage();
            if (storage != null) storage.saveAndJoin();
        } catch (RuntimeException | LinkageError e) {
            horizon.javalayer.Native.log(1, "[JavaLayer] the server's saved data could not be saved: " + e);
        }
    }

    /** The game's BlockState for a block data's text (`minecraft:oak_stairs[facing=north,...]`). */
    public static net.minecraft.world.level.block.state.BlockState blockState(String text) {
        boot();
        try {
            return net.minecraft.commands.arguments.blocks.BlockStateParser.parseForBlock(access.lookupOrThrow(net.minecraft.core.registries.Registries.BLOCK), text, false).blockState();
        } catch (com.mojang.brigadier.exceptions.CommandSyntaxException e) {
            throw new IllegalArgumentException("not a block state: " + text, e);
        }
    }

    /** A players list read each time it is walked: a world's online players, or the server's (no world). */
    static final class LevelPlayers extends java.util.AbstractList<ServerPlayer> {
        private final org.bukkit.World world;

        LevelPlayers(org.bukkit.World world) { this.world = world; }

        private java.util.Collection<? extends org.bukkit.entity.Player> online() {
            return world == null ? org.bukkit.Bukkit.getOnlinePlayers() : world.getPlayers();
        }

        private java.util.List<ServerPlayer> now() {
            java.util.List<ServerPlayer> out = new java.util.ArrayList<>();
            for (org.bukkit.entity.Player p : online()) if (p instanceof CraftPlayer cp) out.add(player(cp));
            return out;
        }

        @Override public ServerPlayer get(int index) { return now().get(index); }
        @Override public int size() { return online().size(); }
        @Override public java.util.Iterator<ServerPlayer> iterator() { return now().iterator(); }
    }

    private static java.lang.reflect.Field connectionChannel;

    /**
     * The channel the player's Connection holds now, if a plugin replaced it (ProtocolLib puts its NettyChannelProxy
     * there and hears the game's packets as Paper's Connection writes them through it), else null.
     */
    static io.netty.channel.Channel replacedChannel(UUID id, io.netty.channel.Channel own) {
        ServerPlayer sp = PLAYERS.get(id);
        if (sp == null || sp.connection == null) return null;
        try {
            Object wire = Craft.get(sp.connection, ServerCommonPacketListenerImpl.class, "connection");
            if (wire == null) return null;
            java.lang.reflect.Field f = connectionChannel;
            if (f == null) {
                f = Connection.class.getDeclaredField("channel");
                f.setAccessible(true);
                connectionChannel = f;
            }
            Object now = f.get(wire);
            return now instanceof io.netty.channel.Channel c && c != own ? c : null;
        } catch (ReflectiveOperationException | RuntimeException e) {
            return null;
        }
    }

    /**
     * A hollow player that rides as the server's player does: a plugin's NMS startRiding (GSit seats the player on an
     * NMS armor stand it adds itself) mounts them in the Rust server once the vehicle is there (NmsEntities).
     */
    public static final class Rider extends ServerPlayer {
        private Rider() { super(null, null, null, null); }

        @Override public boolean startRiding(net.minecraft.world.entity.Entity vehicle, boolean force, boolean sendEventAndTriggers) { return NmsEntities.ride(this, vehicle); }
        @Override public void stopRiding() { NmsEntities.dismount(this); }
        @Override public net.minecraft.world.entity.Entity getVehicle() { return NmsEntities.vehicleOf(this); }
        @Override public boolean isPassenger() { return NmsEntities.vehicleOf(this) != null; }
    }

    /** A player's vanilla Connection with no channel: what is sent goes to the Rust server for them. */
    public static final class Wire extends Connection {
        UUID player;

        private Wire() { super(net.minecraft.network.protocol.PacketFlow.CLIENTBOUND); }

        @Override public void send(Packet<?> packet) { sendNow(packet); }
        @Override public void send(Packet<?> packet, io.netty.channel.ChannelFutureListener listener) { sendNow(packet); }
        @Override public void send(Packet<?> packet, io.netty.channel.ChannelFutureListener listener, boolean flush) { sendNow(packet); }

        /**
         * A player's packet handed to the game later (ProtocolLib lets one its async listeners held go on, or a
         * plugin makes one up): to the Rust server, which handles it as if the player had just sent it.
         */
        @Override
        protected void channelRead0(io.netty.channel.ChannelHandlerContext ctx, Packet<?> packet) {
            Hollow.receive(player, packet);
        }

        /** Through the player's channel when plugins have handlers in it (ProtocolLib hears it), else straight out. */
        private void sendNow(Packet<?> packet) {
            if (!PacketBridge.sendFromConnection(player, packet)) Hollow.send(player, packet);
        }
        @Override public boolean isConnected() { return true; }
    }
}
