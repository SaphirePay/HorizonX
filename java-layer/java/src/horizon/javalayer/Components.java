package horizon.javalayer;

import com.google.gson.JsonArray;
import com.google.gson.JsonElement;
import com.google.gson.JsonObject;
import com.google.gson.JsonParser;
import io.papermc.paper.datacomponent.DataComponentType;
import io.papermc.paper.datacomponent.DataComponentTypes;
import net.kyori.adventure.key.Key;
import net.kyori.adventure.text.Component;
import net.kyori.adventure.text.serializer.gson.GsonComponentSerializer;

import java.lang.reflect.*;
import java.util.*;
import java.util.concurrent.ConcurrentHashMap;

/**
 * Paper's `getData(type)` from the item's component as the game stores it (JSON): the value type comes from the
 * DataComponentTypes constant; scalars convert directly, and Paper's component interfaces (CustomModelData,
 * ItemLore, Tool...) are views whose methods read the JSON field of the same name (floats() -> "floats",
 * damagePerBlock() -> "damage_per_block").
 */
public final class Components {
    private Components() {}

    private static final Map<String, Type> TYPES = new ConcurrentHashMap<>();

    static {
        for (Field f : DataComponentTypes.class.getFields()) {
            try {
                if (!Modifier.isStatic(f.getModifiers())) continue;
                Object value = f.get(null);
                if (!(value instanceof DataComponentType t)) continue;
                Type g = f.getGenericType();
                TYPES.put(t.getKey().toString(), g instanceof ParameterizedType p ? p.getActualTypeArguments()[0] : Void.class);
            } catch (ReflectiveOperationException | RuntimeException | LinkageError ignored) {}
        }
    }

    static Object get(H item, DataComponentType type) {
        String id = type.getKey().toString();
        String json = Native.componentJson(item, id);
        // Every item's prototype has an empty minecraft:enchantments (getData gives the empty one, as on Paper).
        if (json == null && id.equals("minecraft:enchantments")) json = "{}";
        if (json == null) return null;
        Type t = TYPES.getOrDefault(id, Object.class);
        if (t == Void.class) return null; // a component with no value (NonValued): getData has nothing to give
        return convert(JsonParser.parseString(json), t);
    }

    /** A plain Java value as component JSON (numbers, booleans, text, keys, components), or null. */
    static String json(Object v) {
        if (v instanceof Number || v instanceof Boolean) return String.valueOf(v);
        if (v instanceof String s) return new com.google.gson.JsonPrimitive(s).toString();
        if (v instanceof Key k) return new com.google.gson.JsonPrimitive(k.asString()).toString();
        if (v instanceof org.bukkit.Keyed k) return new com.google.gson.JsonPrimitive(k.getKey().toString()).toString();
        if (v instanceof Component c) return GsonComponentSerializer.gson().serialize(c);
        if (v instanceof org.bukkit.Color c) return String.valueOf(c.asARGB());
        if (Proxy.isProxyClass(v.getClass()) && Proxy.getInvocationHandler(v) instanceof View view) return view.json.toString();
        return null;
    }

