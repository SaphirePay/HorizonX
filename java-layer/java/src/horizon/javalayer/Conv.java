package horizon.javalayer;

import net.kyori.adventure.key.Key;
import net.kyori.adventure.text.Component;
import net.kyori.adventure.text.ComponentLike;
import net.kyori.adventure.text.serializer.gson.GsonComponentSerializer;
import net.kyori.adventure.text.serializer.legacy.LegacyComponentSerializer;
import org.bukkit.Keyed;
import org.bukkit.Location;
import org.bukkit.Material;
import org.bukkit.NamespacedKey;
import org.bukkit.World;
import org.bukkit.inventory.ItemStack;
import org.bukkit.permissions.Permission;
import org.bukkit.plugin.Plugin;
import org.bukkit.util.Vector;

import java.lang.reflect.Array;
import java.lang.reflect.GenericArrayType;
import java.lang.reflect.ParameterizedType;
import java.lang.reflect.Type;
import java.lang.reflect.TypeVariable;
import java.lang.reflect.WildcardType;
import java.util.*;
import java.util.concurrent.ConcurrentHashMap;
import java.util.concurrent.atomic.AtomicLong;

/** Values between Java and the server: what a Bukkit method takes goes out, what it returns comes back typed. */
public final class Conv {
    private Conv() {}

    // ---- Java objects the server keeps (a plugin's InventoryHolder) ----
    private static final Map<Long, Object> REFS = new ConcurrentHashMap<>();
    private static final Map<Object, Long> REF_IDS = Collections.synchronizedMap(new IdentityHashMap<>());
    private static final AtomicLong NEXT_REF = new AtomicLong(1);

    static long ref(Object o) {
        synchronized (REF_IDS) {
            Long id = REF_IDS.get(o);
            if (id == null) {
                id = NEXT_REF.getAndIncrement();
                REF_IDS.put(o, id);
                REFS.put(id, o);
            }
            return id;
        }
    }

    static Object refObject(long id) { return REFS.get(id); }

    /** The same arguments with keys as text, or null when none is a key (for the server's text-taking forms). */
    public static Object[] keysAsText(Object[] nargs) {
        if (nargs == null) return null;
        Object[] out = null;
        for (int i = 0; i < nargs.length; i++) {
            if (nargs[i] instanceof Pack p && p.tag == Pack.KEY) {
                if (out == null) out = nargs.clone();
                out[i] = p.s;
            }
        }
        return out;
    }

    public static Object[] args(Object[] a) {
        if (a == null || a.length == 0) return new Object[0];
        Object[] out = new Object[a.length];
        for (int i = 0; i < a.length; i++) out[i] = toNative(a[i]);
        return out;
    }

    private static Object damageSource(org.bukkit.damage.DamageSource ds) {
        Object builder = Native.tryStatic("DamageSource", "builder", new Object[]{ds.getDamageType().getKey().toString()});
        if (builder == null || builder == Native.MISSING) return null;
        // The server's builder is the source itself; each `with` gives the next one.
        Object source = builder;
        org.bukkit.entity.Entity causing = ds.getCausingEntity(), direct = ds.getDirectEntity();
        if (causing != null) source = next(source, "withCausingEntity", causing);
        if (direct != null && direct != causing) source = next(source, "withDirectEntity", direct);
        if (ds.getDamageLocation() != null) source = next(source, "withDamageLocation", ds.getDamageLocation());
        return source;
    }

    private static Object next(Object source, String with, Object value) {
        Object made = Native.tryCall(source, with, new Object[]{toNative(value)});
        return made instanceof H ? made : source;
    }

