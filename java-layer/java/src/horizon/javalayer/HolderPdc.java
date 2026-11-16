package horizon.javalayer;

import com.google.gson.JsonArray;
import com.google.gson.JsonElement;
import com.google.gson.JsonObject;
import com.google.gson.JsonParser;
import com.google.gson.JsonPrimitive;
import java.util.ArrayList;
import java.util.HashSet;
import java.util.List;
import java.util.Map;
import java.util.Set;
import java.util.concurrent.ConcurrentHashMap;
import org.bukkit.Bukkit;
import org.bukkit.persistence.PersistentDataContainer;

/**
 * The PersistentDataContainer of an item meta (custom_data's PublicBukkitValues) or an entity (its BukkitValues), as
 * Paper keeps them: values in their primitive form, read back in the type asked for (FilePdc's container), written
 * through to the item or entity at each change.
 */
final class HolderPdc {
    private HolderPdc() {}

    private static final String ITEM_ROOT = "PublicBukkitValues";

    /** An item meta's container (its stack's custom_data). */
    static PersistentDataContainer ofItem(H item) {
        Map<String, Object> values = new ConcurrentHashMap<>();
        JsonObject data = customData(item);
        if (data.get(ITEM_ROOT) instanceof JsonObject root) for (Map.Entry<String, JsonElement> e : root.entrySet()) put(values, e.getKey(), e.getValue());
        return FilePdc.container(values, () -> {
            JsonObject now = customData(item);
            if (values.isEmpty()) now.remove(ITEM_ROOT);
            else now.add(ITEM_ROOT, toJson(values));
            Native.setComponentJson(item, "minecraft:custom_data", now.toString());
        });
    }

    private static JsonObject customData(H item) {
        String json = Native.componentJson(item, "minecraft:custom_data");
        if (json == null) return new JsonObject();
        JsonElement e = JsonParser.parseString(json);
        if (e.isJsonPrimitive() && e.getAsJsonPrimitive().isString()) e = JsonParser.parseString(e.getAsString());
        return e.isJsonObject() ? e.getAsJsonObject() : new JsonObject();
    }

    /** An entity's container (its BukkitValues): each change merged into the entity, a removal by /data remove. */
    static PersistentDataContainer ofEntity(H entity, org.bukkit.entity.Entity bukkit) {
        Map<String, Object> values = new ConcurrentHashMap<>();
        Object raw = Native.tryCall(entity, "getData", new Object[0]);
        Object tree = raw == Native.MISSING ? null : Conv.fromNative(raw, Map.class);
        if (tree instanceof Map<?, ?> all && all.get("BukkitValues") instanceof Map<?, ?> root) {
            for (Map.Entry<?, ?> e : root.entrySet()) values.put(String.valueOf(e.getKey()), plain(e.getValue()));
        }
        Set<String> known = new HashSet<>(values.keySet());
        return FilePdc.container(values, () -> {
            for (Map.Entry<String, Object> e : values.entrySet()) {
                Object key = Conv.toNative(org.bukkit.NamespacedKey.fromString(e.getKey()));
                Native.tryCall(entity, "pdcSet", new Object[]{key, Conv.toNative(jsonReady(e.getValue()))});
            }
            for (String gone : new ArrayList<>(known)) {
                if (values.containsKey(gone)) continue;
                Bukkit.dispatchCommand(Bukkit.getConsoleSender(), "data remove entity " + bukkit.getUniqueId() + " BukkitValues.\"" + gone + "\"");
            }
            known.clear();
            known.addAll(values.keySet());
        });
    }

    private static void put(Map<String, Object> into, String key, JsonElement value) {
        Object v = plain(value);
        if (v != null) into.put(key, v);
    }

    /** JSON (or the server's maps and lists) as primitives: whole numbers as Long, others as Double. */
    static Object plain(Object value) {
        if (value instanceof JsonElement e) {
            if (e.isJsonNull()) return null;
            if (e.isJsonPrimitive()) {
                JsonPrimitive p = e.getAsJsonPrimitive();
                if (p.isBoolean()) return p.getAsBoolean() ? (byte) 1 : (byte) 0;
                if (p.isNumber()) {
                    double d = p.getAsDouble();
                    return d == Math.rint(d) && !p.getAsString().contains(".") ? (Object) p.getAsLong() : (Object) d;
                }
                return p.getAsString();
            }
            if (e.isJsonArray()) {
                List<Object> out = new ArrayList<>();
                for (JsonElement x : e.getAsJsonArray()) out.add(plain(x));
                return out;
            }
            Map<String, Object> out = new ConcurrentHashMap<>();
            for (Map.Entry<String, JsonElement> x : e.getAsJsonObject().entrySet()) put(out, x.getKey(), x.getValue());
            return out;
        }
        if (value instanceof Map<?, ?> m) {
            Map<String, Object> out = new ConcurrentHashMap<>();
            for (Map.Entry<?, ?> x : m.entrySet()) { Object v = plain(x.getValue()); if (v != null) out.put(String.valueOf(x.getKey()), v); }
            return out;
        }
        if (value instanceof List<?> l) {
            List<Object> out = new ArrayList<>();
            for (Object x : l) out.add(plain(x));
            return out;
        }
        return value;
    }

    /** A primitive as JSON can hold it: arrays as lists. */
    static Object jsonReady(Object v) {
        if (v instanceof byte[] b) { List<Object> l = new ArrayList<>(); for (byte x : b) l.add((int) x); return l; }
        if (v instanceof int[] b) { List<Object> l = new ArrayList<>(); for (int x : b) l.add(x); return l; }
        if (v instanceof long[] b) { List<Object> l = new ArrayList<>(); for (long x : b) l.add(x); return l; }
        if (v instanceof Map<?, ?> m) { Map<String, Object> out = new java.util.LinkedHashMap<>(); for (Map.Entry<?, ?> e : m.entrySet()) out.put(String.valueOf(e.getKey()), jsonReady(e.getValue())); return out; }
        if (v instanceof List<?> l) { List<Object> out = new ArrayList<>(); for (Object x : l) out.add(jsonReady(x)); return out; }
        return v;
    }

    static JsonElement toJson(Object v) {
        v = jsonReady(v);
        if (v == null) return com.google.gson.JsonNull.INSTANCE;
        if (v instanceof Boolean b) return new JsonPrimitive(b);
        if (v instanceof Number n) return new JsonPrimitive(n);
        if (v instanceof String s) return new JsonPrimitive(s);
        if (v instanceof Map<?, ?> m) {
            JsonObject o = new JsonObject();
            for (Map.Entry<?, ?> e : m.entrySet()) o.add(String.valueOf(e.getKey()), toJson(e.getValue()));
            return o;
        }
        if (v instanceof List<?> l) {
            JsonArray a = new JsonArray();
            for (Object x : l) a.add(toJson(x));
            return a;
        }
        return new JsonPrimitive(String.valueOf(v));
    }
}
