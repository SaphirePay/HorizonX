package horizon.javalayer;

import org.bukkit.Bukkit;
import org.bukkit.GameMode;
import org.bukkit.Location;
import org.bukkit.World;
import org.bukkit.command.CommandSender;
import org.bukkit.entity.Entity;
import org.bukkit.entity.EntityType;
import org.bukkit.entity.Player;

import java.util.ArrayList;
import java.util.Collections;
import java.util.Comparator;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Locale;
import java.util.Map;
import java.util.UUID;
import java.util.concurrent.ThreadLocalRandom;

/**
 * Server.selectEntities: the game's entity selectors (`@e[type=cow,limit=1]`, `@p`, a player name, a UUID) as
 * vanilla's EntitySelector reads them, from the sender's place (the console's is the overworld spawn). Arguments
 * the layer can't test (scores, advancements, predicates, nbt, team, rotations) don't narrow the result.
 */
final class Selectors {
    private Selectors() {}

    static List<Entity> select(CommandSender sender, String text) {
        if (text == null) throw new IllegalArgumentException("Selector cannot be null");
        String s = text.trim();
        if (!s.startsWith("@")) {
            // A player's name, or an entity's UUID.
            Player p = Bukkit.getPlayerExact(s);
            if (p != null) return new ArrayList<>(List.of(p));
            try {
                Entity e = Bukkit.getEntity(UUID.fromString(s));
                if (e != null) return new ArrayList<>(List.of(e));
            } catch (IllegalArgumentException ignored) {
                // not a UUID
            }
            return new ArrayList<>();
        }
        if (s.length() < 2) throw new IllegalArgumentException("Invalid selector " + text);
        char kind = s.charAt(1);
        Map<String, List<String>> args = new LinkedHashMap<>();
        String rest = s.substring(2).trim();
        if (!rest.isEmpty()) {
            if (!rest.startsWith("[") || !rest.endsWith("]")) throw new IllegalArgumentException("Invalid selector " + text);
            for (String part : split(rest.substring(1, rest.length() - 1))) {
                int eq = part.indexOf('=');
                if (eq <= 0) throw new IllegalArgumentException("Invalid selector argument " + part);
                args.computeIfAbsent(part.substring(0, eq).trim().toLowerCase(Locale.ROOT), k -> new ArrayList<>()).add(unquote(part.substring(eq + 1).trim()));
            }
        }

        Entity self = sender instanceof Entity e ? e : null;
        Location origin = self != null ? self.getLocation() : consoleOrigin();
        if (origin == null) return new ArrayList<>();
        Double x = num(args, "x"), y = num(args, "y"), z = num(args, "z");
        origin = new Location(origin.getWorld(), x != null ? x : origin.getX(), y != null ? y : origin.getY(), z != null ? z : origin.getZ());

        int limit;
        String sort;
        List<? extends Entity> pool;
        boolean players;
        switch (kind) {
            case 'a' -> { players = true; limit = Integer.MAX_VALUE; sort = "arbitrary"; }
            case 'p' -> { players = true; limit = 1; sort = "nearest"; }
            case 'r' -> { players = true; limit = 1; sort = "random"; }
            case 'e' -> { players = false; limit = Integer.MAX_VALUE; sort = "arbitrary"; }
            case 'n' -> { players = false; limit = 1; sort = "nearest"; }
            case 's' -> {
                return self == null ? new ArrayList<>() : filter(new ArrayList<>(List.of(self)), args, origin, self) ? new ArrayList<>(List.of(self)) : new ArrayList<>();
            }
            default -> throw new IllegalArgumentException("Unknown selector type @" + kind);
        }
        if (args.containsKey("limit")) limit = Integer.parseInt(args.get("limit").get(0));
        if (args.containsKey("sort")) sort = args.get("sort").get(0).toLowerCase(Locale.ROOT);
        // A position or a distance limits the search to the origin's world (EntitySelector.currentWorldOnly).
        boolean here = x != null || y != null || z != null || args.containsKey("distance") || args.containsKey("dx") || args.containsKey("dy") || args.containsKey("dz");
        List<Entity> all = new ArrayList<>();
        if (players) {
            for (Player p : Bukkit.getOnlinePlayers()) if (!here || p.getWorld().equals(origin.getWorld())) all.add(p);
        } else {
            for (World w : here ? List.of(origin.getWorld()) : Bukkit.getWorlds()) all.addAll(w.getEntities());
        }
        // Each entity once (by UUID), in the worlds' order.
        Map<UUID, Entity> unique = new LinkedHashMap<>();
        for (Entity e : all) unique.putIfAbsent(e.getUniqueId(), e);
        List<Entity> out = new ArrayList<>();
        for (Entity e : unique.values()) if (filter(List.of(e), args, origin, self)) out.add(e);
        Location from = origin;
        switch (sort) {
            case "nearest" -> out.sort(Comparator.comparingDouble(e -> dist2(e, from)));
            case "furthest" -> out.sort(Comparator.comparingDouble((Entity e) -> dist2(e, from)).reversed());
            case "random" -> Collections.shuffle(out, ThreadLocalRandom.current());
            default -> { }
        }
        return out.size() > limit ? new ArrayList<>(out.subList(0, limit)) : out;
    }