    public static Object toNative(Object o) {
        if (o == null || o instanceof String || o instanceof Integer || o instanceof Long || o instanceof Double || o instanceof Boolean || o instanceof Character) return o;
        if (o instanceof H) return o;
        if (o instanceof Float f) return f.doubleValue();
        if (o instanceof Short || o instanceof Byte) return ((Number) o).intValue();
        if (o instanceof CharSequence cs) return cs.toString();
        // Scoreboard criteria: the server takes them by name.
        if (o instanceof org.bukkit.scoreboard.Criteria c) return c.getName();
        H h = Proxies.handleOf(o);
        if (h != null) return h;
        if (o instanceof ItemStack stack) return Items.toNative(stack);
        // A banner pattern: the server's own (BannerMeta.addPattern/setPatterns take those: EliteMobs' banners), else
        // as the game stores one: {"pattern": "minecraft:stripe_top", "color": "red"}.
        if (o instanceof org.bukkit.block.banner.Pattern bp) {
            Object made = Native.construct("BannerPattern", new Object[]{bp.getPattern().getKey().toString(), bp.getColor().name().toLowerCase(Locale.ROOT)});
            if (made != null && made != Native.MISSING) return made;
            Map<String, Object> m = new java.util.LinkedHashMap<>();
            m.put("pattern", bp.getPattern().getKey().toString());
            m.put("color", bp.getColor().name().toLowerCase(Locale.ROOT));
            return toNative(m);
        }
        // A damage source built in Java (DamageSource.builder): the server's own, with the same type, entities and place.
        if (o instanceof org.bukkit.damage.DamageSource ds) {
            Object made = damageSource(ds);
            if (made != null) return made;
        }
        if (o instanceof Material m) return Pack.of(Pack.MAT, m.getKey().toString());
        // Values the server takes as text or builds itself.
        if (o instanceof org.bukkit.scoreboard.Criteria c) return c.getName();
        if (o instanceof java.net.URL || o instanceof java.net.URI) return o.toString();
        if (o instanceof org.bukkit.attribute.AttributeModifier am) {
            Object made = Native.construct("AttributeModifier", new Object[]{am.getKey().toString(), am.getAmount(), am.getOperation().name()});
            if (made != null && made != Native.MISSING) return made;
        }
        // A firework effect built in Java (FireworkEffect.builder(): MySpawn's, crates'): the server's own from its
        // saved form, with vanilla's shape names (Bukkit's BALL is small_ball, BALL_LARGE large_ball).
        if (o instanceof org.bukkit.FireworkEffect fe) {
            Map<String, Object> m = new java.util.LinkedHashMap<>();
            m.put("type", switch (fe.getType()) {
                case BALL -> "small_ball";
                case BALL_LARGE -> "large_ball";
                default -> fe.getType().name().toLowerCase(Locale.ROOT);
            });
            m.put("colors", fe.getColors().stream().map(org.bukkit.Color::serialize).toList());
            m.put("fade-colors", fe.getFadeColors().stream().map(org.bukkit.Color::serialize).toList());
            m.put("flicker", fe.hasFlicker());
            m.put("trail", fe.hasTrail());
            Object made = Native.tryStatic("FireworkEffect", "deserialize", new Object[]{toNative(m)});
            if (made != null && made != Native.MISSING) return made;
        }
        if (o instanceof com.destroystokyo.paper.entity.ai.GoalKey<?> gk) {
            Object made = Native.tryStatic("GoalKey", "of", new Object[]{gk.getEntityClass().getName(), toNative(gk.getNamespacedKey())});
            if (made != null && made != Native.MISSING) return made;
        }
        if (o instanceof org.bukkit.attribute.Attribute a && !(o instanceof H)) {
            try { return a.getKey().toString(); } catch (RuntimeException ignored) {}
        }
        if (o instanceof Enum<?> e) return e.name();
        if (Registries.isKeyedValue(o)) return Registries.nativeName(o);
        if (o instanceof UUID u) return u.toString();
        if (o instanceof NamespacedKey k) return Pack.of(Pack.KEY, k.toString());
        if (o instanceof Key k) return Pack.of(Pack.KEY, k.asString());
        if (o instanceof ComponentLike c) return Pack.of(Pack.JSON, GsonComponentSerializer.gson().serialize(c.asComponent()));
        if (o instanceof Location l) {
            World w = null;
            try { w = l.getWorld(); } catch (IllegalArgumentException ignored) {}
            return new Pack(Pack.LOC, w == null ? null : toNative(w), l.getX(), l.getY(), l.getZ(), l.getYaw(), l.getPitch(), null, null, null, 0);
        }
        if (o instanceof Vector v) return new Pack(Pack.VEC, null, v.getX(), v.getY(), v.getZ(), 0, 0, null, null, null, 0);
        if (o instanceof Permission p) return p.getName();
        if (o instanceof Plugin p) return p.getName();
        if (o instanceof Map<?, ?> m) {
            Object[] keys = new Object[m.size()], vals = new Object[m.size()];
            int i = 0;
            for (Map.Entry<?, ?> e : m.entrySet()) {
                keys[i] = toNative(e.getKey());
                vals[i] = toNative(e.getValue());
                i++;
            }
            return new Pack(Pack.MAP, null, 0, 0, 0, 0, 0, null, keys, vals, 0);
        }
        if (o instanceof Collection<?> c) {
            Object[] items = new Object[c.size()];
            int i = 0;
            for (Object x : c) items[i++] = toNative(x);
            return Pack.items(o instanceof Set ? Pack.SET : Pack.LIST, items);
        }
        if (o instanceof Object[] arr) {
            Object[] items = new Object[arr.length];
            for (int i = 0; i < arr.length; i++) items[i] = toNative(arr[i]);
            return Pack.items(Pack.ARR, items);
        }
        if (o.getClass().isArray()) {
            int n = Array.getLength(o);
            Object[] items = new Object[n];
            for (int i = 0; i < n; i++) items[i] = toNative(Array.get(o, i));
            return Pack.items(Pack.ARR, items);
        }
        if (o instanceof org.bukkit.Color c) return c.asRGB();
        Object value = Values.toNative(o);
        if (value != Values.NOT_A_VALUE) return value;
        if (o instanceof Iterable<?> it) {
            List<Object> items = new ArrayList<>();
            for (Object x : it) items.add(toNative(x));
            return Pack.items(Pack.LIST, items.toArray());
        }
        return new Pack(Pack.REF, null, 0, 0, 0, 0, 0, null, null, null, ref(o));
    }

