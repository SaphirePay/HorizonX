package horizon.javalayer;

import org.bukkit.command.ConsoleCommandSender;
import org.bukkit.entity.EntityType;
import org.bukkit.inventory.ItemStack;
import org.bukkit.inventory.meta.ItemMeta;

import java.lang.ref.Cleaner;
import java.lang.ref.WeakReference;
import java.lang.reflect.InvocationHandler;
import java.lang.reflect.Proxy;
import java.util.*;
import java.util.concurrent.ConcurrentHashMap;

/** Bukkit interfaces (`Player`, `World`, `Block`...) answered by the server: a proxy per game object. */
public final class Proxies {
    private Proxies() {}

    public static final Cleaner CLEANER = Cleaner.create();
    /** The same player, world or entity is the same Java object, as on Paper (`==`, map keys). */
    private static final Map<String, WeakReference<Object>> BY_KEY = new ConcurrentHashMap<>();
    /** One CraftPlayer per online player, as on Paper: a plugin's Permissible (LuckPerms') put in one is in all. */
    private static final Map<String, Object> PLAYERS = new ConcurrentHashMap<>();
    /** Loaded worlds, held for good (CraftWorld lives while its world does): plugins keep weak references to a World
     * (WorldEdit's BukkitWorld) and must find the same object there. */
    private static final Map<String, Object> WORLDS = new ConcurrentHashMap<>();

    static void playerLeft(String key) {
        PLAYERS.remove(key);
        BY_KEY.remove(key);
    }
    private static final Map<String, Class<?>> KINDS = new ConcurrentHashMap<>();
    private static final Class<?> NONE = Void.class;

    static Class<?> kindClass(String kind) {
        if (kind == null || kind.isEmpty()) return null;
        Class<?> found = KINDS.computeIfAbsent(kind, k -> {
            try {
                return Class.forName(k, false, Proxies.class.getClassLoader());
            } catch (ClassNotFoundException | LinkageError e) {
                return NONE;
            }
        });
        return found == NONE ? null : found;
    }

    /** The server's handle behind a Java object, if it has one. */
    public static H handleOf(Object o) {
        if (o == null) return null;
        if (o instanceof Handled h) return h.horizonHandle();
        // An item meta (Paper's own class names, Gen.metas): its handle is kept off the interface list.
        if (o instanceof org.bukkit.craftbukkit.inventory.CraftMetaItem m) return m.horizonHandle();
        if (Proxy.isProxyClass(o.getClass())) {
            InvocationHandler handler = Proxy.getInvocationHandler(o);
            if (handler instanceof NativeHandler n) return n.h;
        }
        return null;
    }

    public static final class Release implements Runnable {
        private final long id;
        public Release(long id) { this.id = id; }
        @Override public void run() {
            try { Native.release(id); } catch (Throwable ignored) {}
        }
    }

    static void add(Set<Class<?>> set, Class<?> c) {
        if (c == null || !c.isInterface()) return;
        for (Class<?> have : set) if (c.isAssignableFrom(have)) return;
        set.removeIf(have -> have.isAssignableFrom(c));
        set.add(c);
    }

    /** Inventory types known from where an inventory was made (createInventory's type, a block's), by handle key. */
    static final Map<String, org.bukkit.event.inventory.InventoryType> INVENTORY_TYPES = new java.util.concurrent.ConcurrentHashMap<>();

    static String invKey(H h) {
        return h.key.isEmpty() ? "#" + h.id : h.key;
    }

    /** An inventory of a known type: its proxy with that type's interface (FurnaceInventory...). */
    static Object inventory(H h, org.bukkit.event.inventory.InventoryType type) {
        INVENTORY_TYPES.put(invKey(h), type);
        Set<Class<?>> faces = new LinkedHashSet<>();
        add(faces, kindClass(h.kind));
        add(faces, org.bukkit.inventory.Inventory.class);
        for (Class<?> f : BlockStates.inventoryFaces(type)) add(faces, f);
        // A plain inventory (no kind of its own) is Paper's CraftInventoryCustom (HorizonInventory, Gen.inventory):
        // plugins reach into its `inventory` field (Duels).
        if (faces.contains(org.bukkit.inventory.Inventory.class) && faces.stream().noneMatch(f -> f != org.bukkit.inventory.Inventory.class && org.bukkit.inventory.Inventory.class.isAssignableFrom(f))) {
            return org.bukkit.craftbukkit.inventory.HorizonInventory.make(h);
        }
        return java.lang.reflect.Proxy.newProxyInstance(Proxies.class.getClassLoader(), faces.toArray(new Class<?>[0]), new NativeHandler(h));
    }