    static Object convert(JsonElement e, Type type) {
        if (e == null || e.isJsonNull()) return null;
        Class<?> c = raw(type);
        if (c == Integer.class || c == int.class) return e.isJsonPrimitive() ? e.getAsInt() : 0;
        if (c == Float.class || c == float.class) return e.getAsFloat();
        if (c == Double.class || c == double.class) return e.getAsDouble();
        if (c == Boolean.class || c == boolean.class) return e.getAsBoolean();
        if (c == Long.class || c == long.class) return e.getAsLong();
        if (c == String.class) return e.isJsonPrimitive() ? e.getAsString() : e.toString();
        if (c == Key.class || c == org.bukkit.NamespacedKey.class) {
            String s = e.getAsString();
            return c == Key.class ? Key.key(s) : org.bukkit.NamespacedKey.fromString(s);
        }
        if (Component.class.isAssignableFrom(c)) return GsonComponentSerializer.gson().deserializeFromTree(e);
        if (c == org.bukkit.Color.class) return org.bukkit.Color.fromARGB(e.getAsInt());
        if (List.class.isAssignableFrom(c) || Collection.class.isAssignableFrom(c)) {
            Type item = type instanceof ParameterizedType p ? p.getActualTypeArguments()[0] : Object.class;
            List<Object> out = new ArrayList<>();
            if (e.isJsonArray()) for (JsonElement x : e.getAsJsonArray()) out.add(convert(x, item));
            return out;
        }
        if (java.util.Map.class.isAssignableFrom(c)) {
            // A map stored as a JSON object (ItemEnchantments: {"minecraft:sharpness": 2}): keys as the key type
            // (a registry value by its id, text), values as the value type.
            Type[] kv = type instanceof ParameterizedType p ? p.getActualTypeArguments() : new Type[]{String.class, Object.class};
            java.util.Map<Object, Object> out = new java.util.LinkedHashMap<>();
            if (e.isJsonObject()) {
                for (var entry : e.getAsJsonObject().entrySet()) {
                    Object k = convert(new com.google.gson.JsonPrimitive(entry.getKey()), kv[0]);
                    if (k != null) out.put(k, convert(entry.getValue(), kv[1]));
                }
            }
            return out;
        }
        if (c.isEnum()) {
            String s = e.getAsString().toUpperCase(Locale.ROOT);
            for (Object k : c.getEnumConstants()) if (((Enum<?>) k).name().equals(s)) return k;
            return null;
        }
        if (org.bukkit.Keyed.class.isAssignableFrom(c) && e.isJsonPrimitive()) {
            org.bukkit.NamespacedKey key = org.bukkit.NamespacedKey.fromString(e.getAsString());
            return key == null ? null : Registries.value(c, key);
        }
        if (c.isInterface()) return Proxy.newProxyInstance(Components.class.getClassLoader(), new Class<?>[]{c}, new View(c, e));
        return null;
    }

    private static Class<?> raw(Type t) {
        if (t instanceof Class<?> c) return c;
        if (t instanceof ParameterizedType p) return raw(p.getRawType());
        if (t instanceof WildcardType w) return raw(w.getUpperBounds()[0]);
        return Object.class;
    }

    static String snake(String name) {
        StringBuilder s = new StringBuilder();
        for (char ch : name.toCharArray()) {
            if (Character.isUpperCase(ch)) s.append('_').append(Character.toLowerCase(ch));
            else s.append(ch);
        }
        return s.toString();
    }

    /** A component interface read from its JSON: each no-argument method is the field of its name. */
    public static final class View implements InvocationHandler {
        final Class<?> type;
        final JsonElement json;

        public View(Class<?> type, JsonElement json) {
            this.type = type;
            this.json = json;
        }

        @Override
        public Object invoke(Object proxy, Method m, Object[] args) throws Throwable {
            String name = m.getName();
            int n = args == null ? 0 : args.length;
            switch (name) {
                case "toString":
                    if (n == 0 && type == io.papermc.paper.datacomponent.item.ItemEnchantments.class && json.isJsonObject()) return PaperMeta.enchantmentsString(json.getAsJsonObject());
                    if (n == 0) return type.getSimpleName() + json;
                    break;
                case "hashCode": if (n == 0) return json.hashCode(); break;
                case "equals": if (n == 1) return args[0] != null && Proxy.isProxyClass(args[0].getClass()) && Proxy.getInvocationHandler(args[0]) instanceof View v && v.json.equals(json); break;
                default: break;
            }
            if (n == 0 && m.getReturnType() != void.class) {
                JsonElement field;
                if (json.isJsonObject()) {
                    JsonObject o = json.getAsJsonObject();
                    field = o.has(snake(name)) ? o.get(snake(name)) : o.get(name);
                    // A component that is itself a map (ItemEnchantments, stored {"minecraft:sharpness": 2}): its
                    // map-returning method is the whole object.
                    if (field == null && java.util.Map.class.isAssignableFrom(m.getReturnType())) field = o;
                } else {
                    // A component stored as a bare list or value (ItemLore's lines, a single number).
                    field = json;
                }
                Object v = convert(field, m.getGenericReturnType());
                if (v == null && List.class.isAssignableFrom(m.getReturnType())) return List.of();
                // A record's map or set it doesn't hold is empty too (ItemEnchantments.enchantments() of none).
                if (v == null && java.util.Map.class.isAssignableFrom(m.getReturnType())) return java.util.Map.of();
                if (v == null && java.util.Set.class.isAssignableFrom(m.getReturnType())) return java.util.Set.of();
                if (v == null && m.getReturnType() == java.util.Collection.class) return List.of();
                if (v == null && m.getReturnType().isPrimitive()) return Conv.zero(m.getReturnType());
                return v;
            }
            if (m.isDefault()) return InvocationHandler.invokeDefault(proxy, m, args);
            throw Report.cant(type.getSimpleName() + "." + name + "/" + n);
        }
    }
}
