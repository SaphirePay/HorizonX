package horizon.javalayer;

import java.lang.invoke.MethodHandle;
import java.lang.invoke.MethodHandles;
import java.lang.reflect.Method;
import java.util.Map;
import java.util.concurrent.ConcurrentHashMap;

/**
 * CraftPlayers for vanilla players a plugin makes itself and never adds to the server (GSit's pose bodies, NPC
 * plugins' fake players): on Paper `new ServerPlayer(...)` then getBukkitEntity() gives a CraftPlayer over it. Here
 * that CraftPlayer keeps its ServerPlayer, and each Player method runs Paper's CraftHumanEntity/CraftLivingEntity/
 * CraftEntity code on it, as Paper's CraftPlayer inherits it; what only a CraftPlayer has answers its type's default
 * (an NPC isn't online, has no connection, no permissions of its own).
 */
public final class Npcs {
    private Npcs() {}

    public static void init(org.bukkit.craftbukkit.entity.CraftHumanEntity self, org.bukkit.craftbukkit.CraftServer server, net.minecraft.world.entity.Entity entity) {
        Craft.set(self, org.bukkit.craftbukkit.entity.CraftEntity.class, "entity", entity);
        Craft.set(self, org.bukkit.craftbukkit.entity.CraftEntity.class, "server", server);
        Craft.entity(self, org.bukkit.entity.EntityType.PLAYER);
    }

    public static Object handle(org.bukkit.craftbukkit.entity.CraftHumanEntity self) {
        return Craft.get(self, org.bukkit.craftbukkit.entity.CraftEntity.class, "entity");
    }

    /** Paper's inherited method by name and parameter types (null when CraftPlayer alone has it). */
    private static final Map<String, Object> SUPER = new ConcurrentHashMap<>();
    private static final Object NONE = new Object();

    public static Object invoke(org.bukkit.craftbukkit.entity.CraftHumanEntity self, Method m, Object[] args) {
        String key = m.getName() + java.util.Arrays.toString(m.getParameterTypes());
        Object found = SUPER.computeIfAbsent(key, k -> find(self.getClass(), m));
        try {
            if (found instanceof MethodHandle mh) {
                Object[] all = new Object[(args == null ? 0 : args.length) + 1];
                all[0] = self;
                if (args != null) System.arraycopy(args, 0, all, 1, args.length);
                return mh.invokeWithArguments(all);
            }
        } catch (RuntimeException | Error e) {
            throw e;
        } catch (Throwable t) {
            throw Players.sneaky(t);
        }
        return switch (m.getName()) {
            case "getName", "getDisplayName", "getPlayerListName" -> {
                Object e = handle(self);
                yield e instanceof net.minecraft.world.entity.player.Player p ? p.getGameProfile().name() : null;
            }
            case "isOnline", "isConnected", "isOp", "hasPlayedBefore" -> false;
            default -> defaultOf(m.getReturnType());
        };
    }

    private static Object find(Class<?> craftPlayer, Method m) {
        for (Class<?> c = craftPlayer.getSuperclass(); c != null && c != Object.class; c = c.getSuperclass()) {
            try {
                Method declared = c.getDeclaredMethod(m.getName(), m.getParameterTypes());
                if (java.lang.reflect.Modifier.isAbstract(declared.getModifiers())) continue;
                MethodHandles.Lookup lookup = MethodHandles.privateLookupIn(craftPlayer, MethodHandles.lookup());
                return lookup.findSpecial(c, m.getName(), java.lang.invoke.MethodType.methodType(declared.getReturnType(), declared.getParameterTypes()), craftPlayer);
            } catch (NoSuchMethodException e) {
                // up to the next class
            } catch (IllegalAccessException e) {
                return NONE;
            }
        }
        return NONE;
    }

    private static Object defaultOf(Class<?> type) {
        if (!type.isPrimitive()) {
            if (type == java.util.List.class) return java.util.List.of();
            if (type == java.util.Set.class || type == java.util.Collection.class) return java.util.Set.of();
            if (type == java.util.Map.class) return java.util.Map.of();
            return null;
        }
        if (type == boolean.class) return false;
        if (type == void.class) return null;
        if (type == char.class) return '\0';
        if (type == float.class) return 0f;
        if (type == double.class) return 0d;
        if (type == long.class) return 0L;
        if (type == int.class) return 0;
        if (type == short.class) return (short) 0;
        if (type == byte.class) return (byte) 0;
        return null;
    }
}
