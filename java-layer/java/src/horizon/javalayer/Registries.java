package horizon.javalayer;

import io.papermc.paper.registry.RegistryAccess;
import io.papermc.paper.registry.RegistryKey;
import net.kyori.adventure.key.Key;
import org.bukkit.Keyed;
import org.bukkit.Material;
import org.bukkit.NamespacedKey;
import org.bukkit.Registry;
import org.bukkit.block.BlockType;
import org.bukkit.inventory.ItemType;

import java.lang.reflect.*;
import java.util.*;
import java.util.concurrent.ConcurrentHashMap;
import java.util.stream.Stream;

/**
 * Paper's registries (`Registry.SOUNDS`, `Material.asItemType()`, `Sound.ENTITY_PLAYER_LEVELUP`...). A value is a
 * small Java object holding its key; the server is told its Bukkit name (`ENTITY_PLAYER_LEVELUP`), as the SDK
 * and jport's converted plugins name them. Items and blocks ask the server what each material is.
 */
public final class Registries extends io.papermc.paper.registry.PaperRegistryAccess {
    public Registries() {}

    // Vanilla's registries announce themselves to Paper's registry access while vanilla boots (NMS plugins): the
    // layer answers the API's registries itself, so these are taken and ignored.
    @Override public <M> void registerRegistry(net.minecraft.core.Registry<M> registry) {}
    @Override public <M> void registerReloadableRegistry(net.minecraft.core.Registry<M> registry) {}
    @Override public <M> void lockReferenceHolders(net.minecraft.resources.ResourceKey<? extends net.minecraft.core.Registry<M>> key) {}

    /** A registry value: interface ones are proxies with a {@link Keyed} handler, class ones Gen's K_ classes. */
    public interface Value {
        NamespacedKey horizonKey();
        String horizonName();
    }

    private static final Map<Object, Class<?>> TYPES = new ConcurrentHashMap<>();
    private static final Map<Class<?>, Object> KEYS_BY_TYPE = new ConcurrentHashMap<>();
    private static final Map<Object, Registry<?>> REGISTRIES = new ConcurrentHashMap<>();
    private static final Map<String, Object> VALUES = new ConcurrentHashMap<>();
    private static final Map<String, Boolean> MATERIAL_IS = new ConcurrentHashMap<>();
    private static volatile boolean scanned;

    private static void scan() {
        if (scanned) return;
        synchronized (TYPES) {
            if (scanned) return;
            for (Field f : RegistryKey.class.getFields()) {
                if (!Modifier.isStatic(f.getModifiers()) || !(f.getGenericType() instanceof ParameterizedType p)) continue;
                try {
                    Object key = f.get(null);
                    Class<?> type = Conv.rawClass(p.getActualTypeArguments()[0]);
                    TYPES.put(key, type);
                    KEYS_BY_TYPE.putIfAbsent(type, key);
                } catch (Throwable ignored) {}
            }
            scanned = true;
        }
    }

    static boolean isRegistryType(Class<?> c) {
        scan();
        return KEYS_BY_TYPE.containsKey(c);
    }

    @Override
    @SuppressWarnings("unchecked")
    public <T extends Keyed> Registry<T> getRegistry(Class<T> type) {
        scan();
        Object key = KEYS_BY_TYPE.get(type);
        return key == null ? null : (Registry<T>) registry(key, type);
    }

    @Override
    @SuppressWarnings("unchecked")
    public <T extends Keyed> Registry<T> getRegistry(RegistryKey<T> key) {
        scan();
        Class<?> type = TYPES.getOrDefault(key, Keyed.class);
        return (Registry<T>) registry(key, type);
    }

    private static final Map<String, List<String>> KEY_LISTS = new ConcurrentHashMap<>();

    /** The Paper registry key of a value type (Enchantment -> RegistryKey.ENCHANTMENT). */
    static RegistryKey<?> keyOf(Class<?> type) {
        scan();
        for (Map.Entry<Object, Class<?>> e : TYPES.entrySet()) if (e.getValue() == type && e.getKey() instanceof RegistryKey<?> rk) return rk;
        return null;
    }

