package horizon.javalayer;

import com.google.gson.JsonArray;
import com.google.gson.JsonElement;
import com.google.gson.JsonObject;
import com.google.gson.JsonParser;
import net.kyori.adventure.text.Component;
import net.kyori.adventure.text.serializer.gson.GsonComponentSerializer;
import net.kyori.adventure.text.serializer.legacy.LegacyComponentSerializer;

import java.util.ArrayList;
import java.util.List;

/**
 * BookMeta's pages over the item's book component: `minecraft:written_book_content` (pages are text components)
 * or `minecraft:writable_book_content` (pages are plain text), each page as the game keeps it ({"raw": ...} or the
 * value itself). Text pages use § colours as Bukkit's String API does.
 */
public final class Books {
    private Books() {}

    static final Object NONE = Shapes.NONE;

    private static String id(H h) {
        if (Native.componentJson(h, "minecraft:written_book_content") != null) return "minecraft:written_book_content";
        if (Native.componentJson(h, "minecraft:writable_book_content") != null) return "minecraft:writable_book_content";
        Object type = Native.tryCall(h, "getType", new Object[0]);
        return String.valueOf(type).toUpperCase().contains("WRITABLE") ? "minecraft:writable_book_content" : "minecraft:written_book_content";
    }

    private static JsonObject content(H h, String id) {
        String json = Native.componentJson(h, id);
        return json == null ? new JsonObject() : JsonParser.parseString(json).getAsJsonObject();
    }

    private static List<Component> pages(H h) {
        String id = id(h);
        List<Component> out = new ArrayList<>();
        JsonObject c = content(h, id);
        if (!c.has("pages")) return out;
        for (JsonElement p : c.getAsJsonArray("pages")) {
            JsonElement v = p.isJsonObject() && p.getAsJsonObject().has("raw") ? p.getAsJsonObject().get("raw") : p;
            if (id.endsWith("writable_book_content")) out.add(LegacyComponentSerializer.legacySection().deserialize(v.getAsString()));
            else out.add(v.isJsonPrimitive() ? Component.text(v.getAsString()) : GsonComponentSerializer.gson().deserializeFromTree(v));
        }
        return out;
    }

    private static void write(H h, List<Component> pages) {
        String id = id(h);
        JsonObject c = content(h, id);
        JsonArray list = new JsonArray();
        for (Component p : pages) {
            if (id.endsWith("writable_book_content")) list.add(LegacyComponentSerializer.legacySection().serialize(p));
            else list.add(GsonComponentSerializer.gson().serializeToTree(p));
        }
        c.add("pages", list);
        if (id.endsWith("written_book_content")) {
            if (!c.has("title")) c.addProperty("title", "");
            if (!c.has("author")) c.addProperty("author", "");
        }
        Native.setComponentJson(h, id, c.toString());
    }

    private static Component component(Object o) {
        return o instanceof Component c ? c : LegacyComponentSerializer.legacySection().deserialize(String.valueOf(o));
    }

    private static String legacy(Component c) {
        return LegacyComponentSerializer.legacySection().serialize(c);
    }

    /** BookMeta calls on pages. NONE for other names. */
    @SuppressWarnings("unchecked")
    static Object call(H h, String name, Object[] args, Class<?> returns) {
        int n = args == null ? 0 : args.length;
        switch (name) {
            case "getPages": {
                List<String> out = new ArrayList<>();
                for (Component c : pages(h)) out.add(legacy(c));
                return out;
            }
            case "pages":
                if (n == 0) return pages(h);
                if (n == 1 && args[0] instanceof List<?> l) { List<Component> p = new ArrayList<>(); for (Object o : l) p.add(component(o)); write(h, p); return null; }
                if (n == 1 && args[0] instanceof Component[] arr) { write(h, List.of(arr)); return null; }
                break;
            case "getPage":
            case "page":
                if (n == 1 && args[0] instanceof Integer i) {
                    List<Component> p = pages(h);
                    if (i < 1 || i > p.size()) throw new IllegalArgumentException("Invalid page number " + i + "/" + p.size());
                    return name.equals("page") ? p.get(i - 1) : legacy(p.get(i - 1));
                }
                if (n == 2 && name.equals("page") && args[0] instanceof Integer i) {
                    List<Component> p = pages(h);
                    if (i >= 1 && i <= p.size()) { p.set(i - 1, component(args[1])); write(h, p); }
                    return null;
                }
                break;
            case "setPage":
                if (n == 2 && args[0] instanceof Integer i) {
                    List<Component> p = pages(h);
                    if (i < 1 || i > p.size()) throw new IllegalArgumentException("Invalid page number " + i + "/" + p.size());
                    p.set(i - 1, component(args[1]));
                    write(h, p);
                    return null;
                }
                break;
            case "setPages":
                if (n == 1) {
                    List<Component> p = new ArrayList<>();
                    if (args[0] instanceof List<?> l) for (Object o : l) p.add(component(o));
                    else if (args[0] instanceof Object[] arr) for (Object o : arr) p.add(component(o));
                    write(h, p);
                    return null;
                }
                break;
            case "addPage":
            case "addPages": {
                List<Component> p = pages(h);
                if (args != null) for (Object o : args) {
                    if (o instanceof Object[] arr) for (Object x : arr) p.add(component(x));
                    else p.add(component(o));
                }
                write(h, p);
                return null;
            }
            case "getPageCount":
                if (n == 0) return pages(h).size();
                break;
            case "hasPages":
                if (n == 0) return !pages(h).isEmpty();
                break;
            default:
                break;
        }
        return NONE;
    }
}
