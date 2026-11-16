package horizon.javalayer;

import org.bukkit.Location;
import org.bukkit.event.*;
import org.bukkit.plugin.EventExecutor;
import org.bukkit.plugin.Plugin;
import org.bukkit.util.Vector;

import java.lang.reflect.*;
import java.util.*;
import java.util.concurrent.ConcurrentHashMap;
import java.util.concurrent.CopyOnWriteArrayList;

/**
 * Bukkit events. Plugins' listeners are kept here; for each event class one listens to, the server is asked
 * for that event at that priority. When the server fires it, a real Bukkit event object is made and filled from
 * the server's event (its fields, through their getters), the listeners run, and what they changed goes back.
 */
public final class Events {
    private Events() {}

    static final class Reg {
        final Plugin plugin;
        final Listener listener;
        final Class<? extends Event> type;
        final EventPriority priority;
        final boolean ignoreCancelled;
        final EventExecutor executor;

        Reg(Plugin plugin, Listener listener, Class<? extends Event> type, EventPriority priority, boolean ignoreCancelled, EventExecutor executor) {
            this.plugin = plugin;
            this.listener = listener;
            this.type = type;
            this.priority = priority;
            this.ignoreCancelled = ignoreCancelled;
            this.executor = executor;
        }
    }

    private static final List<Reg> ALL = new CopyOnWriteArrayList<>();
    private static final Map<Class<?>, List<Reg>[]> BY_CLASS = new ConcurrentHashMap<>();
    private static final Set<String> NOT_FIRED = ConcurrentHashMap.newKeySet();

    /** The class whose HandlerList an event class is dispatched by (Bukkit: the nearest declaring getHandlerList). */
    private static final Map<Class<?>, Class<?>> OWNER = new ConcurrentHashMap<>();
    private static final Class<?> NO_OWNER = Void.class;

    static Class<?> owner(Class<?> type) {
        Class<?> found = OWNER.computeIfAbsent(type, t -> {
            for (Class<?> at = t; at != null && at != Event.class && at != Object.class; at = at.getSuperclass()) {
                try {
                    at.getDeclaredMethod("getHandlerList");
                    return at;
                } catch (NoSuchMethodException ignored) {}
            }
            return NO_OWNER;
        });
        return found == NO_OWNER ? null : found;
    }

    /** Every API event class by the HandlerList owner it shares (horizon-events.txt, written by Gen). */
    private static volatile Map<String, List<String>> families;

    private static synchronized Map<String, List<String>> families() {
        if (families != null) return families;
        Map<String, List<String>> out = new HashMap<>();
        Set<String> fired = new HashSet<>(Arrays.asList(Native.eventClasses()));
        try (var in = Events.class.getResourceAsStream("/horizon-events.txt")) {
            if (in != null) {
                for (String line : new String(in.readAllBytes(), java.nio.charset.StandardCharsets.UTF_8).split("\\R")) {
                    String[] parts = line.trim().split(" ");
                    if (parts.length == 2 && fired.contains(parts[0])) out.computeIfAbsent(parts[1], k -> new ArrayList<>()).add(parts[0]);
                }
            }
        } catch (java.io.IOException ignored) {}
        // Subclasses (no HandlerList of their own) first: they fire before the owner, which then knows it was a subclass hit.
        for (Map.Entry<String, List<String>> e : out.entrySet()) e.getValue().sort((a, b) -> a.equals(e.getKey()) ? 1 : b.equals(e.getKey()) ? -1 : a.compareTo(b));
        families = out;
        return out;
    }

    static void add(Reg reg) {
        Class<?> owner = owner(reg.type);
        if (owner == null) {
            Logs.warning(reg.plugin.getName() + " tried to listen to " + reg.type.getName() + ", which has no HandlerList (Bukkit refuses it too)");
            return;
        }
        ALL.add(reg);
        BY_CLASS.clear();
        // The server fires every class of this HandlerList: the owner and its subclasses that share it
        // (an EntityDamageEvent listener hears EntityDamageByEntityEvent too). A plugin's own event needs none.
        List<String> family = families().get(owner.getName());
        boolean any = Synth.own(owner);
        Synth.want(owner);
        if (family != null) for (String member : family) any |= Native.wantEvent(member, reg.priority.getSlot());
        String name = owner.getName();
        boolean api = name.startsWith("org.bukkit.") || name.startsWith("io.papermc.") || name.startsWith("com.destroystokyo.") || name.startsWith("org.spigotmc.");
        if (!any && api && NOT_FIRED.add(name)) {
            Logs.warning(reg.plugin.getName() + " listens to " + reg.type.getSimpleName() + ", which this server doesn't fire yet");
        }
    }