    private static Location consoleOrigin() {
        List<World> worlds = Bukkit.getWorlds();
        if (worlds.isEmpty()) return null;
        Location spawn = worlds.get(0).getSpawnLocation();
        return new Location(spawn.getWorld(), spawn.getBlockX(), spawn.getBlockY(), spawn.getBlockZ());
    }

    private static double dist2(Entity e, Location from) {
        Location l = e.getLocation();
        if (!l.getWorld().equals(from.getWorld())) return Double.MAX_VALUE;
        return l.distanceSquared(from);
    }

    private static boolean filter(List<Entity> one, Map<String, List<String>> args, Location origin, Entity self) {
        Entity e = one.get(0);
        for (Map.Entry<String, List<String>> a : args.entrySet()) {
            for (String v : a.getValue()) {
                boolean not = v.startsWith("!");
                String value = not ? v.substring(1) : v;
                boolean ok;
                switch (a.getKey()) {
                    case "type" -> {
                        if (value.startsWith("#")) continue;
                        String key = value.contains(":") ? value : "minecraft:" + value;
                        ok = e.getType() != EntityType.UNKNOWN && e.getType().getKey().toString().equals(key.toLowerCase(Locale.ROOT));
                    }
                    case "name" -> ok = e instanceof Player p ? p.getName().equals(value) : value.equals(e.getCustomName() != null ? org.bukkit.ChatColor.stripColor(e.getCustomName()) : e.getName());
                    case "tag" -> ok = value.isEmpty() ? e.getScoreboardTags().isEmpty() : e.getScoreboardTags().contains(value);
                    case "gamemode" -> ok = e instanceof Player p && p.getGameMode() == GameMode.valueOf(value.toUpperCase(Locale.ROOT));
                    case "level" -> ok = e instanceof Player p && range(value, p.getLevel());
                    case "distance" -> ok = e.getWorld().equals(origin.getWorld()) && range(value, e.getLocation().distance(origin));
                    case "dx", "dy", "dz" -> ok = inVolume(e, args, origin);
                    default -> { continue; }
                }
                if (ok == not) return false;
            }
        }
        return true;
    }

    /** dx/dy/dz: the entity's box meets the volume from the origin to origin + (dx, dy, dz) (each block grown by 1). */
    private static boolean inVolume(Entity e, Map<String, List<String>> args, Location o) {
        double dx = args.containsKey("dx") ? Double.parseDouble(args.get("dx").get(0)) : 0;
        double dy = args.containsKey("dy") ? Double.parseDouble(args.get("dy").get(0)) : 0;
        double dz = args.containsKey("dz") ? Double.parseDouble(args.get("dz").get(0)) : 0;
        org.bukkit.util.BoundingBox volume = new org.bukkit.util.BoundingBox(o.getX(), o.getY(), o.getZ(), o.getX() + dx + 1, o.getY() + dy + 1, o.getZ() + dz + 1);
        return e.getWorld().equals(o.getWorld()) && volume.overlaps(e.getBoundingBox());
    }

    private static boolean range(String text, double v) {
        int dots = text.indexOf("..");
        if (dots < 0) return v == Double.parseDouble(text);
        String lo = text.substring(0, dots), hi = text.substring(dots + 2);
        return (lo.isEmpty() || v >= Double.parseDouble(lo)) && (hi.isEmpty() || v <= Double.parseDouble(hi));
    }

    private static Double num(Map<String, List<String>> args, String key) {
        return args.containsKey(key) ? Double.valueOf(args.get(key).get(0)) : null;
    }

    private static String unquote(String v) {
        boolean not = v.startsWith("!");
        String body = not ? v.substring(1) : v;
        if (body.length() >= 2 && (body.startsWith("\"") && body.endsWith("\"") || body.startsWith("'") && body.endsWith("'"))) body = body.substring(1, body.length() - 1);
        return not ? "!" + body : body;
    }

    /** Splits `a=1,b={x:1,y:2},c="q,r"` at its top-level commas. */
    private static List<String> split(String s) {
        List<String> out = new ArrayList<>();
        int depth = 0, start = 0;
        char quote = 0;
        for (int i = 0; i < s.length(); i++) {
            char c = s.charAt(i);
            if (quote != 0) {
                if (c == '\\') i++;
                else if (c == quote) quote = 0;
            } else if (c == '"' || c == '\'') {
                quote = c;
            } else if (c == '{' || c == '[') {
                depth++;
            } else if (c == '}' || c == ']') {
                depth--;
            } else if (c == ',' && depth == 0) {
                if (!s.substring(start, i).isBlank()) out.add(s.substring(start, i).trim());
                start = i + 1;
            }
        }
        if (!s.substring(start).isBlank()) out.add(s.substring(start).trim());
        return out;
    }
}
