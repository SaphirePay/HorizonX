package horizon.javalayer;

import org.bukkit.Color;
import org.bukkit.Location;
import org.bukkit.Particle;
import org.bukkit.block.data.BlockData;
import org.bukkit.entity.Player;
import org.bukkit.inventory.ItemStack;

/**
 * Player.spawnParticle's one abstract form (26.3: particle, x y z, count, offset x y z, speed x y z, data, force,
 * randomization), which every shorter overload reaches through the API's default methods: one
 * `core_particle_for` request to the server, `ServerLevel.sendParticles` to that player alone. The data is what the
 * particle takes, in the server's form; the wrong kind of data for a particle sends nothing, as Paper throws then.
 */
final class Particles {
    private Particles() {}

    static Object forPlayer(Player p, Object[] a) {
        Particle particle = (Particle) a[0];
        StringBuilder q = new StringBuilder("{\"op\":\"core_particle_for\",\"particle\":");
        q.append(quote(particle.getKey().toString()));
        q.append(",\"dimension\":").append(quote(dimension(p)));
        q.append(",\"pos\":[").append(num(a[1])).append(',').append(num(a[2])).append(',').append(num(a[3])).append(']');
        q.append(",\"count\":").append(((Number) a[4]).intValue());
        q.append(",\"offset\":[").append(num(a[5])).append(',').append(num(a[6])).append(',').append(num(a[7])).append(']');
        q.append(",\"speed\":[").append(num(a[8])).append(',').append(num(a[9])).append(',').append(num(a[10])).append(']');
        q.append(",\"data\":").append(data(a[11]));
        q.append(",\"force\":").append(Boolean.TRUE.equals(a[12]));
        if (a[13] instanceof Enum<?> e) q.append(",\"randomization\":").append(e.ordinal());
        q.append(",\"viewers\":[").append(quote(p.getUniqueId().toString().replace("-", ""))).append("]}");
        Native.query(q.toString());
        return null;
    }

    /** The level the player is in, as the server names it (a created world's own id included). */
    private static String dimension(Player p) {
        String state = Native.query("{\"op\":\"player\",\"uuid\":" + quote(p.getUniqueId().toString().replace("-", "")) + "}");
        java.util.regex.Matcher m = java.util.regex.Pattern.compile("\"dimension\":\"([^\"]+)\"").matcher(state == null ? "" : state);
        return m.find() ? m.group(1) : "minecraft:overworld";
    }

    private static String quote(String s) {
        return "\"" + s.replace("\\", "\\\\").replace("\"", "\\\"") + "\"";
    }

    private static String num(Object o) {
        double d = o instanceof Number n ? n.doubleValue() : 0;
        return Double.isFinite(d) ? Double.toString(d) : "0";
    }

    /** The particle's data as the server reads it (`particle_data`). */
    static String data(Object d) {
        if (d == null) return "null";
        if (d instanceof BlockData b) return "{\"block\":" + quote(b.getAsString()) + "}";
        if (d instanceof Particle.DustTransition t) return "{\"from\":" + t.getColor().asRGB() + ",\"to\":" + t.getToColor().asRGB() + ",\"scale\":" + t.getSize() + "}";
        if (d instanceof Particle.DustOptions o) return "{\"color\":" + o.getColor().asRGB() + ",\"scale\":" + o.getSize() + "}";
        if (d instanceof Particle.Spell s) return "{\"color\":" + s.getColor().asARGB() + ",\"power\":" + s.getPower() + "}";
        if (d instanceof Particle.Trail t) {
            Location to = t.getTarget();
            return "{\"target\":[" + to.getX() + "," + to.getY() + "," + to.getZ() + "],\"color\":" + t.getColor().asARGB() + ",\"duration\":" + t.getDuration() + "}";
        }
        if (d instanceof Color c) return "{\"color\":" + c.asARGB() + "}";
        if (d instanceof Float f) return "{\"scale\":" + f + "}";
        if (d instanceof Integer i) return "{\"delay\":" + i + "}";
        if (d instanceof ItemStack stack) {
            String item = Native.itemToJson(Conv.toNative(stack));
            return item == null ? "null" : "{\"item\":" + item + "}";
        }
        if (d instanceof org.bukkit.Vibration v) {
            Location to = v.getDestination() instanceof org.bukkit.Vibration.Destination.BlockDestination b ? b.getLocation() : null;
            return to == null ? "null" : "{\"pos\":[" + to.getX() + "," + to.getY() + "," + to.getZ() + "],\"ticks\":" + v.getArrivalTime() + "}";
        }
        return "null";
    }
}