    // ---- the server's answers, typed ----

    public static Class<?> rawClass(Type type) {
        if (type instanceof Class<?> c) return c;
        if (type instanceof ParameterizedType p) return rawClass(p.getRawType());
        if (type instanceof GenericArrayType g) return Array.newInstance(rawClass(g.getGenericComponentType()), 0).getClass();
        if (type instanceof WildcardType w) return rawClass(w.getUpperBounds().length > 0 ? w.getUpperBounds()[0] : Object.class);
        if (type instanceof TypeVariable<?> v) return rawClass(v.getBounds().length > 0 ? v.getBounds()[0] : Object.class);
        return Object.class;
    }

    static Type typeArg(Type type, int index) {
        if (type instanceof ParameterizedType p && p.getActualTypeArguments().length > index) {
            Type arg = p.getActualTypeArguments()[index];
            if (arg instanceof WildcardType w) return w.getUpperBounds().length > 0 ? w.getUpperBounds()[0] : Object.class;
            return arg;
        }
        return Object.class;
    }

    static Object zero(Class<?> c) {
        if (!c.isPrimitive()) return null;
        if (c == boolean.class) return false;
        if (c == char.class) return '\0';
        if (c == byte.class) return (byte) 0;
        if (c == short.class) return (short) 0;
        if (c == int.class) return 0;
        if (c == long.class) return 0L;
        if (c == float.class) return 0f;
        if (c == double.class) return 0d;
        return null;
    }

    public static Object fromNative(Object raw, Type type) {
        Class<?> cls = rawClass(type);
        if (cls == void.class || cls == Void.class) return null;
        if (raw == null) return zero(cls);
        // Paper's GoalKey is a final class of its own: made again from the server's (entity class name, key).
        if (raw instanceof H h && cls == com.destroystokyo.paper.entity.ai.GoalKey.class) return goalKey(h);
        // A registry key set (Enchantment.getSupportedItems...): the server gives the keys, Paper a RegistryKeySet.
        if (io.papermc.paper.registry.set.RegistryKeySet.class.isAssignableFrom(cls) || cls == io.papermc.paper.registry.set.RegistrySet.class) {
            Object set = keySet(raw, type);
            if (set != null) return set;
        }
        // The server's recipes as Paper's classes (ShapedRecipe, FurnaceRecipe... all Keyed), from their JSON (GameRecipes).
        if (raw instanceof H h && org.bukkit.inventory.Recipe.class.isAssignableFrom(cls)) {
            Object recipe = GameRecipes.of(h);
            if (recipe != null && cls.isInstance(recipe)) return recipe;
        }
        // Bukkit's AttributeModifier is a class: made from the server's (key, amount, operation, slot group); a proxy
        // can't be one, so the multimap of ItemMeta.getAttributeModifiers() held nulls (EliteMobs).
        if (raw instanceof H h && cls == org.bukkit.attribute.AttributeModifier.class) return attributeModifier(h);
        if (raw instanceof H h) return fromHandle(h, type, cls);
        if (raw instanceof Pack p) {
            Object made = fromPack(p, type, cls);
            // A JSON object that stands for an API value (a banner Pattern): made into it.
            return made instanceof Map<?, ?> && !Map.class.isAssignableFrom(cls) ? coerce(made, cls) : made;
        }
        Object out = coerce(raw, cls);
        // The SDK answers `true` where Bukkit returns an object (`openInventory` -> InventoryView): no object then.
        if (out != null && !cls.isPrimitive() && cls != Object.class && !cls.isInstance(out)) return null;
        return out;
    }