    static void removeAll(Plugin plugin) {
        ALL.removeIf(r -> r.plugin == plugin);
        BY_CLASS.clear();
    }

    static void removeListener(Listener listener) {
        ALL.removeIf(r -> r.listener == listener);
        BY_CLASS.clear();
    }

    // ---- Bukkit's HandlerList.unregister*, reaching the listeners the layer keeps (org.bukkit.event.HandlerList) ----

    /** HandlerList.unregister(Listener) on one event's list: that listener's handlers of that event only. */
    public static void unregistered(HandlerList list, Listener listener) {
        if (ALL.removeIf(r -> r.listener == listener && handlerList(owner(r.type)) == list)) BY_CLASS.clear();
    }

    /** HandlerList.unregister(Plugin) on one event's list. */
    public static void unregistered(HandlerList list, Plugin plugin) {
        if (ALL.removeIf(r -> r.plugin == plugin && handlerList(owner(r.type)) == list)) BY_CLASS.clear();
    }

    /** HandlerList.unregisterAll(Listener): that listener's handlers of every event. */
    public static void unregisteredEverywhere(Listener listener) {
        removeListener(listener);
    }

    /** HandlerList.unregisterAll(Plugin). */
    public static void unregisteredEverywhere(Plugin plugin) {
        removeAll(plugin);
    }

    /** HandlerList.unregisterAll(): every plugin's listeners. */
    public static void unregisteredEverything() {
        ALL.clear();
        BY_CLASS.clear();
    }

    static List<Reg> listenersOf(Class<?> type) {
        List<Reg> out = new ArrayList<>();
        for (Reg r : ALL) if (r.type == type) out.add(r);
        return out;
    }

    @SuppressWarnings("unchecked")
    private static List<Reg>[] forClass(Class<?> type) {
        return BY_CLASS.computeIfAbsent(type, t -> {
            List<Reg>[] slots = new List[6];
            for (int i = 0; i < 6; i++) slots[i] = new ArrayList<>();
            Class<?> owner = owner(t);
            for (Reg r : ALL) if (owner != null && owner(r.type) == owner) slots[r.priority.getSlot()].add(r);
            return slots;
        });
    }

    static void dispatch(Event event, int slot) {
        List<Reg> regs = forClass(event.getClass())[slot];
        for (Reg r : regs) {
            if (!r.plugin.isEnabled()) continue;
            if (r.ignoreCancelled && event instanceof Cancellable c && c.isCancelled()) continue;
            try {
                r.executor.execute(r.listener, event);
            } catch (Throwable e) {
                Throwable cause = e instanceof EventException && e.getCause() != null ? e.getCause() : e;
                Logs.severe("Could not pass event " + event.getEventName() + " to " + r.plugin.getName(), cause);
                Synth.exception(new com.destroystokyo.paper.exception.ServerEventException("Could not pass event " + event.getEventName() + " to " + r.plugin.getName(), cause, r.plugin, r.listener, event));
            }
        }
        // Listeners put straight on the event's Bukkit HandlerList (MCCoroutine's registerSuspendingEvents and other
        // frameworks: HandlerList.register/registerAll), which Bukkit calls as it calls the rest.
        HandlerList list = handlerList(owner(event.getClass()));
        if (list == null) return;
        for (org.bukkit.plugin.RegisteredListener rl : list.getRegisteredListeners()) {
            if (rl.getPriority().getSlot() != slot || !rl.getPlugin().isEnabled()) continue;
            try {
                rl.callEvent(event);
            } catch (Throwable e) {
                Throwable cause = e instanceof EventException && e.getCause() != null ? e.getCause() : e;
                Logs.severe("Could not pass event " + event.getEventName() + " to " + rl.getPlugin().getName(), cause);
            }
        }
    }

    /** Whether a listener put straight on this event's HandlerList (not by the layer) listens at this priority. */
    static boolean externalAt(Class<?> type, int slot) {
        HandlerList list = handlerList(owner(type));
        if (list == null) return false;
        for (org.bukkit.plugin.RegisteredListener rl : list.getRegisteredListeners()) if (rl.getPriority().getSlot() == slot) return true;
        return false;
    }