    static Registry<?> registry(Object key, Class<?> type) {
        // Not computeIfAbsent: making the first one starts org.bukkit.Registry's static setup, which asks for
        // registries again (ConcurrentHashMap refuses that as a recursive update).
        Registry<?> have = REGISTRIES.get(key);
        if (have != null) return have;
        Registry<?> made = (Registry<?>) Proxy.newProxyInstance(Registries.class.getClassLoader(), new Class<?>[]{Registry.class}, new RegistryHandler(type));
        Registry<?> raced = REGISTRIES.putIfAbsent(key, made);
        return raced != null ? raced : made;
    }

    /** Vanilla 26.3's integer game rules and their defaults; every other rule is a boolean. */
    static final Map<String, Integer> INT_RULES = Map.ofEntries(
            Map.entry("random_tick_speed", 3), Map.entry("respawn_radius", 10), Map.entry("max_entity_cramming", 24),
            Map.entry("max_command_sequence_length", 65536), Map.entry("max_command_forks", 65536),
            Map.entry("max_snow_accumulation_height", 1), Map.entry("players_sleeping_percentage", 100),
            Map.entry("players_nether_portal_default_delay", 80), Map.entry("players_nether_portal_creative_delay", 0),
            Map.entry("max_block_modifications", 32768), Map.entry("spawn_chunk_radius", 2), Map.entry("fire_spread_radius_around_player", 128),
            Map.entry("max_minecart_speed", 8), Map.entry("command_modification_block_limit", 32768));
    /** Vanilla 26.3 mob effects by category (the rest are beneficial). */
    static final Set<String> HARMFUL = Set.of("slowness", "mining_fatigue", "instant_damage", "nausea", "blindness", "hunger", "weakness", "poison",
            "wither", "levitation", "unluck", "darkness", "wind_charged", "weaving", "oozing", "infested", "bad_omen", "raid_omen", "trial_omen");
    static final Set<String> NEUTRAL = Set.of("glowing");
    /** Boolean rules that start off. */
    static final Set<String> FALSE_RULES = Set.of("keep_inventory", "disable_raids", "show_death_messages_false", "reduced_debug_info",
            "spectators_generate_chunks_false", "universal_anger", "forgive_dead_players_false", "freeze_damage_false", "allow_entering_nether_using_portals_false",
            "immediate_respawn", "limited_crafting", "disable_elytra_movement_check", "disable_player_movement_check", "spawn_monsters_false", "locator_bar_false",
            "enable_command_blocks_false", "tnt_explodes_false", "pvp_false", "ender_pearls_vanish_on_death_false");

    static String nameOf(NamespacedKey key) {
        return key.getKey().replace('.', '_').replace('/', '_').toUpperCase(Locale.ROOT);
    }

    /** Whether the server says this material is an item / a block (Paper: `asItemType() != null`). */
    static boolean materialIs(String key, String what) {
        return MATERIAL_IS.computeIfAbsent(what + key, k -> {
            Object raw = Native.tryCall(Pack.of(Pack.MAT, key), what, new Object[0]);
            return raw instanceof Boolean b ? b : true;
        });
    }

    /** The value of this registry type for a key, the same object each time. */
    static Object value(Class<?> type, NamespacedKey key) {
        if (key == null) return null;
        if (type == ItemType.class || type == BlockType.class) {
            Material material = Material.matchMaterial(key.toString());
            if (material == null || material.isLegacy()) return null;
            if (!materialIs(key.toString(), type == ItemType.class ? "isItem" : "isBlock")) return null;
        }
        String id = type.getName() + "|" + key;
        Object have = VALUES.get(id);
        if (have != null) return have;
        Object made = make(type, key, nameOf(key));
        if (made != null) {
            Object raced = VALUES.putIfAbsent(id, made);
            if (raced != null) return raced;
        }
        return made;
    }