    @SuppressWarnings({"unchecked", "rawtypes"})
    /** Each registry's RegistryKey by the class of its values (RegistryKey<ItemType> ITEM...), from RegistryKey's own fields. */
    private static volatile Map<Class<?>, io.papermc.paper.registry.RegistryKey<?>> REGISTRY_KEYS;

    static io.papermc.paper.registry.RegistryKey<?> registryKeyFor(Class<?> values) {
        Map<Class<?>, io.papermc.paper.registry.RegistryKey<?>> all = REGISTRY_KEYS;
        if (all == null) {
            all = new HashMap<>();
            for (java.lang.reflect.Field f : io.papermc.paper.registry.RegistryKey.class.getFields()) {
                if (!java.lang.reflect.Modifier.isStatic(f.getModifiers()) || !(f.getGenericType() instanceof ParameterizedType p)) continue;
                if (!(p.getActualTypeArguments()[0] instanceof Class<?> c)) continue;
                try {
                    all.putIfAbsent(c, (io.papermc.paper.registry.RegistryKey<?>) f.get(null));
                } catch (IllegalAccessException ignored) {
                }
            }
            REGISTRY_KEYS = all;
        }
        return all.get(values);
    }

    @SuppressWarnings({"unchecked", "rawtypes"})
    static Object keySet(Object raw, Type type) {
        if (!(type instanceof ParameterizedType p) || !(p.getActualTypeArguments()[0] instanceof Class<?> values)) return null;
        io.papermc.paper.registry.RegistryKey key = registryKeyFor(values);
        if (key == null) return null;
        List<Object> items = raw instanceof Pack pk && pk.items != null ? java.util.Arrays.asList(pk.items) : raw instanceof List<?> l ? (List<Object>) l : null;
        if (items == null) return null;
        List<io.papermc.paper.registry.TypedKey> keys = new ArrayList<>();
        for (Object item : items) {
            Object text = item instanceof Pack ip ? ip.s : item;
            if (text == null) continue;
            keys.add(io.papermc.paper.registry.TypedKey.create(key, net.kyori.adventure.key.Key.key(String.valueOf(text))));
        }
        return io.papermc.paper.registry.set.RegistrySet.keySet(key, (Iterable) keys);
    }

    static org.bukkit.attribute.AttributeModifier attributeModifier(H h) {
        Object key = Native.tryCall(h, "getKey", new Object[0]);
        Object amount = Native.tryCall(h, "getAmount", new Object[0]);
        Object op = Native.tryCall(h, "getOperation", new Object[0]);
        Object slot = Native.tryCall(h, "getSlotGroup", new Object[0]);
        if (key == Native.MISSING || key == null || !(amount instanceof Number n)) return null;
        org.bukkit.NamespacedKey k = org.bukkit.NamespacedKey.fromString(String.valueOf(key).toLowerCase(java.util.Locale.ROOT));
        if (k == null) return null;
        org.bukkit.attribute.AttributeModifier.Operation operation = switch (String.valueOf(op)) {
            case "ADD_SCALAR", "add_multiplied_base" -> org.bukkit.attribute.AttributeModifier.Operation.ADD_SCALAR;
            case "MULTIPLY_SCALAR_1", "add_multiplied_total" -> org.bukkit.attribute.AttributeModifier.Operation.MULTIPLY_SCALAR_1;
            default -> org.bukkit.attribute.AttributeModifier.Operation.ADD_NUMBER;
        };
        org.bukkit.inventory.EquipmentSlotGroup group = slot instanceof String sg ? org.bukkit.inventory.EquipmentSlotGroup.getByName(sg) : null;
        return new org.bukkit.attribute.AttributeModifier(k, n.doubleValue(), operation, group == null ? org.bukkit.inventory.EquipmentSlotGroup.ANY : group);
    }