    /** An event class's own HandlerList (its static getHandlerList), or null. */
    private static final Map<Class<?>, Object> LISTS = new ConcurrentHashMap<>();

    static HandlerList handlerList(Class<?> owner) {
        if (owner == null) return null;
        Object got = LISTS.computeIfAbsent(owner, o -> {
            try {
                Method m = o.getDeclaredMethod("getHandlerList");
                m.setAccessible(true);
                Object hl = m.invoke(null);
                return hl instanceof HandlerList h ? h : NO_OWNER;
            } catch (ReflectiveOperationException | RuntimeException | LinkageError e) {
                return NO_OWNER;
            }
        });
        return got instanceof HandlerList h ? h : null;
    }

    /** The events whose HandlerLists got listeners from outside the layer: the server fires them for those too.
     *  Run after each plugin enables and every few seconds (a framework may register later). */
    private static final Set<String> WANTED_EXTERNAL = ConcurrentHashMap.newKeySet();

    /** Each fired event family's HandlerList -> its owner class name: made once (`mapOwners`, at a plugin's enable),
     *  so the per-second check loads no class and takes no lock. */
    private static volatile Map<HandlerList, String> LIST_OWNERS;

    static void mapOwners() {
        if (LIST_OWNERS != null) return;
        Map<HandlerList, String> out = new IdentityHashMap<>();
        for (String ownerName : families().keySet()) {
            try {
                HandlerList list = handlerList(Class.forName(ownerName, false, Events.class.getClassLoader()));
                if (list != null) out.put(list, ownerName);
            } catch (ClassNotFoundException | LinkageError e) {
                // Not in this API: never fired.
            }
        }
        LIST_OWNERS = out;
    }

    static void syncExternal() {
        Map<HandlerList, String> owners = LIST_OWNERS;
        if (owners == null) return;
        Map<String, List<String>> fam = families;
        if (fam == null) return;
        for (HandlerList list : HandlerList.getHandlerLists()) {
            org.bukkit.plugin.RegisteredListener[] regs = list.getRegisteredListeners();
            if (regs.length == 0) continue;
            String ownerName = owners.get(list);
            if (ownerName == null) continue;
            for (org.bukkit.plugin.RegisteredListener rl : regs) {
                int slot = rl.getPriority().getSlot();
                if (!WANTED_EXTERNAL.add(ownerName + "#" + slot)) continue;
                for (String member : fam.getOrDefault(ownerName, List.of())) Native.wantEvent(member, slot);
            }
        }
    }

    /** A plugin's own `callEvent`: every priority, in order. */
    static void call(Event event) {
        for (int slot = 0; slot < 6; slot++) dispatch(event, slot);
    }

    /** `@EventHandler` methods of a listener. */
    @SuppressWarnings("unchecked")
    static void register(Listener listener, Plugin plugin) {
        Set<Method> methods = new LinkedHashSet<>();
        try {
            methods.addAll(Arrays.asList(listener.getClass().getMethods()));
            methods.addAll(Arrays.asList(listener.getClass().getDeclaredMethods()));
        } catch (NoClassDefFoundError e) {
            Logs.severe(plugin.getName() + " tried to register a listener that needs a class this server doesn't have: " + e.getMessage(), null);
            return;
        }
        for (Method method : methods) {
            EventHandler handler = method.getAnnotation(EventHandler.class);
            if (handler == null || method.isBridge() || method.isSynthetic()) continue;
            if (method.getParameterCount() != 1 || !Event.class.isAssignableFrom(method.getParameterTypes()[0])) {
                Logs.warning(plugin.getName() + " has an invalid @EventHandler: " + method);
                continue;
            }
            Class<? extends Event> type = (Class<? extends Event>) method.getParameterTypes()[0];
            method.setAccessible(true);
            EventExecutor executor = (l, event) -> {
                if (!type.isInstance(event)) return;
                try {
                    method.invoke(l, event);
                } catch (InvocationTargetException e) {
                    throw new EventException(e.getCause());
                } catch (Throwable e) {
                    throw new EventException(e);
                }
            };
            add(new Reg(plugin, listener, type, handler.priority(), handler.ignoreCancelled(), executor));
        }
    }