    private static Object make(Class<?> type, NamespacedKey key, String name) {
        if (type.isEnum()) {
            // Enum registries (Particle, EntityType...): the constant with this key, else of this name.
            for (Object c : type.getEnumConstants()) {
                try {
                    if (c instanceof Keyed k && key.equals(k.getKey())) return c;
                } catch (RuntimeException ignored) {}
            }
            for (Object c : type.getEnumConstants()) if (((Enum<?>) c).name().equals(name)) return c;
            return null;
        }
        if (type == io.papermc.paper.datacomponent.DataComponentType.class) {
            // DataComponentTypes asks each entry for Valued or NonValued by instanceof: the server knows which by
            // name, the static table only needs both to hold.
            return Proxy.newProxyInstance(Registries.class.getClassLoader(), new Class<?>[]{type, io.papermc.paper.datacomponent.DataComponentType.Valued.class,
                    io.papermc.paper.datacomponent.DataComponentType.NonValued.class, Value.class}, new KeyedHandler(type, key, name));
        }
        if (type.isInterface()) {
            // The type's own nested sub-interfaces too (MenuType.Typed, ItemType.Typed): the API's constants are cast to them.
            List<Class<?>> faces = new ArrayList<>(List.of(type, Value.class));
            for (Class<?> inner : type.getDeclaredClasses()) if (inner.isInterface() && type.isAssignableFrom(inner)) faces.add(inner);
            return Proxy.newProxyInstance(Registries.class.getClassLoader(), faces.toArray(new Class<?>[0]), new KeyedHandler(type, key, name));
        }
        try {
            String simple = "K_" + type.getName().substring(type.getName().lastIndexOf('.') + 1).replace('$', '_');
            Class<?> made = Class.forName("horizon.javalayer." + simple);
            return made.getConstructor(NamespacedKey.class, String.class).newInstance(key, name);
        } catch (ReflectiveOperationException | LinkageError e) {
            Throwable cause = e instanceof java.lang.reflect.InvocationTargetException ite && ite.getCause() != null ? ite.getCause() : e;
            Report.error("registry value " + type.getSimpleName() + " " + key, cause);
            NativeHandler.missing("registry values of " + type.getSimpleName());
            return null;
        }
    }

    static boolean isKeyedValue(Object o) {
        return o instanceof Value;
    }

    /** What the server calls a registry value: its key (`minecraft:efficiency`), as the SDK's constants are. */
    static String nativeName(Object o) {
        return ((Value) o).horizonKey().toString();
    }

    /** A Bukkit name (`SHARPNESS`) or a key (`minecraft:sharpness`) as this registry's value. */
    static Object byName(Class<?> type, String text) {
        if (text == null || text.isEmpty()) return null;
        NamespacedKey key = text.indexOf(':') >= 0 ? NamespacedKey.fromString(text.toLowerCase(Locale.ROOT)) : NamespacedKey.minecraft(text.toLowerCase(Locale.ROOT));
        if (key == null) return null;
        if (type == ItemType.class || type == BlockType.class || type == Keyed.class) return value(type, key);
        String id = type.getName() + "|" + key;
        Object have = VALUES.get(id);
        if (have != null) return have;
        Object made = make(type, key, text.indexOf(':') >= 0 ? nameOf(key) : text.toUpperCase(Locale.ROOT));
        if (made != null) VALUES.putIfAbsent(id, made);
        return made;
    }

