package horizon.javalayer;

import org.bukkit.event.Event;
import org.bukkit.event.entity.EntityDamageEvent;

import java.lang.reflect.Field;
import java.util.EnumMap;
import java.util.Map;
import java.util.function.Function;

/**
 * EntityDamageEvent keeps its damage in maps by modifier (BASE, ARMOR...): filled from the server's damage, so
 * getDamage/setDamage/getFinalDamage work; a changed damage goes back with setDamage.
 */
final class Damage {
    private Damage() {}

    private static final Field MODIFIERS, FUNCTIONS, ORIGINALS;

    static {
        Field m = null, f = null, o = null;
        try {
            m = EntityDamageEvent.class.getDeclaredField("modifiers");
            f = EntityDamageEvent.class.getDeclaredField("modifierFunctions");
            o = EntityDamageEvent.class.getDeclaredField("originals");
            m.setAccessible(true);
            f.setAccessible(true);
            o.setAccessible(true);
        } catch (ReflectiveOperationException ignored) {}
        MODIFIERS = m;
        FUNCTIONS = f;
        ORIGINALS = o;
    }

    static boolean isDamageMap(Field field) {
        return field.equals(MODIFIERS) || field.equals(FUNCTIONS) || field.equals(ORIGINALS);
    }

    @SuppressWarnings("deprecation")
    static double fill(Event event, H h) {
        if (!(event instanceof EntityDamageEvent) || MODIFIERS == null) return Double.NaN;
        Object raw = Native.tryCall(h, "getDamage", new Object[0]);
        double damage = raw instanceof Number n ? n.doubleValue() : 0;
        try {
            Map<EntityDamageEvent.DamageModifier, Double> modifiers = new EnumMap<>(EntityDamageEvent.DamageModifier.class);
            modifiers.put(EntityDamageEvent.DamageModifier.BASE, damage);
            Map<EntityDamageEvent.DamageModifier, Function<? super Double, Double>> functions = new EnumMap<>(EntityDamageEvent.DamageModifier.class);
            functions.put(EntityDamageEvent.DamageModifier.BASE, d -> -0.0);
            MODIFIERS.set(event, modifiers);
            FUNCTIONS.set(event, functions);
            ORIGINALS.set(event, new EnumMap<>(modifiers));
        } catch (IllegalAccessException ignored) {}
        return damage;
    }

    static void writeBack(Event event, H h, double before) {
        if (!(event instanceof EntityDamageEvent e) || Double.isNaN(before)) return;
        double now = e.getDamage();
        if (now != before) Native.tryCall(h, "setDamage", new Object[]{now});
    }
}