    // ---- events the server fires ----

    private static final Object UNSAFE;
    private static final Method ALLOCATE;

    static {
        Object u = null;
        Method a = null;
        try {
            Class<?> unsafe = Class.forName("sun.misc.Unsafe");
            Field f = unsafe.getDeclaredField("theUnsafe");
            f.setAccessible(true);
            u = f.get(null);
            a = unsafe.getMethod("allocateInstance", Class.class);
        } catch (ReflectiveOperationException ignored) {}
        UNSAFE = u;
        ALLOCATE = a;
    }

    /** Collections the event's constructor would have made (it isn't run: events are allocated, then filled). */
    static void fillCollections(Event event, List<Slot> slots) {
        for (Slot s : slots) {
            try {
                if (s.field.get(event) != null) continue;
                Class<?> t = s.field.getType();
                Object empty = t == Map.class ? new java.util.LinkedHashMap<>() : t == List.class ? new ArrayList<>() : t == Set.class ? new java.util.LinkedHashSet<>()
                        : t == Collection.class ? new ArrayList<>() : null;
                if (empty != null) s.field.set(event, empty);
            } catch (ReflectiveOperationException | RuntimeException ignored) {
            }
        }
    }

    /** A structure's transformers the listeners set, onto the server's event (its adapters run them). */
    private static void writeTransformers(org.bukkit.event.world.AsyncStructureGenerateEvent e, H h) {
        for (Map.Entry<org.bukkit.NamespacedKey, org.bukkit.util.BlockTransformer> t : e.getBlockTransformers().entrySet()) {
            Native.tryCall(h, "setBlockTransformer", Conv.args(new Object[]{t.getKey(), t.getValue()}));
        }
        for (Map.Entry<org.bukkit.NamespacedKey, org.bukkit.util.EntityTransformer> t : e.getEntityTransformers().entrySet()) {
            Native.tryCall(h, "setEntityTransformer", Conv.args(new Object[]{t.getKey(), t.getValue()}));
        }
    }

    private static Field worldField;

    /** A world event the server gave no world (ChunkUnloadEvent): its chunk's, as Paper's comes from the chunk. */
    static void fillWorld(Event event) {
        if (!(event instanceof org.bukkit.event.world.WorldEvent we)) return;
        try {
            if (worldField == null) {
                Field f = org.bukkit.event.world.WorldEvent.class.getDeclaredField("world");
                f.setAccessible(true);
                worldField = f;
            }
            if (worldField.get(event) != null) return;
            org.bukkit.World w = null;
            if (event instanceof org.bukkit.event.world.ChunkEvent ce && ce.getChunk() != null) w = ce.getChunk().getWorld();
            else if (event instanceof org.bukkit.event.world.StructureGrowEvent sg && sg.getLocation() != null) w = sg.getLocation().getWorld();
            else if (event instanceof org.bukkit.event.world.PortalCreateEvent pc && !pc.getBlocks().isEmpty()) w = pc.getBlocks().get(0).getWorld();
            if (w != null) worldField.set(event, w);
        } catch (ReflectiveOperationException | RuntimeException ignored) {
        }
    }

    /** How one field of an event class is read from (and written back to) the server's event. */
    static final class Slot {
        final Field field;
        final String[] getters;
        final String[] setters;
        String getter;
        boolean tried;

        Slot(Field field, String[] getters, String[] setters) {
            this.field = field;
            this.getters = getters;
            this.setters = setters;
        }
    }

    private static final Map<Class<?>, List<Slot>> SLOTS = new ConcurrentHashMap<>();

    static String cap(String name) { return Character.toUpperCase(name.charAt(0)) + name.substring(1); }

