package horizon.javalayer;

import com.destroystokyo.paper.ClientOption;

import java.lang.reflect.Field;
import java.lang.reflect.Modifier;
import java.util.Locale;

/**
 * Paper's getClientOption(ClientOption) on a player or a connection (EssentialsX reads LOCALE as a player
 * configures, to load their language): the option goes to the server by its name, and what the client said comes
 * back as the option's own type (MainHand, ChatVisibility, ParticleVisibility, SkinParts, Integer, Boolean, String);
 * what the client hasn't said is Paper's default for it.
 */
final class ClientOptions {
    private ClientOptions() {}

    /** The constant's name in ClientOption (LOCALE, VIEW_DISTANCE...). */
    static String name(ClientOption<?> option) {
        for (Field f : ClientOption.class.getFields()) {
            if (!Modifier.isStatic(f.getModifiers()) || f.getType() != ClientOption.class) continue;
            try {
                if (f.get(null) == option) return f.getName();
            } catch (IllegalAccessException ignored) {
            }
        }
        return null;
    }

    /** The option's value for the game object `h`, or null when the server can't answer it. */
    static Object of(H h, ClientOption<?> option) {
        String name = name(option);
        if (name == null) return null;
        // A player takes the name; a configuration-phase connection the server's own ClientOption.
        Object raw = Native.tryCall(h, "getClientOption", new Object[]{name});
        if (raw == Native.MISSING) {
            Object sdk = Native.tryStatic("ClientOption", "$byName", new Object[]{name});
            raw = sdk == null || sdk == Native.MISSING ? Native.MISSING : Native.tryCall(h, "getClientOption", new Object[]{sdk});
        }
        if (raw == Native.MISSING) return null;
        return typed(option, name, raw);
    }

    @SuppressWarnings({"unchecked", "rawtypes"})
    static Object typed(ClientOption<?> option, String name, Object raw) {
        Class<?> type = option.getType();
        if (type == String.class) return raw == null ? "en_us" : String.valueOf(raw);
        if (type == Integer.class) return raw instanceof Number n ? n.intValue() : org.bukkit.Bukkit.getViewDistance();
        if (type == Boolean.class) {
            if (raw instanceof Boolean b) return b;
            if (raw instanceof Number n) return n.intValue() != 0;
            return !name.equals("TEXT_FILTERING_ENABLED");
        }
        if (type.isEnum()) {
            Object[] values = type.getEnumConstants();
            if (raw instanceof Number n && n.intValue() >= 0 && n.intValue() < values.length) return values[n.intValue()];
            if (raw != null) {
                try {
                    return Enum.valueOf((Class) type, String.valueOf(raw).toUpperCase(Locale.ROOT));
                } catch (IllegalArgumentException ignored) {
                }
            }
            // Paper's defaults: the right hand, full chat, all particles.
            if (type == org.bukkit.inventory.MainHand.class) return org.bukkit.inventory.MainHand.RIGHT;
            return values[0];
        }
        if (type == com.destroystokyo.paper.SkinParts.class) return skinParts(raw instanceof Number n ? n.intValue() : 0x7F);
        return raw;
    }

    /** Paper's SkinParts over the client's bits (cape 0x01, jacket, left/right sleeve, left/right pants, hat 0x40). */
    static com.destroystokyo.paper.SkinParts skinParts(int raw) {
        return (com.destroystokyo.paper.SkinParts) java.lang.reflect.Proxy.newProxyInstance(
                com.destroystokyo.paper.SkinParts.class.getClassLoader(), new Class<?>[]{com.destroystokyo.paper.SkinParts.class},
                (self, m, args) -> switch (m.getName()) {
                    case "hasCapeEnabled" -> (raw & 0x01) != 0;
                    case "hasJacketEnabled" -> (raw & 0x02) != 0;
                    case "hasLeftSleeveEnabled" -> (raw & 0x04) != 0;
                    case "hasRightSleeveEnabled" -> (raw & 0x08) != 0;
                    case "hasLeftPantsEnabled" -> (raw & 0x10) != 0;
                    case "hasRightPantsEnabled" -> (raw & 0x20) != 0;
                    case "hasHatsEnabled" -> (raw & 0x40) != 0;
                    case "getRaw" -> raw;
                    case "equals" -> self == args[0];
                    case "hashCode" -> raw;
                    case "toString" -> "SkinParts{raw=" + raw + "}";
                    default -> m.isDefault() ? java.lang.reflect.InvocationHandler.invokeDefault(self, m, args) : null;
                });
    }
}