    /** A registry value's methods (K_ classes and KeyedHandler). */
    public static Object invoke(Value self, Method m, Object[] args) {
        String name = m.getName();
        int n = args == null ? 0 : args.length;
        NamespacedKey key = self.horizonKey();
        switch (name) {
            case "getKey":
            case "key":
            case "getKeyOrThrow":
                if (n == 0) return m.getReturnType() == Key.class ? Key.key(key.toString()) : key;
                break;
            case "name":
            case "getName":
                if (n == 0 && self instanceof org.bukkit.GameRule<?>) return key.getKey();
                if (n == 0) return self.horizonName();
                break;
            case "toString":
                if (n == 0) {
                    String paper = paperString(self, key);
                    return paper != null ? paper : self.horizonName();
                }
                break;
            case "hashCode":
                if (n == 0) return key.hashCode();
                break;
            case "equals":
                if (n == 1) return args[0] instanceof Value other && other.horizonKey().equals(key) && other.getClass() == self.getClass();
                break;
            case "ordinal":
                // OldEnum.ordinal: the value's place in its registry, as values() lists it (CraftCat.CraftType counts
                // them so); plugins index enum switch maps by it (Shopkeepers: values().length sized).
                if (n == 0) {
                    Integer at = ordinalOf(self, key);
                    return at != null ? at : Math.abs(key.hashCode() % 100000);
                }
                break;
            case "compareTo":
                if (n == 1 && args[0] instanceof Value other) return self.horizonName().compareTo(other.horizonName());
                break;
            case "translationKey":
            case "getTranslationKey":
                if (n == 0) return key.getNamespace() + "." + key.getKey().replace('/', '.');
                break;
            case "isRegistered":
                if (n == 0) return true;
                break;
            case "asMaterial":
                if (n == 0) return Material.matchMaterial(key.toString());
                break;
            case "getType":
                if (n == 0 && self instanceof org.bukkit.GameRule<?>) return INT_RULES.containsKey(key.getKey()) ? Integer.class : Boolean.class;
                break;
            case "isInstant":
                if (n == 0 && self instanceof org.bukkit.potion.PotionEffectType) return Set.of("instant_health", "instant_damage", "saturation").contains(key.getKey());
                break;
            case "getEffectCategory":
                if (n == 0 && self instanceof org.bukkit.potion.PotionEffectType) return HARMFUL.contains(key.getKey()) ? org.bukkit.potion.PotionEffectType.Category.HARMFUL : NEUTRAL.contains(key.getKey()) ? org.bukkit.potion.PotionEffectType.Category.NEUTRAL : org.bukkit.potion.PotionEffectType.Category.BENEFICIAL;
                break;
            case "getDefaultValue":
                if (n == 0 && self instanceof org.bukkit.GameRule<?>) return INT_RULES.containsKey(key.getKey()) ? INT_RULES.get(key.getKey()) : !FALSE_RULES.contains(key.getKey());
                break;
            case "createItemStack":
                if (n == 0) return Items.create(Material.matchMaterial(key.toString()), 1);
                if (n == 1 && args[0] instanceof Integer amount) return Items.create(Material.matchMaterial(key.toString()), amount);
                break;
            case "hasItemType":
                if (n == 0) return materialIs(key.toString(), "isItem");
                break;
            case "canEnchantItem":
                // CraftEnchantment: canEnchant(asNMSCopy(null)) is the empty stack's, false.
                if (n == 1 && self instanceof org.bukkit.enchantments.Enchantment e) {
                    if (!(args[0] instanceof org.bukkit.inventory.ItemStack s) || s.isEmpty()) return false;
                    // The game's Enchantment.canEnchant: the item is one of its supported items.
                    try {
                        return e.getSupportedItems().contains(io.papermc.paper.registry.TypedKey.create(io.papermc.paper.registry.RegistryKey.ITEM, s.getType().getKey()));
                    } catch (RuntimeException ignored) {
                        // then the server's own answer, below
                    }
                }
                break;
            case "hasBlockType":
                if (n == 0) return materialIs(key.toString(), "isBlock");
                break;
            case "getItemType":
                if (n == 0) return value(ItemType.class, key);
                break;
            case "getBlockType":
                if (n == 0) return value(BlockType.class, key);
                break;
            default:
                break;
        }
        boolean material = self instanceof ItemType || self instanceof BlockType;
        Object raw = Native.MISSING;
        if (material) {
            raw = Native.tryCall(Pack.of(Pack.MAT, key.toString()), name, Conv.args(args));
            // What the server has on its BlockType / ItemType and not on Material (BlockType.hasCollision).
            if (raw == Native.MISSING) {
                Object twin = Native.tryCall(Pack.of(Pack.MAT, key.toString()), self instanceof BlockType ? "asBlockType" : "asItemType", new Object[0]);
                if (twin instanceof H) raw = Native.tryCall(twin, name, Conv.args(args));
            }
        } else {
            for (Object twin : nativeTwins(self, key)) {
                raw = Native.tryCall(twin, name, Conv.args(args));
                if (raw != Native.MISSING) break;
            }
        }
        if (raw == Native.MISSING) {
            String where = m.getDeclaringClass().getSimpleName() + "." + name + "/" + n;
            NativeHandler.missing(where);
            // Which value: the game's own entries answer, a datapack's or plugin's may not.
            StringBuilder kinds = new StringBuilder();
            if (!material) for (Object twin : nativeTwins(self, key)) kinds.append(twin instanceof H th ? th.kind + "#" + th.key : String.valueOf(twin)).append(' ');
            throw Report.cant(where + " of " + key + " [twins: " + kinds.toString().trim() + " on " + Thread.currentThread().getName() + "]");
        }
        return Conv.fromNative(raw, m.getGenericReturnType());
    }

