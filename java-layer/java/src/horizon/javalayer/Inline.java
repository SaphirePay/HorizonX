package horizon.javalayer;

import com.google.gson.JsonObject;
import com.google.gson.JsonParser;

import java.lang.reflect.Method;
import java.lang.reflect.Proxy;
import java.util.function.Consumer;

/**
 * Paper's inlined registry values (createTrimMaterial, createTrimPattern): a builder whose setters become the
 * game's JSON fields (assetName -> asset_name), and the value it builds is read from that JSON (Components.View).
 * Also TextDialogInput.MultilineOptions, which the server takes as its two numbers.
 */
public final class Inline {
    private Inline() {}

    /** The JSON a value made here holds. */
    public interface Made {
        JsonObject horizonJson();
    }

    @SuppressWarnings({"unchecked", "rawtypes"})
    static Object create(Method bridge, Object consumer) {
        Class<?> value = bridge.getReturnType();
        Class<?> builderType = builderOf(value);
        JsonObject json = new JsonObject();
        Object builder = Proxy.newProxyInstance(Inline.class.getClassLoader(), new Class<?>[]{builderType}, (proxy, m, args) -> {
            switch (m.getName()) {
                case "toString": return builderType.getSimpleName() + json;
                case "hashCode": return System.identityHashCode(proxy);
                case "equals": return proxy == args[0];
                default: break;
            }
            if (args != null && args.length == 1) {
                String text = args[0] instanceof Made made ? made.horizonJson().toString() : Components.json(args[0]);
                if (text != null) json.add(Components.snake(m.getName()), JsonParser.parseString(text));
                return m.getReturnType().isInstance(proxy) ? proxy : null;
            }
            if (m.isDefault()) return java.lang.reflect.InvocationHandler.invokeDefault(proxy, m, args);
            return m.getReturnType().isInstance(proxy) ? proxy : null;
        });
        Object factory = Proxy.newProxyInstance(Inline.class.getClassLoader(), new Class<?>[]{io.papermc.paper.registry.RegistryBuilderFactory.class}, (proxy, m, args) -> switch (m.getName()) {
            case "empty", "copyFrom" -> builder;
            case "hashCode" -> System.identityHashCode(proxy);
            case "equals" -> proxy == args[0];
            default -> "RegistryBuilderFactory";
        });
        ((Consumer) consumer).accept(factory);
        return Proxy.newProxyInstance(Inline.class.getClassLoader(), new Class<?>[]{value, Made.class}, (proxy, m, args) -> {
            if (m.getName().equals("horizonJson")) return json;
            if (m.getName().equals("getKey") || m.getName().equals("key")) return null;
            return new Components.View(value, json).invoke(proxy, m, args);
        });
    }

    /** The registry entry builder interface of a value type (TrimMaterial -> TrimMaterialRegistryEntry.Builder). */
    private static Class<?> builderOf(Class<?> value) {
        String name = "io.papermc.paper.registry.data." + value.getSimpleName() + "RegistryEntry$Builder";
        try {
            return Class.forName(name);
        } catch (ClassNotFoundException e) {
            throw Report.cant("registry builder for " + value.getSimpleName());
        }
    }

    /** TextDialogInput.MultilineOptions: its two numbers, for the server's multiline(maxLines, height). */
    static Object multiline(Integer maxLines, Integer height) {
        return Proxy.newProxyInstance(Inline.class.getClassLoader(), new Class<?>[]{io.papermc.paper.registry.data.dialog.input.TextDialogInput.MultilineOptions.class}, (proxy, m, args) -> switch (m.getName()) {
            case "maxLines" -> maxLines;
            case "height" -> height;
            case "hashCode" -> java.util.Objects.hash(maxLines, height);
            case "equals" -> proxy == args[0];
            default -> "MultilineOptionsImpl[maxLines=" + maxLines + ", height=" + height + "]";
        });
    }
}