    private static List<Slot> slotsOf(Class<?> type) {
        return SLOTS.computeIfAbsent(type, t -> {
            List<Slot> out = new ArrayList<>();
            boolean cancellable = Cancellable.class.isAssignableFrom(t);
            for (Class<?> at = t; at != null && at != Event.class && at != Object.class; at = at.getSuperclass()) {
                for (Field f : at.getDeclaredFields()) {
                    if (Modifier.isStatic(f.getModifiers()) || f.isSynthetic()) continue;
                    if (f.getType() == HandlerList.class) continue;
                    try {
                        f.setAccessible(true);
                    } catch (RuntimeException e) {
                        continue;
                    }
                    String n = f.getName();
                    // Bukkit's names that differ from their getters: InventoryEvent keeps its view as `transaction`.
                    if (n.equals("transaction")) n = "view";
                    String c = cap(n);
                    String[] getters, setters;
                    if (cancellable && (n.equals("cancelled") || n.equals("cancel") || n.equals("isCancelled") || n.equals("canceled"))) {
                        getters = new String[]{"isCancelled"};
                        setters = new String[]{"setCancelled"};
                    } else if (f.getType() == boolean.class || f.getType() == Boolean.class) {
                        String bare = n.startsWith("is") && n.length() > 2 && Character.isUpperCase(n.charAt(2)) ? n.substring(2) : c;
                        // `allow` read by isAllowed() (PlayerServerFullCheckEvent: false here before, so EssentialsX
                        // took every join for a full server).
                        getters = new String[]{"is" + bare, "get" + c, n, "has" + c, "should" + c, "is" + bare + "ed", "is" + bare + "d"};
                        setters = new String[]{"set" + bare, "set" + c, n};
                    } else {
                        // `profile` is read by `getPlayerProfile()`: a getter named after the field's type too.
                        String byType = "get" + f.getType().getSimpleName();
                        getters = byType.equals("get" + c) ? new String[]{"get" + c, n, "is" + c} : new String[]{"get" + c, n, "is" + c, byType};
                        // A BlockData field (EntityChangeBlockEvent's `to`): the server's full state first; its
                        // getTo is the Material, the block's name only.
                        if (f.getType() == org.bukkit.block.data.BlockData.class && !byType.equals("get" + c)) getters = new String[]{byType, "get" + c, n};
                        setters = byType.equals("get" + c) ? new String[]{"set" + c, n} : new String[]{"set" + c, n, "set" + f.getType().getSimpleName()};
                        // A getter that names the field with more words (PreCreatureSpawnEvent's `location` is read
                        // by getSpawnLocation(): Multiverse read null on every spawn): the class's own public getters
                        // of the field's type whose names end with it.
                        List<String> more = new ArrayList<>(List.of(getters));
                        for (Method g : t.getMethods()) {
                            if (g.getParameterCount() == 0 && g.getName().startsWith("get") && g.getName().endsWith(c) && !more.contains(g.getName())
                                    && f.getType().isAssignableFrom(g.getReturnType()) && g.getDeclaringClass() != Object.class) more.add(g.getName());
                        }
                        getters = more.toArray(new String[0]);
                    }
                    // PlayerServerFullCheckEvent's kick message has no setter on Paper (EssentialsX sets the field):
                    // the server takes it by deny(message), and the event's `allow`, written after it, stands.
                    if (n.equals("kickMessage") && t.getName().equals("io.papermc.paper.event.player.PlayerServerFullCheckEvent")) setters = new String[]{"deny"};
                    out.add(new Slot(f, getters, setters));
                }
            }
            return out;
        });
    }

    /** What a listener could change in place (a Location, a list of drops) is compared by a copy. */
    private static Object copy(Object v) {
        if (v instanceof Location l) return l.clone();
        if (v instanceof Vector vec) return vec.clone();
        if (v instanceof List<?> list) return new ArrayList<>(list);
        if (v instanceof Set<?> set) return new LinkedHashSet<>(set);
        if (v instanceof Map<?, ?> map) return new LinkedHashMap<>(map);
        if (v instanceof Object[] arr) return arr.clone();
        return v;
    }

    private static final Set<String> NO_SETTER = ConcurrentHashMap.newKeySet();