    /** Paper's CraftRegistryItem toString (`CraftEnchantment{holder=Reference{ResourceKey[minecraft:enchantment /
     *  minecraft:sharpness]=Enchantment Sharpness}}`) for the types whose game value prints the same each run (or
     *  only an address, which the game's Object.toString has too); null for the others (their own names). */
    /** Each registry value type's keys in its registry's order. */
    private static final Map<Class<?>, Map<NamespacedKey, Integer>> ORDINALS = new ConcurrentHashMap<>();

    static Integer ordinalOf(Object self, NamespacedKey key) {
        Class<?> type = null;
        for (Class<?> face : self.getClass().getInterfaces()) if (isRegistryType(face)) { type = face; break; }
        if (type == null && isRegistryType(self.getClass().getSuperclass())) type = self.getClass().getSuperclass();
        if (type == null) return null;
        Class<?> t = type;
        Map<NamespacedKey, Integer> order = ORDINALS.get(t);
        if (order == null) {
            Object regKey = KEYS_BY_TYPE.get(t);
            if (regKey == null) return null;
            // Paper numbers them in key order (measured: Cat.Type ALL_BLACK 0 .. TABBY 9, Profession ARMORER 0 ..
            // FARMER 4), not in values()' order.
            java.util.TreeSet<String> keys = new java.util.TreeSet<>();
            for (Object v : registry(regKey, t)) if (v instanceof Keyed k) keys.add(k.getKey().toString());
            Map<NamespacedKey, Integer> made = new java.util.HashMap<>();
            int i = 0;
            for (String k : keys) made.put(NamespacedKey.fromString(k), i++);
            if (made.isEmpty()) return null;
            order = made;
            ORDINALS.put(t, made);
        }
        return order.get(key);
    }

    static String paperString(Object self, NamespacedKey key) {
        String craft, registry, value;
        String hash = Integer.toHexString(System.identityHashCode(self));
        if (self instanceof org.bukkit.enchantments.Enchantment) {
            String english = Translations.english("enchantment." + key.getNamespace() + "." + key.getKey());
            craft = "CraftEnchantment"; registry = "enchantment"; value = "Enchantment " + (english != null ? english : key.getKey());
        } else if (self instanceof org.bukkit.potion.PotionEffectType) {
            craft = "CraftPotionEffectType"; registry = "mob_effect"; value = "net.minecraft.world.effect.MobEffect@" + hash;
        } else if (self instanceof org.bukkit.block.Biome && !key.getKey().equals("custom")) {
            craft = "CraftBiome"; registry = "worldgen/biome"; value = "net.minecraft.world.level.biome.Biome@" + hash;
        } else if (self instanceof org.bukkit.attribute.Attribute) {
            craft = "CraftAttribute"; registry = "attribute"; value = "net.minecraft.world.entity.ai.attributes.RangedAttribute@" + hash;
        } else if (self instanceof org.bukkit.Sound) {
            craft = "CraftSound"; registry = "sound_event"; value = "SoundEvent[location=" + key + ", fixedRange=Optional.empty]";
        } else if (self instanceof ItemType) {
            craft = "CraftItemType"; registry = "item"; value = key.toString();
        } else if (self instanceof BlockType) {
            craft = "CraftBlockType"; registry = "block"; value = "Block{" + key + "}";
        } else if (self instanceof org.bukkit.GameRule<?>) {
            craft = "CraftGameRule"; registry = "game_rule"; value = key.getKey();
        } else if (self instanceof org.bukkit.Fluid) {
            String k = key.getKey();
            String cls = k.equals("water") ? "WaterFluid$Source" : k.equals("flowing_water") ? "WaterFluid$Flowing" : k.equals("lava") ? "LavaFluid$Source" : k.equals("flowing_lava") ? "LavaFluid$Flowing" : "EmptyFluid";
            craft = "CraftFluid"; registry = "fluid"; value = "net.minecraft.world.level.material." + cls + "@" + hash;
        } else {
            return null;
        }
        return craft + "{holder=Reference{ResourceKey[minecraft:" + registry + " / " + key + "]=" + value + "}}";
    }