    static Object goalKey(H h) {
        String name = String.valueOf(Native.tryCall(h, "getEntityClass", new Object[0]));
        Class<?> c = org.bukkit.entity.Mob.class;
        for (String tryName : new String[]{name, "org.bukkit.entity." + name}) {
            try {
                Class<?> found = Class.forName(tryName, false, Conv.class.getClassLoader());
                if (org.bukkit.entity.Mob.class.isAssignableFrom(found)) { c = found; break; }
            } catch (ClassNotFoundException ignored) {
            }
        }
        NamespacedKey key = (NamespacedKey) fromNative(Native.tryCall(h, "getNamespacedKey", new Object[0]), NamespacedKey.class);
        return com.destroystokyo.paper.entity.ai.GoalKey.of((Class) c, key);
    }

    @SuppressWarnings({"unchecked", "rawtypes"})
    static Object enumOf(Class<?> cls, String text) {
        String name = text;
        int colon = name.indexOf(':');
        if (colon >= 0) name = name.substring(colon + 1);
        name = name.replace('.', '_').replace('/', '_').toUpperCase(Locale.ROOT);
        try {
            return Enum.valueOf((Class<Enum>) cls, name);
        } catch (IllegalArgumentException e) {
            for (Object c : cls.getEnumConstants()) if (((Enum<?>) c).name().equalsIgnoreCase(text)) return c;
            return null;
        }
    }

