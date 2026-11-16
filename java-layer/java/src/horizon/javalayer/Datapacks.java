package horizon.javalayer;

import io.papermc.paper.datapack.DatapackRegistrar;
import org.bukkit.Bukkit;
import org.bukkit.plugin.Plugin;

import java.io.IOException;
import java.net.URI;
import java.nio.file.*;
import java.util.HashMap;
import java.util.Map;
import java.util.function.Consumer;

/**
 * Paper's DATAPACK_DISCOVERY lifecycle event: a plugin's data pack (a folder of its jar, or on disk) is copied into
 * the main world's datapacks/ as `<plugin>_<id>` and enabled, so its recipes, tags, enchantments... load as on Paper.
 */
public final class Datapacks {
    private Datapacks() {}

    static Object registrar(Plugin plugin) {
        Map<String, Object> discovered = new HashMap<>();
        return java.lang.reflect.Proxy.newProxyInstance(Datapacks.class.getClassLoader(), new Class<?>[]{DatapackRegistrar.class}, (proxy, m, args) -> {
            String name = m.getName();
            switch (name) {
                case "discoverPack": {
                    Object where = args[0] instanceof io.papermc.paper.plugin.configuration.PluginMeta ? args[1] : args[0];
                    String id = String.valueOf(args[0] instanceof io.papermc.paper.plugin.configuration.PluginMeta ? args[2] : args[1]);
                    String folder = (plugin.getName() + "_" + id).toLowerCase(java.util.Locale.ROOT).replaceAll("[^a-z0-9_.-]", "_");
                    copy(where, folder);
                    Bukkit.getScheduler().runTask(plugin, () -> Bukkit.dispatchCommand(Bukkit.getConsoleSender(), "datapack enable \"file/" + folder + "\""));
                    Object pack = Services.inert(io.papermc.paper.datapack.DiscoveredDatapack.class, folder);
                    discovered.put(id, pack);
                    return pack;
                }
                case "hasPackDiscovered": return discovered.containsKey(args[0]);
                case "getDiscoveredPack": return discovered.get(args[0]);
                case "removeDiscoveredPack": return discovered.remove(args[0]) != null;
                case "getDiscoveredPacks": return new HashMap<>(discovered);
                case "toString": return "DatapackRegistrar[" + plugin.getName() + "]";
                case "hashCode": return System.identityHashCode(proxy);
                case "equals": return proxy == args[0];
                default: break;
            }
            if (m.isDefault()) return java.lang.reflect.InvocationHandler.invokeDefault(proxy, m, args);
            return null;
        });
    }

    private static void copy(Object where, String folder) throws IOException {
        Path world = Bukkit.getWorlds().isEmpty() ? Paths.get("world") : Bukkit.getWorlds().get(0).getWorldFolder().toPath();
        Path target = world.resolve("datapacks").resolve(folder);
        Path source;
        FileSystem opened = null;
        if (where instanceof URI uri && "jar".equals(uri.getScheme())) {
            try {
                opened = FileSystems.newFileSystem(uri, Map.of());
            } catch (FileSystemAlreadyExistsException e) {
                opened = null;
            }
            source = Paths.get(uri);
        } else {
            source = where instanceof Path p ? p : Paths.get((URI) where);
        }
        try (var walk = Files.walk(source)) {
            for (Path p : (Iterable<Path>) walk::iterator) {
                Path rel = target.resolve(source.relativize(p).toString());
                if (Files.isDirectory(p)) Files.createDirectories(rel);
                else {
                    Files.createDirectories(rel.getParent());
                    Files.copy(p, rel, StandardCopyOption.REPLACE_EXISTING);
                }
            }
        } finally {
            if (opened != null) opened.close();
        }
    }
}
