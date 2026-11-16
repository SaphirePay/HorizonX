package horizon.javalayer;

import org.bukkit.Location;
import org.bukkit.Material;
import org.bukkit.Statistic;
import org.bukkit.entity.Entity;
import org.bukkit.entity.EntityType;

import java.util.function.Consumer;

/** Statistics by the game's names (type and key), and spawning by an entity's class. */
final class Stats {
    private Stats() {}

    static String typeKey(Statistic stat) {
        return switch (stat) {
            case MINE_BLOCK -> "minecraft:mined";
            case BREAK_ITEM -> "minecraft:broken";
            case CRAFT_ITEM -> "minecraft:crafted";
            case USE_ITEM -> "minecraft:used";
            case PICKUP -> "minecraft:picked_up";
            case DROP -> "minecraft:dropped";
            case KILL_ENTITY -> "minecraft:killed";
            case ENTITY_KILLED_BY -> "minecraft:killed_by";
            default -> "minecraft:custom";
        };
    }

    /** get/set/increment/decrementStatistic(stat[, material | entity type][, amount]). */
    static Object call(H player, String name, Statistic stat, Object[] args) {
        // The SDK keeps statistics on OfflinePlayer: a player's own offline self answers.
        H h = player;
        if (player.kind.equals("org.bukkit.entity.Player") && player.key.startsWith("p:")) {
            Object off = Native.tryStatic("Bukkit", "getOfflinePlayer", new Object[]{player.key.substring(2)});
            if (off instanceof H o) h = o;
        }
        String type = typeKey(stat);
        String key = stat.getKey().toString();
        int at = 1;
        if (args.length > 1 && args[1] instanceof Material m) {
            key = m.getKey().toString();
            at = 2;
        } else if (args.length > 1 && args[1] instanceof EntityType e) {
            key = e.getKey().toString();
            at = 2;
        }
        int amount = args.length > at && args[at] instanceof Integer i ? i : 1;
        Object current = Native.call(h, "getStatistic", new Object[]{type, key});
        long now = current instanceof Number number ? number.longValue() : 0;
        switch (name) {
            case "getStatistic": return (int) now;
            case "setStatistic": Native.call(h, "setStatistic", new Object[]{type, key, (long) amount}); return null;
            case "incrementStatistic": Native.call(h, "setStatistic", new Object[]{type, key, now + amount}); return null;
            default: Native.call(h, "setStatistic", new Object[]{type, key, Math.max(0, now - amount)}); return null;
        }
    }

    /** `world.spawn(location, Zombie.class[, reason][, consumer])`: the entity type of that class. */
    @SuppressWarnings("unchecked")
    static Object spawn(H h, Location at, Class<?> type, Object[] args) {
        EntityType found = null;
        for (EntityType t : EntityType.values()) if (t.getEntityClass() != null && type.isAssignableFrom(t.getEntityClass()) && (found == null || t.getEntityClass() == type)) found = t;
        if (found == null) throw new IllegalArgumentException("Can't spawn an entity of " + type.getName());
        Entity spawned = (Entity) Conv.fromNative(Native.call(h, "spawnEntity", Conv.args(new Object[]{at, found})), Entity.class);
        for (Object a : args) if (a instanceof Consumer<?> c && spawned != null) ((Consumer<Object>) c).accept(spawned);
        return spawned;
    }
}