    /** The server fired `className` at priority `slot`; its event is `h` while this runs. */
    public static void fire(String className, H h, int slot) {
        try {
            Class<?> type = Class.forName(className, true, Events.class.getClassLoader());
            boolean conversation = slot == 0 && (Conversations.wants(type) || type == io.papermc.paper.event.player.PlayerCustomClickEvent.class
                    || type == org.bukkit.event.player.PlayerJoinEvent.class && !listenersOf(org.spigotmc.event.player.PlayerSpawnLocationEvent.class).isEmpty());
            if (forClass(type)[slot].isEmpty() && !conversation && !externalAt(type, slot)) return;
            Class<?> owner = owner(type);
            if (ALLOCATE == null) throw new IllegalStateException("sun.misc.Unsafe is not available");
            Event event = (Event) ALLOCATE.invoke(UNSAFE, type);
            List<Slot> slots = slotsOf(type);
            Object[] before = new Object[slots.size()];
            for (int i = 0; i < slots.size(); i++) {
                Slot s = slots.get(i);
                Object value = read(h, s);
                if (value != Native.MISSING) {
                    try {
                        s.field.set(event, value);
                    } catch (IllegalArgumentException | IllegalAccessException e) {
                        value = Native.MISSING;
                    }
                }
                before[i] = value == Native.MISSING ? Native.MISSING : copy(value);
            }
            markAsync(event, className);
            fillProfile(event, slots, h);
            fillWorld(event);
            fillCollections(event, slots);
            // What the layer filled in itself (a profile made from the UUID, the world) is the event as it came:
            // only the listeners' changes go back to the server.
            for (int i = 0; i < slots.size(); i++) {
                if (before[i] != Native.MISSING) before[i] = copy(slots.get(i).field.get(event));
            }
            double damageBefore = Damage.fill(event, h);
            // One hit fires the SDK's subclass event and its parent's (EntityDamageByEntityEvent, then EntityDamageEvent):
            // Paper fires only the most specific, so the parent's copy of a hit a subclass already took is skipped.
            String subject = subject(event, slots);
            String mark = owner.getName() + "|" + slot;
            long tick = Scheduler.INSTANCE.currentTick();
            if (type != owner) {
                LAST_SUB.put(mark, subject + "@" + tick);
            } else if ((subject + "@" + tick).equals(LAST_SUB.get(mark))) {
                LAST_SUB.remove(mark);
                return;
            }
            if (conversation) Synth.beforeJoin(event);
            if (conversation && Callbacks.dialogClicked(event)) conversation = true;
            else if (!(conversation && Conversations.consume(event))) dispatch(event, slot);
            Damage.writeBack(event, h, damageBefore);
            if (event instanceof org.bukkit.event.world.AsyncStructureGenerateEvent structure) writeTransformers(structure, h);
            if (event instanceof org.bukkit.event.player.PlayerQuitEvent quit && slot == 5) {
                H player = Proxies.handleOf(quit.getPlayer());
                if (player != null) Proxies.playerLeft(player.key);
                // The player's channel closes (PacketEvents lets its User go), as their connection's does on Paper.
                try {
                    horizon.javalayer.nms.Hollow.left(quit.getPlayer().getUniqueId());
                } catch (LinkageError ignored) {
                }
            }
            for (int i = 0; i < slots.size(); i++) {
                if (before[i] == Native.MISSING) continue;
                Slot s = slots.get(i);
                if (Damage.isDamageMap(s.field)) continue;
                Object now = s.field.get(event);
                if (Objects.equals(now, before[i]) && !(now instanceof Location l && !sameLocation(l, before[i]))) continue;
                write(h, s, now, type);
            }
        } catch (Throwable e) {
            Logs.severe("[JavaLayer] event " + className, e instanceof InvocationTargetException ite && ite.getCause() != null ? ite.getCause() : e);
        } finally {
            Native.release(h.id);
        }
    }

    private static final Map<String, String> LAST_SUB = new ConcurrentHashMap<>();

    /** A login event's `profile` the server has no getter for: made from its name and UUID, as Paper's is. */
    private static void fillProfile(Event event, List<Slot> slots, H h) {
        Slot profile = null;
        UUID id = null;
        String name = null;
        try {
            for (Slot s : slots) {
                Object v = s.field.get(event);
                // No profile, or one without its UUID yet (offline mode, before login): made again below.
                if (s.field.getType() == com.destroystokyo.paper.profile.PlayerProfile.class) {
                    com.destroystokyo.paper.profile.PlayerProfile have = (com.destroystokyo.paper.profile.PlayerProfile) v;
                    profile = have == null || have.getId() == null ? s : null;
                    if (have != null && name == null) name = have.getName();
                }
                else if (v instanceof UUID u && id == null) id = u;
                else if (s.field.getName().equals("name") && v instanceof String n) name = n;
            }
            if (profile == null) return;
            // Paper keeps the joining player's UUID and name only in the profile: ask the server's event for them.
            if (id == null) {
                Object raw = Native.tryCall(h, "getUniqueId", new Object[0]);
                if (raw instanceof String s) {
                    try { id = UUID.fromString(s); } catch (IllegalArgumentException ignored) {}
                }
            }
            if (name == null) {
                Object raw = Native.tryCall(h, "getName", new Object[0]);
                if (raw instanceof String s) name = s;
            }
            // Offline mode: the game's UUID from the name, as vanilla makes it.
            if (id == null && name != null) id = UUID.nameUUIDFromBytes(("OfflinePlayer:" + name).getBytes(java.nio.charset.StandardCharsets.UTF_8));
            if (id != null || name != null) profile.field.set(event, org.bukkit.Bukkit.createProfile(id, name));
        } catch (Throwable ignored) {}
    }