    /** HorizonInventory's `inventory`: Paper's MinecraftInventory, empty, untitled (a plugin may set its title). */
    public static void fillCraftInventory(org.bukkit.craftbukkit.inventory.CraftInventory self) {
        try {
            Class<?> mc = Class.forName("org.bukkit.craftbukkit.inventory.CraftInventoryCustom$MinecraftInventory");
            Object inv = Craft.allocate(mc);
            Craft.set(inv, mc, "title", "");
            Craft.set(inv, mc, "adventure$title", net.kyori.adventure.text.Component.empty());
            Craft.set(inv, mc, "viewers", new ArrayList<>());
            Craft.set(inv, mc, "maxStack", 99);
            Craft.set(inv, mc, "type", org.bukkit.event.inventory.InventoryType.CHEST);
            Craft.set(self, org.bukkit.craftbukkit.inventory.CraftInventory.class, "inventory", inv);
        } catch (ReflectiveOperationException | RuntimeException | LinkageError ignored) {
            // the field stays empty
        }
    }

    /**
     * The class loader a proxy of these interfaces is made in: the layer's, unless one comes from a loader below it
     * (the built-in ProtocolLib's, lib/builtin: its ProtocolManager isn't visible from the layer's loader).
     */
    static ClassLoader loaderFor(List<Class<?>> faces) {
        ClassLoader layer = Proxies.class.getClassLoader();
        for (Class<?> c : faces) {
            ClassLoader own = c.getClassLoader();
            if (own == null || own == layer) continue;
            for (ClassLoader up = own.getParent(); up != null; up = up.getParent()) if (up == layer) return own;
        }
        return layer;
    }

