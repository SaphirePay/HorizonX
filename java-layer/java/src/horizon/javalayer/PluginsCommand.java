package horizon.javalayer;

import com.google.gson.JsonArray;
import com.google.gson.JsonElement;
import com.google.gson.JsonObject;
import com.google.gson.JsonParser;
import org.bukkit.command.Command;
import org.bukkit.command.CommandSender;
import org.bukkit.plugin.java.JavaPlugin;

import java.util.ArrayList;
import java.util.Comparator;
import java.util.List;

/**
 * `/plugins` (`/pl`) with the Java layer on: the engine's own lists only the plugins it loaded itself, where the
 * layer is one plugin (JavaLayer) and the jars it runs are not seen. A plugin's command takes the word from the
 * engine's (paper_events::register_plugin_command), so the layer prints both: the engine's first line, then the
 * HorizonsX plugins and the Java plugins each under their own heading, as Paper splits Paper and Bukkit plugins.
 * Green is enabled, red is not (Paper's colours). Every player may run it, as on Paper (bukkit.command.plugins is
 * default true) and as the engine's own.
 */
final class PluginsCommand extends Command {
    static final PluginsCommand INSTANCE = new PluginsCommand();

    private PluginsCommand() {
        super("plugins", "Gets a list of plugins running on the server", "/plugins", List.of("pl"));
    }

    /** After every Java plugin is enabled: a plugin with a `/plugins` of its own keeps the word (Paper's fallback rule). */
    static void register() {
        if (Commands.MAP.getCommand("plugins") == null) Commands.MAP.register("bukkit", INSTANCE);
    }

    @Override
    public boolean execute(CommandSender sender, String label, String[] args) {
        for (String line : lines()) sender.sendMessage(line);
        return true;
    }

    @Override
    public List<String> tabComplete(CommandSender sender, String alias, String[] args) {
        return List.of();
    }

    static List<String> lines() {
        List<String> horizon = new ArrayList<>();
        try {
            JsonElement ids = JsonParser.parseString(String.valueOf(Native.query("{\"op\":\"lead_mods\"}")));
            if (ids instanceof JsonArray array) for (JsonElement id : array) horizon.add("§a" + id.getAsString());
        } catch (RuntimeException e) {
            Logs.warning("[JavaLayer] /plugins: the HorizonsX plugin list: " + e);
        }
        horizon.sort(Comparator.comparing(name -> name.substring(2).toLowerCase(java.util.Locale.ROOT)));
        List<JavaPlugin> plugins = new ArrayList<>(Loader.all());
        plugins.sort(Comparator.comparing(p -> p.getName().toLowerCase(java.util.Locale.ROOT)));
        List<String> java = new ArrayList<>();
        for (JavaPlugin p : plugins) java.add((p.isEnabled() ? "§a" : "§c") + p.getName());

        List<String> out = new ArrayList<>();
        String fork = forkLine();
        if (fork != null) out.add(fork);
        out.add("§fℹ Server Plugins (" + (horizon.size() + java.size()) + "):");
        out.add("§9HorizonsX Plugins:");
        out.add(" §8- " + (horizon.isEmpty() ? "§7none" : String.join("§f, ", horizon)));
        out.add("§6Java Plugins:");
        out.add(" §8- " + (java.isEmpty() ? "§7none" : String.join("§f, ", java)));
        return out;
    }

    /** The engine's first `/plugins` line (paper_fam_core::engine_lines), from its `version` answer. */
    private static String forkLine() {
        try {
            JsonObject v = JsonParser.parseString(String.valueOf(Native.query("{\"op\":\"version\"}"))).getAsJsonObject();
            String fork = v.has("fork") ? v.get("fork").getAsString() : null;
            if (fork == null) return null;
            String engine = v.has("engine") ? v.get("engine").getAsString() : "";
            String build = v.has("build") ? v.get("build").getAsString() : "local";
            String built = v.has("built") && !v.get("built").getAsString().isEmpty() ? v.get("built").getAsString() : "unknown day";
            String source = v.has("horizon") && !v.get("horizon").getAsString().isEmpty() ? v.get("horizon").getAsString() : "?";
            return "§fThis server is running §b" + fork + "§f " + engine + " (Paper API in Rust, build §a" + build + "§f, " + built + ", Horizon " + source + ")";
        } catch (RuntimeException e) {
            return null;
        }
    }
}