    private static final Map<String, Object> TWINS = new java.util.concurrent.ConcurrentHashMap<>();
    private static final Object NO_TWIN = new Object();

    /**
     * The server's own object for a registry value (an SDK Enchantment, PotionEffectType...), found once by its key
     * through the type's static lookup (getByKey, get, of...): the value's other methods go to it.
     */
    private static final Map<String, List<Object>> TWIN_LISTS = new java.util.concurrent.ConcurrentHashMap<>();

    /** Every server object the type's lookups give for this key (the SDK can have two kinds of one value). */
    static List<Object> nativeTwins(Object self, NamespacedKey key) {
        Class<?> type = self.getClass();
        for (Class<?> face : type.getInterfaces()) if (face != Value.class && Keyed.class.isAssignableFrom(face)) { type = face; break; }
        if (!type.isInterface()) type = type.getSuperclass();
        String id = type.getSimpleName() + "|" + key;
        List<Object> have = TWIN_LISTS.get(id);
        if (have != null) return have;
        List<Object> found = new ArrayList<>();
        Set<String> kinds = new HashSet<>();
        for (String lookup : new String[]{"getByKey", "get", "of", "fromKey", "byKey", "getByName"}) {
            for (Object arg : new Object[]{key.toString(), Pack.of(Pack.KEY, key.toString()), key.getKey(), key.getKey().toUpperCase(Locale.ROOT)}) {
                Object raw = Native.tryStatic(type.getSimpleName(), lookup, new Object[]{arg});
                // A twin is a server object: a lookup that answers with text (a name) isn't one.
                if (raw instanceof H) {
                    String kind = raw instanceof H h ? String.valueOf(h.kind) : raw.getClass().getName();
                    if (kinds.add(kind)) found.add(raw);
                    break;
                }
            }
        }
        // Some kinds are reached only by listing them (PotionEffectType.values()): the one with this key.
        Object all = Native.tryStatic(type.getSimpleName(), "values", new Object[0]);
        if (all != Native.MISSING && all instanceof Pack p && p.items != null) {
            for (Object item : p.items) {
                Object k = Native.tryCall(item, "getKey", new Object[0]);
                if (k == Native.MISSING) k = Native.tryCall(item, "key", new Object[0]);
                Object text = k instanceof Pack kp ? kp.s : k;
                if (text != null && (key.toString().equals(String.valueOf(text)) || key.getKey().equals(String.valueOf(text)))) {
                    String kind = item instanceof H h ? String.valueOf(h.kind) : item.getClass().getName();
                    if (kinds.add(kind)) found.add(item);
                    break;
                }
            }
        }
        // Kept only once found: asked before the server can answer (a plugin's onLoad), nothing yet is not the answer.
        if (!found.isEmpty()) TWIN_LISTS.put(id, found);
        else {
            Object probe = Native.tryStatic(type.getSimpleName(), "getByKey", new Object[]{key.toString()});
            Logs.warning("[JavaLayer] no server twin for " + id + " on " + Thread.currentThread() + ": getByKey -> " + (probe == Native.MISSING ? "MISSING" : String.valueOf(probe)));
        }
        return found;
    }

    static Object nativeTwin(Object self, NamespacedKey key) {
        Class<?> type = self.getClass();
        for (Class<?> face : type.getInterfaces()) if (face != Value.class && Keyed.class.isAssignableFrom(face)) { type = face; break; }
        if (!type.isInterface()) type = type.getSuperclass();
        String id = type.getSimpleName() + "|" + key;
        Object have = TWINS.get(id);
        if (have != null) return have == NO_TWIN ? null : have;
        Object found = null;
        for (String lookup : new String[]{"getByKey", "get", "of", "fromKey", "byKey"}) {
            for (Object arg : new Object[]{Pack.of(Pack.KEY, key.toString()), key.toString(), key.getKey()}) {
                Object raw = Native.tryStatic(type.getSimpleName(), lookup, new Object[]{arg});
                if (raw != Native.MISSING && raw != null) { found = raw; break; }
            }
            if (found != null) break;
        }
        // Not kept when not found: asked too early (onLoad), it may be there later.
        if (found != null) TWINS.put(id, found);
        return found;
    }