    static Object coerce(Object raw, Class<?> cls) {
        if (cls == Object.class || (cls.isInstance(raw) && !cls.isPrimitive())) return raw;
        // A banner pattern as the game stores one ({"pattern": "minecraft:stripe_top", "color": "red"}).
        if (cls == org.bukkit.block.banner.Pattern.class && raw instanceof Map<?, ?> m) {
            NamespacedKey key = NamespacedKey.fromString(String.valueOf(m.get("pattern")));
            org.bukkit.block.banner.PatternType type = key == null ? null : (org.bukkit.block.banner.PatternType) Registries.value(org.bukkit.block.banner.PatternType.class, key);
            org.bukkit.DyeColor color = (org.bukkit.DyeColor) enumOf(org.bukkit.DyeColor.class, String.valueOf(m.get("color")));
            if (type != null && color != null) return new org.bukkit.block.banner.Pattern(color, type);
        }
        // Files come as their path (World.getWorldFolder, getWorldPath).
        if (raw instanceof String path && cls == java.io.File.class) return new java.io.File(path);
        if (raw instanceof String path && cls == java.nio.file.Path.class) return java.nio.file.Path.of(path);
        if (raw instanceof Number n) {
            if (cls == int.class || cls == Integer.class) return n.intValue();
            if (cls == long.class || cls == Long.class) return n.longValue();
            if (cls == double.class || cls == Double.class) return n.doubleValue();
            if (cls == float.class || cls == Float.class) return n.floatValue();
            if (cls == short.class || cls == Short.class) return n.shortValue();
            if (cls == byte.class || cls == Byte.class) return n.byteValue();
            if (cls == boolean.class || cls == Boolean.class) return n.intValue() != 0;
            if (cls == char.class || cls == Character.class) return (char) n.intValue();
            if (cls == String.class) return n.toString();
            if (cls == org.bukkit.Color.class) return org.bukkit.Color.fromARGB(n.intValue());
            if (cls == OptionalInt.class) return OptionalInt.of(n.intValue());
            if (cls == Number.class) return n;
        }
        if (raw instanceof Boolean b) {
            if (cls == boolean.class || cls == Boolean.class) return b;
            if (cls == String.class) return b.toString();
        }
        if (raw instanceof Character ch) {
            if (cls == char.class || cls == Character.class) return ch;
            if (cls == String.class) return ch.toString();
        }
        if (raw instanceof String s) {
            if (cls == String.class || cls == CharSequence.class) return s;
            if (cls == java.net.InetSocketAddress.class || cls == java.net.SocketAddress.class) {
                int colon = s.lastIndexOf(':');
                try {
                    String host = colon > 0 ? s.substring(0, colon) : s;
                    int port = colon > 0 ? Integer.parseInt(s.substring(colon + 1)) : 0;
                    return new java.net.InetSocketAddress(java.net.InetAddress.getByName(host.replace("[", "").replace("]", "")), port);
                } catch (Exception e) {
                    return null;
                }
            }
            if (cls == java.net.InetAddress.class) {
                try {
                    return java.net.InetAddress.getByName(s);
                } catch (Exception e) {
                    return null;
                }
            }
            if (cls == UUID.class) {
                try { return UUID.fromString(s); } catch (IllegalArgumentException e) { return null; }
            }
            // A block state as text (`minecraft:sand`, `minecraft:oak_stairs[half=top]`: an event's new block, the
            // SDK's getBlockData): the game's state, every property set.
            if (cls == org.bukkit.block.data.BlockData.class && !s.isEmpty()) {
                try {
                    return org.bukkit.Bukkit.createBlockData(s);
                } catch (IllegalArgumentException e) {
                    return null;
                }
            }
            // PlayerTextures.getSkin/getCape: the SDK keeps the URL as text.
            if (cls == java.net.URL.class) {
                try { return java.net.URI.create(s).toURL(); } catch (Exception e) { return null; }
            }
            if (cls == java.net.URI.class) {
                try { return java.net.URI.create(s); } catch (Exception e) { return null; }
            }
            if (cls == Material.class) return Material.matchMaterial(s);
            if (cls.isEnum()) return enumOf(cls, s);
            if (Component.class.isAssignableFrom(cls) || cls == ComponentLike.class) return LegacyComponentSerializer.legacySection().deserialize(s);
            if (cls == NamespacedKey.class) return NamespacedKey.fromString(s.toLowerCase(Locale.ROOT));
            if (cls == Key.class) return Key.key(s.toLowerCase(Locale.ROOT));
            if (Keyed.class.isAssignableFrom(cls) || Registries.isRegistryType(cls)) {
                Object found = Registries.byName(cls, s);
                if (found != null) return found;
            }
            try {
                if (cls == int.class || cls == Integer.class) return Integer.parseInt(s.trim());
                if (cls == long.class || cls == Long.class) return Long.parseLong(s.trim());
                if (cls == double.class || cls == Double.class) return Double.parseDouble(s.trim());
                if (cls == float.class || cls == Float.class) return Float.parseFloat(s.trim());
                if (cls == boolean.class || cls == Boolean.class) return Boolean.parseBoolean(s.trim());
            } catch (NumberFormatException e) {
                return zero(cls);
            }
            if ((cls == char.class || cls == Character.class) && !s.isEmpty()) return s.charAt(0);
            if (cls == Optional.class) return Optional.of(s);
        }
        if (cls == Optional.class) return Optional.ofNullable(raw);
        return cls.isPrimitive() ? zero(cls) : raw;
    }

    static Object fromHandle(H h, Type type, Class<?> cls) {
        if (cls == Optional.class) return Optional.ofNullable(fromHandle(h, typeArg(type, 0), rawClass(typeArg(type, 0))));
        boolean stackKind = h.kind.equals("org.bukkit.inventory.ItemStack");
        if (stackKind && (cls == Object.class || cls.isAssignableFrom(ItemStack.class))) return Items.wrap(h);
        if (cls == String.class) {
            Object text = Native.tryCall(h, "toString", new Object[0]);
            Native.release(h.id);
            return text == Native.MISSING ? h.kind : String.valueOf(text);
        }
        // A Java class that stands for a server object (ProtocolLib's PacketContainer...): its (H) constructor.
        if (!cls.isInterface() && Proxies.Handled.class.isAssignableFrom(cls)) {
            try {
                java.lang.reflect.Constructor<?> k = cls.getDeclaredConstructor(H.class);
                k.setAccessible(true);
                return k.newInstance(h);
            } catch (ReflectiveOperationException e) {
                throw new IllegalStateException("can't make " + cls.getName() + " from " + h, e);
            }
        }
        if (cls.isEnum()) {
            Object name = Native.tryCall(h, "name", new Object[0]);
            if (name == Native.MISSING || name == null) name = Native.tryCall(h, "toString", new Object[0]);
            Native.release(h.id);
            return name == Native.MISSING || name == null ? null : enumOf(cls, String.valueOf(name));
        }
        if (!cls.isInterface() && cls != Object.class) {
            Object value = Values.fromNative(h, cls);
            if (value != Values.NOT_A_VALUE) return value;
        }
        // A plugin of the server's own (a Rust one, in its PluginEnableEvent): a Plugin with its description.
        if (cls == org.bukkit.plugin.Plugin.class || (h.kind.isEmpty() && org.bukkit.plugin.Plugin.class.isAssignableFrom(cls))) return Builtins.serverPlugin(h);
        return Proxies.make(h, cls);
    }

