package horizon.javalayer;

import com.google.gson.JsonElement;
import com.google.gson.JsonObject;
import com.google.gson.JsonParser;

import java.lang.reflect.InvocationHandler;
import java.lang.reflect.Method;
import java.lang.reflect.Proxy;
import java.util.function.Consumer;

/**
 * Paper's dialogs (`Dialog.create(b -> b.empty().base(...).type(...))`): the builder collects the parts (made by the
 * server as the game's JSON) into one dialog; `showDialog` hands that JSON to the game's `/dialog show`.
 */
public final class Dialogs {
    private Dialogs() {}

    /** A dialog made here: its JSON. */
    public interface Made {
        JsonObject horizonJson();
    }

    @SuppressWarnings({"unchecked", "rawtypes"})
    static Object create(Object consumer) {
        JsonObject fields = new JsonObject();
        Object builder = Proxy.newProxyInstance(Dialogs.class.getClassLoader(), new Class<?>[]{io.papermc.paper.registry.data.dialog.DialogRegistryEntry.Builder.class}, (proxy, m, args) -> {
            switch (m.getName()) {
                case "base":
                case "type":
                    if (args != null && args.length == 1) {
                        JsonElement part = json(args[0]);
                        if (part != null && part.isJsonObject()) for (var e : part.getAsJsonObject().entrySet()) fields.add(e.getKey(), e.getValue());
                        return proxy;
                    }
                    break;
                case "toString": return "DialogBuilder" + fields;
                case "hashCode": return System.identityHashCode(proxy);
                case "equals": return proxy == args[0];
                default: break;
            }
            throw Report.cant("DialogRegistryEntry.Builder." + m.getName());
        });
        Object factory = Proxy.newProxyInstance(Dialogs.class.getClassLoader(), new Class<?>[]{io.papermc.paper.registry.RegistryBuilderFactory.class}, (proxy, m, args) -> {
            switch (m.getName()) {
                case "empty": return builder;
                case "copyFrom":
                    if (args != null && args.length == 1) {
                        // A registry dialog as the start: its JSON from the server, by key.
                        Object raw = Native.tryStatic("Dialog", "getEntry", Conv.args(args));
                        if (raw != Native.MISSING) {
                            JsonElement e = json(Conv.fromNative(raw, Object.class));
                            if (e != null && e.isJsonObject()) for (var x : e.getAsJsonObject().entrySet()) fields.add(x.getKey(), x.getValue());
                        }
                        return builder;
                    }
                    break;
                case "toString": return "RegistryBuilderFactory";
                case "hashCode": return System.identityHashCode(proxy);
                case "equals": return proxy == args[0];
                default: break;
            }
            throw Report.cant("RegistryBuilderFactory." + m.getName());
        });
        ((Consumer) consumer).accept(factory);
        return made(fields);
    }

    static Object made(JsonObject json) {
        InvocationHandler h = (proxy, m, args) -> {
            switch (m.getName()) {
                case "horizonJson": return json;
                case "toString": return "Dialog" + json;
                case "hashCode": return json.hashCode();
                case "equals": return args[0] instanceof Made other && other.horizonJson().equals(json);
                case "key": case "getKey": return null;
                default: break;
            }
            if (m.isDefault()) return InvocationHandler.invokeDefault(proxy, m, args);
            throw Report.cant("Dialog." + m.getName());
        };
        return Proxy.newProxyInstance(Dialogs.class.getClassLoader(), new Class<?>[]{io.papermc.paper.dialog.Dialog.class, Made.class}, h);
    }

    /** The game's JSON for a dialog part the server made (DialogBase, DialogType, a body, an input...). */
    static JsonElement json(Object part) {
        if (part instanceof Made made) return made.horizonJson();
        String text = Native.valueJson(Conv.toNative(part));
        return text == null ? null : JsonParser.parseString(text);
    }

    /** Player.showDialog of a dialog made here. */
    static boolean show(org.bukkit.entity.Player player, Object dialog) {
        if (!(dialog instanceof Made made)) return false;
        org.bukkit.Bukkit.dispatchCommand(org.bukkit.Bukkit.getConsoleSender(), "dialog show " + player.getName() + " " + made.horizonJson());
        return true;
    }
}
