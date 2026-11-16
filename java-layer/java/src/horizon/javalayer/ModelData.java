package horizon.javalayer;

import com.google.gson.JsonArray;
import com.google.gson.JsonElement;
import com.google.gson.JsonObject;
import com.google.gson.JsonParser;
import org.bukkit.Color;
import org.bukkit.inventory.meta.components.CustomModelDataComponent;

import java.lang.reflect.Proxy;
import java.util.ArrayList;
import java.util.List;

/**
 * ItemMeta.getCustomModelDataComponent / setCustomModelDataComponent: the item's `minecraft:custom_model_data`
 * component (floats, flags, strings, colors) as Bukkit's CustomModelDataComponent, a copy to edit and set back.
 */
public final class ModelData {
    private ModelData() {}

    /** The component's JSON a view holds. */
    public interface Held {
        JsonObject horizonJson();
    }

    static Object of(H item) {
        String json = Native.componentJson(item, "minecraft:custom_model_data");
        JsonObject o = json == null ? new JsonObject() : JsonParser.parseString(json).getAsJsonObject();
        return view(o);
    }

    static void set(H item, Object component) {
        JsonObject o = component instanceof Held held ? held.horizonJson() : null;
        if (o == null) return;
        Native.setComponentJson(item, "minecraft:custom_model_data", o.toString());
    }

    private static Object view(JsonObject o) {
        return Proxy.newProxyInstance(ModelData.class.getClassLoader(), new Class<?>[]{CustomModelDataComponent.class, Held.class}, (proxy, m, args) -> {
            switch (m.getName()) {
                case "horizonJson": return o;
                case "getFloats": return list(o, "floats", e -> e.getAsFloat());
                case "getFlags": return list(o, "flags", e -> e.getAsBoolean());
                case "getStrings": return list(o, "strings", e -> e.getAsString());
                case "getColors": return list(o, "colors", e -> Color.fromRGB(e.getAsInt() & 0xFFFFFF));
                case "setFloats": o.add("floats", array((List<?>) args[0], v -> ((Number) v).floatValue())); return null;
                case "setFlags": o.add("flags", array((List<?>) args[0], v -> (Boolean) v)); return null;
                case "setStrings": o.add("strings", array((List<?>) args[0], v -> (String) v)); return null;
                case "setColors": o.add("colors", array((List<?>) args[0], v -> ((Color) v).asRGB())); return null;
                case "serialize": return java.util.Map.of("floats", list(o, "floats", e -> e.getAsFloat()), "flags", list(o, "flags", e -> e.getAsBoolean()), "strings", list(o, "strings", e -> e.getAsString()));
                case "toString": return "CustomModelDataComponent" + o;
                case "hashCode": return o.hashCode();
                case "equals": return args[0] instanceof Held h && h.horizonJson().equals(o);
                default: break;
            }
            throw Report.cant("CustomModelDataComponent." + m.getName());
        });
    }

    private static <T> List<T> list(JsonObject o, String key, java.util.function.Function<JsonElement, T> f) {
        List<T> out = new ArrayList<>();
        if (o.has(key) && o.get(key).isJsonArray()) for (JsonElement e : o.getAsJsonArray(key)) out.add(f.apply(e));
        return out;
    }

    private static JsonArray array(List<?> values, java.util.function.Function<Object, Object> f) {
        JsonArray a = new JsonArray();
        for (Object v : values) {
            Object x = f.apply(v);
            if (x instanceof Number n) a.add(n);
            else if (x instanceof Boolean b) a.add(b);
            else a.add(String.valueOf(x));
        }
        return a;
    }
}