    @SuppressWarnings({"unchecked", "rawtypes"})
    static Object fromPack(Pack p, Type type, Class<?> cls) {
        switch (p.tag) {
            case Pack.LOC: {
                World world = p.o == null ? null : (World) fromNative(p.o, World.class);
                return new Location(world, p.x, p.y, p.z, p.f1, p.f2);
            }
            case Pack.VEC:
                return new Vector(p.x, p.y, p.z);
            case Pack.KEY:
                if (cls == Key.class) return Key.key(p.s);
                if (cls == String.class) return p.s;
                return NamespacedKey.fromString(p.s);
            case Pack.JSON: {
                Component c = GsonComponentSerializer.gson().deserialize(p.s);
                if (cls == String.class) return LegacyComponentSerializer.legacySection().serialize(c);
                return c;
            }
            case Pack.MAT: {
                Material m = Material.matchMaterial(p.s);
                if (cls == String.class) return m == null ? p.s : m.name();
                if (cls == org.bukkit.inventory.ItemType.class) return m == null ? null : m.asItemType();
                if (cls == org.bukkit.block.BlockType.class) return m == null ? null : m.asBlockType();
                return m;
            }
            case Pack.REF:
                return REFS.get(p.id);
            case Pack.MAP: {
                Type kt = typeArg(type, 0), vt = typeArg(type, 1);
                // A Multimap (ItemMeta.getAttributeModifiers): each key with its value, or each of a list of them.
                if (com.google.common.collect.Multimap.class.isAssignableFrom(cls)) {
                    com.google.common.collect.Multimap<Object, Object> mm = com.google.common.collect.LinkedHashMultimap.create();
                    for (int i = 0; i < p.items.length; i++) {
                        Object k = fromNative(p.items[i], kt);
                        Object v = p.vals[i];
                        if (v instanceof Pack vp && (vp.tag == Pack.LIST || vp.tag == Pack.ARR || vp.tag == Pack.SET) && vp.items != null) for (Object x : vp.items) mm.put(k, fromNative(x, vt));
                        else mm.put(k, fromNative(v, vt));
                    }
                    return mm;
                }
                Map<Object, Object> out = new LinkedHashMap<>();
                for (int i = 0; i < p.items.length; i++) out.put(fromNative(p.items[i], kt), fromNative(p.vals[i], vt));
                return out;
            }
            default: {
                Object[] items = p.items == null ? new Object[0] : p.items;
                if (cls.isArray()) {
                    Class<?> component = cls.getComponentType();
                    Type componentType = type instanceof GenericArrayType g ? g.getGenericComponentType() : component;
                    Object out = Array.newInstance(component, items.length);
                    for (int i = 0; i < items.length; i++) {
                        Object v = fromNative(items[i], componentType);
                        if (v != null || !component.isPrimitive()) Array.set(out, i, v);
                    }
                    return out;
                }
                Type element = typeArg(type, 0);
                Collection<Object> out = Set.class.isAssignableFrom(cls) || (p.tag == Pack.SET && !List.class.isAssignableFrom(cls)) ? new LinkedHashSet<>() : new ArrayList<>();
                for (Object item : items) out.add(fromNative(item, element));
                if (cls == Optional.class) return out.isEmpty() ? Optional.empty() : Optional.ofNullable(out.iterator().next());
                if (cls == Iterator.class) return out.iterator();
                if (cls == java.util.stream.Stream.class) return out.stream();
                return out;
            }
        }
    }
}
