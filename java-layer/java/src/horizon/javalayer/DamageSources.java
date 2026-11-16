package horizon.javalayer;

import org.bukkit.Location;
import org.bukkit.damage.DamageSource;
import org.bukkit.damage.DamageType;
import org.bukkit.entity.Entity;

import java.lang.reflect.InvocationHandler;
import java.lang.reflect.Proxy;

/** `DamageSource.builder(type)`: a damage source made in Java (its type, entities and place). */
final class DamageSources {
    private DamageSources() {}

    static Object builder(Class<?> builderType, DamageType type) {
        Object[] state = new Object[4]; // causing entity, direct entity, location, context (Adventure pointers)
        InvocationHandler handler = (proxy, m, args) -> {
            switch (m.getName()) {
                case "withCausingEntity": state[0] = args[0]; return proxy;
                case "withDirectEntity": state[1] = args[0]; return proxy;
                case "withDamageLocation": state[2] = args[0]; return proxy;
                case "withDamageContext": {
                    net.kyori.adventure.pointer.Pointers.Builder b = net.kyori.adventure.pointer.Pointers.builder();
                    @SuppressWarnings("unchecked") java.util.function.Consumer<net.kyori.adventure.pointer.Pointers.Builder> fill = (java.util.function.Consumer<net.kyori.adventure.pointer.Pointers.Builder>) args[0];
                    fill.accept(b);
                    state[3] = b.build();
                    return proxy;
                }
                case "build": return source(type, (Entity) state[0], (Entity) state[1], (Location) state[2], (net.kyori.adventure.pointer.Pointers) state[3]);
                case "toString": return "DamageSource.Builder[" + type + "]";
                case "hashCode": return System.identityHashCode(proxy);
                case "equals": return proxy == args[0];
                default: break;
            }
            if (m.getReturnType().isInstance(proxy)) return proxy;
            return null;
        };
        return Proxy.newProxyInstance(DamageSources.class.getClassLoader(), new Class<?>[]{builderType}, handler);
    }

    static DamageSource source(DamageType type, Entity causing, Entity direct, Location location) {
        return source(type, causing, direct, location, null);
    }

    static DamageSource source(DamageType type, Entity causing, Entity direct, Location location, net.kyori.adventure.pointer.Pointers context) {
        // Paper's own class (CraftDamageSource), not a proxy: plugins and the probes read its name.
        return new org.bukkit.craftbukkit.damage.CraftDamageSource(type, causing, direct, location, context);
    }
}
