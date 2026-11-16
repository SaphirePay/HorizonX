package horizon.javalayer;

import org.bukkit.Color;
import org.bukkit.NamespacedKey;
import org.bukkit.Registry;
import org.bukkit.block.Block;
import org.bukkit.block.BlockFace;
import org.bukkit.entity.Entity;
import org.bukkit.potion.PotionEffect;
import org.bukkit.potion.PotionEffectType;
import org.bukkit.util.BoundingBox;
import org.bukkit.util.RayTraceResult;
import org.bukkit.util.Vector;

/**
 * Bukkit's value classes (concrete Java classes, not interfaces) the server has as its own structs: made from the
 * server's object by its getters, and back by the SDK's constructor.
 */
final class Values {
    private Values() {}

    static final Object NOT_A_VALUE = new Object();

    private static Object get(H h, String name) {
        Object raw = Native.tryCall(h, name, new Object[0]);
        return raw == Native.MISSING ? null : raw;
    }

    private static double num(Object raw) { return raw instanceof Number n ? n.doubleValue() : 0; }

    private static boolean truth(Object raw) { return raw instanceof Boolean b && b; }

    /** A Bukkit value of class `cls` from the server's object, or NOT_A_VALUE when `cls` is none of these. */
    static Object fromNative(H h, Class<?> cls) {
        if (cls != PotionEffect.class && cls != BoundingBox.class && cls != Color.class && cls != RayTraceResult.class
                && cls != org.bukkit.block.banner.Pattern.class) return NOT_A_VALUE;
        Object out = make(h, cls);
        Native.release(h.id);
        return out;
    }

    private static Object make(H h, Class<?> cls) {
        if (cls == PotionEffect.class) return potion(h);
        if (cls == BoundingBox.class) return box(h);
        if (cls == Color.class) return Color.fromARGB((int) num(get(h, "asARGB")));
        if (cls == org.bukkit.block.banner.Pattern.class) return pattern(h);
        return ray(h);
    }

    /** A banner layer: Bukkit's Pattern, from the server's pattern key and dye colour name. */
    private static org.bukkit.block.banner.Pattern pattern(H h) {
        String key = String.valueOf(get(h, "getPattern")), color = String.valueOf(get(h, "getColor"));
        org.bukkit.block.banner.PatternType type = org.bukkit.Registry.BANNER_PATTERN.get(org.bukkit.NamespacedKey.fromString(key));
        if (type == null) return null;
        org.bukkit.DyeColor dye;
        try {
            dye = org.bukkit.DyeColor.valueOf(color.toUpperCase(java.util.Locale.ROOT));
        } catch (IllegalArgumentException e) {
            return null;
        }
        return new org.bukkit.block.banner.Pattern(dye, type);
    }

    private static PotionEffect potion(H h) {
        Object type = Conv.fromNative(get(h, "getType"), PotionEffectType.class);
        if (!(type instanceof PotionEffectType)) return null;
        PotionEffectType t = (PotionEffectType) type;
        int duration = (int) num(get(h, "getDuration"));
        int amplifier = (int) num(get(h, "getAmplifier"));
        boolean ambient = truth(get(h, "isAmbient"));
        boolean particles = truth(get(h, "hasParticles"));
        boolean icon = truth(get(h, "hasIcon"));
        PotionEffect effect = new PotionEffect(t, duration, amplifier, ambient, particles, icon);
        return effect;
    }

    private static BoundingBox box(H h) {
        BoundingBox b = new BoundingBox(num(get(h, "getMinX")), num(get(h, "getMinY")), num(get(h, "getMinZ")), num(get(h, "getMaxX")), num(get(h, "getMaxY")), num(get(h, "getMaxZ")));
        return b;
    }

    private static Object ray(H h) {
        Object at = Conv.fromNative(get(h, "getHitPosition"), Vector.class);
        Object block = Conv.fromNative(get(h, "getHitBlock"), Block.class);
        Object face = Conv.fromNative(get(h, "getHitBlockFace"), BlockFace.class);
        Object entity = Conv.fromNative(get(h, "getHitEntity"), Entity.class);
        Vector position = at instanceof Vector v ? v : new Vector();
        return RayTraces.of(position, (Block) block, (BlockFace) face, (Entity) entity);
    }

    /** The server's object for a Bukkit value, or NOT_A_VALUE. */
    static Object toNative(Object o) {
        if (o instanceof PotionEffect e) {
            NamespacedKey key = e.getType().getKey();
            Object made = Native.construct("PotionEffect", new Object[]{key.toString(), e.getDuration(), e.getAmplifier(), e.isAmbient(), e.hasParticles()});
            return made == Native.MISSING ? key.toString() : made;
        }
        if (o instanceof BoundingBox b) {
            Object made = Native.construct("BoundingBox", new Object[]{b.getMinX(), b.getMinY(), b.getMinZ(), b.getMaxX(), b.getMaxY(), b.getMaxZ()});
            return made == Native.MISSING ? NOT_A_VALUE : made;
        }
        return NOT_A_VALUE;
    }

    static PotionEffectType effect(String key) {
        NamespacedKey k = NamespacedKey.fromString(key);
        return k == null ? null : Registry.EFFECT.get(k);
    }
}