    /** What an event is about (its entity, player or block), to pair a subclass's fire with its parent's. */
    private static String subject(Event event, List<Slot> slots) {
        for (String want : new String[]{"entity", "player", "block", "caught", "item"}) {
            for (Slot s : slots) {
                if (!s.field.getName().equals(want)) continue;
                try {
                    Object v = s.field.get(event);
                    if (v == null) continue;
                    H h = Proxies.handleOf(v);
                    if (h != null && !h.key.isEmpty()) return h.key;
                    if (v instanceof org.bukkit.block.Block b) return b.getX() + "," + b.getY() + "," + b.getZ();
                    return String.valueOf(v);
                } catch (IllegalAccessException ignored) {}
            }
        }
        return "";
    }

    private static boolean sameLocation(Location a, Object b) {
        return b instanceof Location l && a.getX() == l.getX() && a.getY() == l.getY() && a.getZ() == l.getZ() && a.getYaw() == l.getYaw() && a.getPitch() == l.getPitch() && Objects.equals(a.getWorld(), l.getWorld());
    }

    private static final Field ASYNC;

    static {
        Field f = null;
        try {
            f = Event.class.getDeclaredField("async");
            f.setAccessible(true);
        } catch (ReflectiveOperationException ignored) {}
        ASYNC = f;
    }

    private static void markAsync(Event event, String className) {
        if (ASYNC == null) return;
        try {
            ASYNC.setBoolean(event, className.substring(className.lastIndexOf('.') + 1).startsWith("Async") && !Native.isMainThread());
        } catch (IllegalAccessException ignored) {}
    }

    private static Object read(H h, Slot s) {
        Type type = s.field.getGenericType();
        if (s.tried) {
            if (s.getter == null) return Native.MISSING;
            Object raw = Native.tryCall(h, s.getter, new Object[0]);
            return raw == Native.MISSING ? Native.MISSING : Conv.fromNative(raw, type);
        }
        for (String getter : s.getters) {
            Object raw;
            try {
                raw = Native.tryCall(h, getter, new Object[0]);
            } catch (RuntimeException e) {
                continue;
            }
            if (raw == Native.MISSING) continue;
            Object value = Conv.fromNative(raw, type);
            if (value != null && !Conv.rawClass(type).isInstance(value) && !s.field.getType().isPrimitive()) continue;
            s.getter = getter;
            s.tried = true;
            return value;
        }
        s.tried = true;
        return Native.MISSING;
    }

    private static void write(H h, Slot s, Object value, Class<?> type) {
        Object arg = Conv.toNative(value);
        String[] setters = s.setters;
        // A component (`joinMessage`): Paper's Adventure setter first; Bukkit's String one gets legacy text.
        if (value instanceof net.kyori.adventure.text.ComponentLike c) {
            String bare = s.setters.length > 1 ? s.setters[s.setters.length - 1] : s.setters[0];
            Object raw = Native.tryCall(h, bare, new Object[]{arg});
            if (raw != Native.MISSING) return;
            arg = net.kyori.adventure.text.serializer.legacy.LegacyComponentSerializer.legacySection().serialize(c.asComponent());
        }
        for (String setter : setters) {
            Object raw;
            try {
                raw = Native.tryCall(h, setter, new Object[]{arg});
            } catch (RuntimeException e) {
                Logs.warning(type.getSimpleName() + "." + setter + ": " + e.getMessage());
                return;
            }
            if (raw != Native.MISSING) return;
        }
        if (NO_SETTER.add(type.getName() + "." + s.field.getName())) {
            Logs.warning("a plugin changed " + type.getSimpleName() + "." + s.field.getName() + ", which this server can't take back yet");
        }
    }
}
