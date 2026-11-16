package horizon.javalayer;

import net.kyori.adventure.key.Key;
import org.bukkit.Fluid;
import org.bukkit.GameEvent;
import org.bukkit.Keyed;
import org.bukkit.Material;
import org.bukkit.NamespacedKey;
import org.bukkit.Tag;
import org.bukkit.entity.EntityType;

import java.lang.reflect.Proxy;
import java.util.*;
import java.util.concurrent.ConcurrentHashMap;

/** Bukkit's tags (`Tag.LOGS`, `Bukkit.getTag(...)`): the game's own tag data, asked of the server. */
final class Tags {
    private Tags() {}

    private static final Map<String, Tag<?>> MADE = new ConcurrentHashMap<>();

    static String keyOf(Object value) {
        if (value instanceof Keyed k) return k.getKey().toString();
        if (value instanceof Key k) return k.asString();
        return String.valueOf(value);
    }

    static Object valueOf(Class<?> type, String key) {
        if (type == Material.class) return Material.matchMaterial(key);
        if (type == EntityType.class) {
            NamespacedKey k = NamespacedKey.fromString(key);
            return k == null ? null : EntityType.fromName(k.getKey());
        }
        if (type == Fluid.class) return Conv.enumOf(Fluid.class, key);
        if (type == GameEvent.class) return Registries.byName(GameEvent.class, key);
        return null;
    }

    /** `Bukkit.getTag(registry, key, class)`: null when the game has no such tag. */
    @SuppressWarnings("unchecked")
    static <T extends Keyed> Tag<T> get(String registry, NamespacedKey key, Class<T> type) {
        if (key == null) return null;
        return (Tag<T>) MADE.computeIfAbsent(registry + "|" + key, k -> make(registry, key, type));
    }

    private static Tag<?> make(String registry, NamespacedKey key, Class<?> type) {
        String name = key.toString();
        Object[] members = new Object[1];
        return (Tag<?>) Proxy.newProxyInstance(Tags.class.getClassLoader(), new Class<?>[]{Tag.class}, (proxy, m, args) -> {
            switch (m.getName()) {
                case "getKey": return key;
                case "key": return Key.key(key.toString());
                case "isTagged": {
                    Object answer = Native.tag(registry, name, keyOf(args[0]));
                    return answer instanceof Boolean b && b;
                }
                case "getValues": {
                    if (members[0] == null) {
                        Set<Object> out = new LinkedHashSet<>();
                        Object raw = Native.tag(registry, name, null);
                        if (raw instanceof String[] ids) for (String id : ids) {
                            Object v = valueOf(type, id);
                            if (v != null) out.add(v);
                        }
                        members[0] = Collections.unmodifiableSet(out);
                    }
                    return members[0];
                }
                case "toString": return "Tag{" + key + "}";
                case "hashCode": return key.hashCode();
                case "equals": return args[0] instanceof Tag<?> t && key.equals(t.getKey());
                default: break;
            }
            if (m.isDefault()) return java.lang.reflect.InvocationHandler.invokeDefault(proxy, m, args);
            return null;
        });
    }

    /** `Bukkit.getTags(registry, class)`: every tag of that folder. */
    static <T extends Keyed> Iterable<Tag<T>> all(String registry, Class<T> type) {
        List<Tag<T>> out = new ArrayList<>();
        Object raw = Native.callStatic("Bukkit", "getTags", new Object[]{registry, type.getSimpleName()});
        if (raw instanceof Pack p && p.items != null) {
            for (Object item : p.items) {
                NamespacedKey key = NamespacedKey.fromString(String.valueOf(item instanceof H h ? Native.call(h, "getKey", new Object[0]) : item));
                if (key != null) out.add(get(registry, key, type));
            }
        }
        return out;
    }
}
