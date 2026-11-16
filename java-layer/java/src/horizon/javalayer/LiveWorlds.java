package horizon.javalayer;

import java.util.AbstractMap;
import java.util.LinkedHashMap;
import java.util.Locale;
import java.util.Map;
import java.util.Set;

/**
 * CraftServer's `worlds` field (Paper: a map of the worlds by lower-case name), read by plugins through reflection
 * (WorldEdit's version adapter checks for it when it loads): the server's worlds as they are each time it is read.
 */
public final class LiveWorlds extends AbstractMap<String, org.bukkit.World> {
    @Override
    public Set<Map.Entry<String, org.bukkit.World>> entrySet() {
        Map<String, org.bukkit.World> now = new LinkedHashMap<>();
        for (org.bukkit.World w : org.bukkit.Bukkit.getWorlds()) now.put(w.getName().toLowerCase(Locale.ROOT), w);
        return now.entrySet();
    }

    @Override
    public org.bukkit.World get(Object name) {
        return name instanceof String s ? org.bukkit.Bukkit.getWorld(s) : null;
    }
}