    public static Object make(H h, Class<?> expected) {
        if (!h.key.isEmpty()) {
            Object world = WORLDS.get(h.key);
            if (world != null && (expected == Object.class || expected.isInstance(world))) {
                Native.release(h.id);
                return world;
            }
        }
        if (h.key.startsWith("p:")) {
            Object player = PLAYERS.get(h.key);
            if (player != null && (expected == Object.class || expected.isInstance(player))) {
                Native.release(h.id);
                return player;
            }
        }
        if (!h.key.isEmpty()) {
            WeakReference<Object> ref = BY_KEY.get(h.key);
            Object cached = ref == null ? null : ref.get();
            if (cached != null && (expected == Object.class || expected.isInstance(cached))) {
                Native.release(h.id);
                return cached;
            }
        }
        Set<Class<?>> faces = new LinkedHashSet<>();
        Class<?> kind = kindClass(h.kind);
        boolean stack = kind == ItemStack.class;
        if (!stack) add(faces, kind);
        if (h.kind.startsWith("org.bukkit.entity.") && !h.extra.isEmpty()) {
            try {
                EntityType type = EntityType.valueOf(h.extra);
                add(faces, type.getEntityClass());
            } catch (Throwable ignored) {}
        }
        if ("console".equals(h.extra)) add(faces, ConsoleCommandSender.class);
        // Block data: Paper's CraftBlockData.getState() (the game's state), found by reflection (NmsBlockState).
        if (kind != null && org.bukkit.block.data.BlockData.class.isAssignableFrom(kind)) add(faces, NmsBlockState.class);
        // An inventory: the interface of its kind (FurnaceInventory, BrewerInventory...), by its type.
        if (kind != null && org.bukkit.inventory.Inventory.class.isAssignableFrom(kind)) {
            Object type = Native.tryCall(h, "getType", new Object[0]);
            Object inv = type == Native.MISSING ? null : Conv.fromNative(type, org.bukkit.event.inventory.InventoryType.class);
            if (inv instanceof org.bukkit.event.inventory.InventoryType t) for (Class<?> face : BlockStates.inventoryFaces(t)) add(faces, face);
        }
        // A block entity's state: the interface of its block (Furnace, Chest, Sign...), by its type.
        if (kind != null && org.bukkit.block.BlockState.class.isAssignableFrom(kind)) {
            Object type = Native.tryCall(h, "getType", new Object[0]);
            Object material = type == Native.MISSING ? null : Conv.fromNative(type, org.bukkit.Material.class);
            if (material instanceof org.bukkit.Material m) for (Class<?> face : BlockStates.faces(m)) {
                try { add(faces, face); } catch (Throwable ignored) {}
            }
        }
        if (stack || ItemMeta.class.isAssignableFrom(expected)) {
            add(faces, ItemMeta.class);
            for (Class<?> meta : Items.metaFaces(h.extra)) add(faces, meta);
        }
        boolean fits = expected == Object.class;
        for (Class<?> f : faces) if (expected.isAssignableFrom(f)) fits = true;
        if (!fits) add(faces, expected);
        if (faces.isEmpty()) {
            // A server object of no Bukkit interface asked for as a class: hand back the handle itself.
            return expected.isInstance(h) ? h : null;
        }
        Object proxy;
        // A player is CraftBukkit's CraftPlayer (LuckPerms sets its `perm`; plugins check `instanceof CraftPlayer`).
        if (faces.contains(org.bukkit.entity.Player.class) || kind == org.bukkit.entity.Player.class) {
            proxy = org.bukkit.craftbukkit.entity.CraftPlayer.make(h);
            if (!h.key.isEmpty()) {
                Object raced = PLAYERS.putIfAbsent(h.key, proxy);
                if (raced != null) return raced;
                BY_KEY.put(h.key, new WeakReference<>(proxy));
            }
            return proxy;
        }
        // A plain inventory is Paper's CraftInventoryCustom (HorizonInventory, Gen.inventory): plugins reach into
        // its `inventory` field (Duels). Kinds of inventory (player, furnace...) stay as they are.
        if (faces.contains(org.bukkit.inventory.Inventory.class) && faces.stream().noneMatch(f -> f != org.bukkit.inventory.Inventory.class && org.bukkit.inventory.Inventory.class.isAssignableFrom(f))
                && faces.stream().allMatch(f -> f.isAssignableFrom(org.bukkit.inventory.Inventory.class))) {
            proxy = org.bukkit.craftbukkit.inventory.HorizonInventory.make(h);
            if (!h.key.isEmpty()) BY_KEY.put(h.key, new WeakReference<>(proxy));
            return proxy;
        }
        // A world is Paper's CraftWorld (HorizonWorld, Gen.world): plugins cast to it and ask getHandle().
        if (faces.contains(org.bukkit.World.class) || kind == org.bukkit.World.class) {
            proxy = org.bukkit.craftbukkit.HorizonWorld.make(h);
            if (!h.key.isEmpty()) {
                Object raced = WORLDS.putIfAbsent(h.key, proxy);
                if (raced != null) return raced;
                BY_KEY.put(h.key, new WeakReference<>(proxy));
            }
            return proxy;
        }
        NativeHandler handler = new NativeHandler(h);
        // An item meta: its kind's generated class (Gen.metas), which is every interface Paper's meta class is
        // (SkullMeta and Damageable both: a Proxy can't be, their clone()s clash).
        if (stack || ItemMeta.class.isAssignableFrom(expected)) {
            Class<?> metaClass = Items.metaClass(h.extra);
            if (metaClass != null && expected.isAssignableFrom(metaClass)) {
                try {
                    proxy = metaClass.getConstructor(InvocationHandler.class).newInstance(handler);
                    CLEANER.register(proxy, new Release(h.id));
                    if (!h.key.isEmpty()) BY_KEY.put(h.key, new WeakReference<>(proxy));
                    return proxy;
                } catch (ReflectiveOperationException ignored) {
                }
            }
        }
        // The proxy class for this set of interfaces, worked out once (clashing ones dropped): a meta clone was
        // 135 us of failed attempts each time, now one constructor call.
        List<Class<?>> cacheKey = new ArrayList<>(faces);
        cacheKey.add(expected);
        java.lang.reflect.Constructor<?> made = CTORS.get(cacheKey);
        if (made != null) {
            try {
                proxy = made.newInstance(handler);
                CLEANER.register(proxy, new Release(h.id));
                if (!h.key.isEmpty()) BY_KEY.put(h.key, new WeakReference<>(proxy));
                if (proxy instanceof org.bukkit.World && !h.key.isEmpty()) WORLDS.putIfAbsent(h.key, proxy);
                return proxy;
            } catch (ReflectiveOperationException e) {
                CTORS.remove(cacheKey);
            }
        }
        proxy = null;
        // Interfaces that clash (SkullMeta, Damageable and Repairable each narrow clone()): the generic extras go
        // first, then whatever is left beyond the asked-for one.
        List<Class<?>> list = new ArrayList<>(faces);
        while (proxy == null) {
            try {
                proxy = Proxy.newProxyInstance(loaderFor(list), list.toArray(new Class<?>[0]), handler);
            } catch (IllegalArgumentException e) {
                Class<?> drop = null;
                for (Class<?> c : list) if (c.getSimpleName().equals("Repairable")) drop = c;
                if (drop == null) for (Class<?> c : list) if (c.getSimpleName().equals("Damageable") && c.getName().contains(".meta.")) drop = c;
                if (drop == null) for (Class<?> c : list) if (c != expected) drop = c;
                if (drop == null || list.size() == 1) {
                    list = List.of(expected.isInterface() ? expected : faces.iterator().next());
                    proxy = Proxy.newProxyInstance(loaderFor(list), list.toArray(new Class<?>[0]), handler);
                    break;
                }
                list.remove(drop);
            }
        }
        try {
            java.lang.reflect.Constructor<?> k = proxy.getClass().getConstructor(java.lang.reflect.InvocationHandler.class);
            CTORS.put(cacheKey, k);
        } catch (NoSuchMethodException ignored) {}
        CLEANER.register(proxy, new Release(h.id));
        if (!h.key.isEmpty()) BY_KEY.put(h.key, new WeakReference<>(proxy));
        if (proxy instanceof org.bukkit.World && !h.key.isEmpty()) WORLDS.putIfAbsent(h.key, proxy);
        return proxy;
    }

    /** A world unloaded: its object goes (Bukkit.unloadWorld). */
    static void forgetWorld(org.bukkit.World w) {
        WORLDS.values().removeIf(v -> v == w);
    }

    private static final Map<List<Class<?>>, java.lang.reflect.Constructor<?>> CTORS = new java.util.concurrent.ConcurrentHashMap<>();

    /** Implemented by Java classes that stand for a server object (HItemStack). */
    public interface Handled {
        H horizonHandle();
    }
}