    /** An interface registry value (Sound, Biome, Attribute, ItemType...). */
    static final class KeyedHandler implements InvocationHandler {
        final Class<?> type;
        final NamespacedKey key;
        final String name;

        KeyedHandler(Class<?> type, NamespacedKey key, String name) {
            this.type = type;
            this.key = key;
            this.name = name;
        }

        @Override
        public Object invoke(Object proxy, Method m, Object[] args) throws Throwable {
            if (m.getDeclaringClass() == Value.class) return m.getName().equals("horizonKey") ? key : name;
            Value self = (Value) proxy;
            try {
                return Registries.invoke(self, m, args);
            } catch (UnsupportedOperationException e) {
                if (m.isDefault()) return InvocationHandler.invokeDefault(proxy, m, args);
                throw e;
            }
        }
    }

    /** One registry: values made from keys; items and blocks listed from Material. */
    static final class RegistryHandler implements InvocationHandler {
        final Class<?> type;

        RegistryHandler(Class<?> type) { this.type = type; }

        List<Object> all() {
            List<Object> out = new ArrayList<>();
            if (type == ItemType.class || type == BlockType.class) {
                for (Material m : Material.values()) {
                    if (m.isLegacy()) continue;
                    Object v = value(type, m.getKey());
                    if (v != null) out.add(v);
                }
            } else {
                // Every key the server has for this registry (its Paper key's path: enchantment, sound_event...).
                RegistryKey<?> rk = keyOf(type);
                if (rk != null) {
                    List<String> keys = KEY_LISTS.computeIfAbsent(rk.key().value(), path -> {
                        List<String> list = new ArrayList<>();
                        try {
                            String json = Native.query("{\"op\":\"paper_registry\",\"registry\":\"" + path + "\"}");
                            com.google.gson.JsonElement e = com.google.gson.JsonParser.parseString(json);
                            if (e.isJsonArray()) for (com.google.gson.JsonElement k : e.getAsJsonArray()) list.add(k.getAsString());
                        } catch (RuntimeException | LinkageError ignored) {}
                        return list;
                    });
                    for (String k : keys) {
                        Object v = value(type, NamespacedKey.fromString(k));
                        if (v != null) out.add(v);
                    }
                }
                if (out.isEmpty()) for (Map.Entry<String, Object> e : VALUES.entrySet()) if (e.getKey().startsWith(type.getName() + "|")) out.add(e.getValue());
            }
            return out;
        }

        @Override
        public Object invoke(Object proxy, Method m, Object[] args) throws Throwable {
            String name = m.getName();
            int n = args == null ? 0 : args.length;
            switch (name) {
                case "get":
                    if (n == 1 && args[0] instanceof NamespacedKey k) return value(type, k);
                    if (n == 1 && args[0] instanceof Key k) return value(type, NamespacedKey.fromString(k.asString()));
                    break;
                case "getKey":
                    if (n == 1 && args[0] instanceof Value v) return v.horizonKey();
                    if (n == 1 && args[0] instanceof Keyed k) return k.getKey();
                    break;
                // Paper's registry tags: the game's tags of this registry's folder (RegistryTags).
                case "hasTag":
                    if (n == 1 && args[0] instanceof io.papermc.paper.registry.tag.TagKey<?> tk) return RegistryTags.has(tk);
                    return false;
                case "getTags": {
                    RegistryKey<?> rk = keyOf(type);
                    return rk == null ? List.of() : RegistryTags.all(rk);
                }
                case "getTag":
                    if (n == 1 && args[0] instanceof io.papermc.paper.registry.tag.TagKey tk) return RegistryTags.get(tk);
                    throw new NoSuchElementException("No tag " + (n == 1 ? args[0] : "") + " found");
                case "stream":
                    return all().stream();
                case "keyStream":
                    return all().stream().map(v -> ((Value) v).horizonKey());
                case "iterator":
                    return all().iterator();
                case "size":
                    return all().size();
                case "equals":
                    return proxy == args[0];
                case "hashCode":
                    return System.identityHashCode(proxy);
                case "toString":
                    return "Registry<" + type.getSimpleName() + ">";
                default:
                    break;
            }
            if (m.isDefault()) return InvocationHandler.invokeDefault(proxy, m, args);
            throw new UnsupportedOperationException("Registry." + name);
        }
    }
}
