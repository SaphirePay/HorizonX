package horizon.javalayer;

import org.bukkit.NamespacedKey;
import org.bukkit.configuration.ConfigurationSection;
import org.bukkit.configuration.file.YamlConfiguration;
import org.bukkit.persistence.PersistentDataAdapterContext;
import org.bukkit.persistence.PersistentDataContainer;
import org.bukkit.persistence.PersistentDataType;

import java.io.File;
import java.lang.reflect.Proxy;
import java.util.*;
import java.util.concurrent.ConcurrentHashMap;

/**
 * PersistentDataContainers the server has no place for (worlds, chunks): kept by the layer in
 * `<world>/data/horizon-pdc.yml`, one section per holder, saved shortly after each change. Values are kept in their
 * primitive form, as Paper keeps them in NBT.
 */
public final class FilePdc {
    private FilePdc() {}

    private static final Map<String, Map<String, Object>> HOLDERS = new ConcurrentHashMap<>();
    private static final Map<String, File> FILES = new ConcurrentHashMap<>();
    private static volatile boolean saving;

    /** The container of a holder (`world:<name>`, `chunk:<world>:<x>:<z>`), kept in that world's folder. */
    static PersistentDataContainer of(org.bukkit.World world, String holder) {
        File file = new File(world.getWorldFolder(), "data/horizon-pdc.yml");
        FILES.putIfAbsent(holder, file);
        Map<String, Object> values = HOLDERS.computeIfAbsent(holder, h -> load(file, h));
        return container(values, () -> save(file));
    }

    private static Map<String, Object> load(File file, String holder) {
        Map<String, Object> out = new ConcurrentHashMap<>();
        if (!file.isFile()) return out;
        YamlConfiguration yaml = YamlConfiguration.loadConfiguration(file);
        ConfigurationSection s = yaml.getConfigurationSection(holder.replace('.', '_'));
        if (s != null) for (String k : s.getKeys(false)) out.put(k.replace('|', ':'), plain(s.get(k)));
        return out;
    }

    private static Object plain(Object v) {
        if (v instanceof ConfigurationSection s) {
            Map<String, Object> m = new ConcurrentHashMap<>();
            for (String k : s.getKeys(false)) m.put(k.replace('|', ':'), plain(s.get(k)));
            return m;
        }
        return v;
    }

    private static synchronized void save(File file) {
        if (saving) return;
        saving = true;
        Thread t = new Thread(() -> {
            try {
                Thread.sleep(500);
            } catch (InterruptedException ignored) {}
            synchronized (FilePdc.class) {
                saving = false;
                YamlConfiguration yaml = new YamlConfiguration();
                for (Map.Entry<String, Map<String, Object>> e : HOLDERS.entrySet()) {
                    if (!file.equals(FILES.get(e.getKey()))) continue;
                    String section = e.getKey().replace('.', '_');
                    for (Map.Entry<String, Object> v : e.getValue().entrySet()) yaml.set(section + "." + v.getKey().replace(':', '|'), yamlValue(v.getValue()));
                }
                try {
                    file.getParentFile().mkdirs();
                    yaml.save(file);
                } catch (Exception ex) {
                    Logs.warning("could not save " + file + ": " + ex);
                }
            }
        }, "JavaLayer PDC save");
        t.setDaemon(true);
        t.start();
    }

    private static Object yamlValue(Object v) {
        if (v instanceof byte[] b) { List<Integer> l = new ArrayList<>(); for (byte x : b) l.add((int) x); return l; }
        if (v instanceof int[] a) { List<Integer> l = new ArrayList<>(); for (int x : a) l.add(x); return l; }
        if (v instanceof long[] a) { List<Long> l = new ArrayList<>(); for (long x : a) l.add(x); return l; }
        if (v instanceof Map<?, ?> m) {
            Map<String, Object> out = new LinkedHashMap<>();
            for (Map.Entry<?, ?> e : m.entrySet()) out.put(String.valueOf(e.getKey()).replace(':', '|'), yamlValue(e.getValue()));
            return out;
        }
        return v;
    }

    @SuppressWarnings("unchecked")
    private static Object toType(Object stored, Class<?> primitive) {
        if (stored == null) return null;
        if (primitive.isInstance(stored)) return stored;
        if (stored instanceof Number n) {
            if (primitive == Integer.class) return n.intValue();
            if (primitive == Long.class) return n.longValue();
            if (primitive == Double.class) return n.doubleValue();
            if (primitive == Float.class) return n.floatValue();
            if (primitive == Short.class) return n.shortValue();
            if (primitive == Byte.class) return n.byteValue();
            if (primitive == Boolean.class) return n.intValue() != 0;
        }
        if (stored instanceof List<?> l) {
            if (primitive == byte[].class) { byte[] b = new byte[l.size()]; for (int i = 0; i < b.length; i++) b[i] = ((Number) l.get(i)).byteValue(); return b; }
            if (primitive == int[].class) { int[] b = new int[l.size()]; for (int i = 0; i < b.length; i++) b[i] = ((Number) l.get(i)).intValue(); return b; }
            if (primitive == long[].class) { long[] b = new long[l.size()]; for (int i = 0; i < b.length; i++) b[i] = ((Number) l.get(i)).longValue(); return b; }
        }
        if (primitive == PersistentDataContainer.class && stored instanceof Map<?, ?> m) return container((Map<String, Object>) m, () -> {});
        if (primitive == PersistentDataContainer[].class && stored instanceof List<?> l) {
            PersistentDataContainer[] out = new PersistentDataContainer[l.size()];
            for (int i = 0; i < out.length; i++) out[i] = container((Map<String, Object>) l.get(i), () -> {});
            return out;
        }
        return null;
    }

    /** Values made by an adapter context stay plain maps inside: what a nested container holds. */
    public interface Backed {
        Map<String, Object> horizonValues();
    }

    static PersistentDataContainer container(Map<String, Object> values, Runnable changed) {
        PersistentDataAdapterContext[] context = new PersistentDataAdapterContext[1];
        context[0] = () -> container(new ConcurrentHashMap<>(), () -> {});
        return (PersistentDataContainer) Proxy.newProxyInstance(FilePdc.class.getClassLoader(), new Class<?>[]{PersistentDataContainer.class, Backed.class}, (proxy, m, args) -> {
            String name = m.getName();
            int n = args == null ? 0 : args.length;
            switch (name) {
                case "horizonValues": return values;
                case "set": {
                    NamespacedKey key = (NamespacedKey) args[0];
                    PersistentDataType<Object, Object> type = (PersistentDataType<Object, Object>) args[1];
                    Object prim = type.toPrimitive(args[2], context[0]);
                    if (prim instanceof Backed b) prim = b.horizonValues();
                    if (prim instanceof PersistentDataContainer[] arr) { List<Object> l = new ArrayList<>(); for (PersistentDataContainer c : arr) l.add(c instanceof Backed b ? b.horizonValues() : Map.of()); prim = l; }
                    values.put(key.toString(), prim);
                    changed.run();
                    return null;
                }
                case "get":
                case "getOrDefault": {
                    NamespacedKey key = (NamespacedKey) args[0];
                    PersistentDataType<Object, Object> type = (PersistentDataType<Object, Object>) args[1];
                    Object prim = toType(values.get(key.toString()), type.getPrimitiveType());
                    if (prim == null) return name.equals("getOrDefault") ? args[2] : null;
                    return type.fromPrimitive(prim, context[0]);
                }
                case "has":
                    if (n == 1) return values.containsKey(args[0].toString());
                    if (n == 2) return toType(values.get(args[0].toString()), ((PersistentDataType<?, ?>) args[1]).getPrimitiveType()) != null;
                    break;
                case "remove":
                    if (values.remove(args[0].toString()) != null) changed.run();
                    return null;
                case "getKeys": {
                    Set<NamespacedKey> keys = new HashSet<>();
                    for (String k : values.keySet()) { NamespacedKey key = NamespacedKey.fromString(k); if (key != null) keys.add(key); }
                    return keys;
                }
                case "isEmpty": return values.isEmpty();
                case "getSize": return values.size();
                case "getAdapterContext": return context[0];
                case "copyTo":
                    if (args[0] instanceof Backed b) {
                        boolean replace = n > 1 && Boolean.TRUE.equals(args[1]);
                        for (Map.Entry<String, Object> e : values.entrySet()) if (replace || !b.horizonValues().containsKey(e.getKey())) b.horizonValues().put(e.getKey(), e.getValue());
                    }
                    return null;
                case "serializeToBytes": return YamlConfiguration.class.getName().getBytes();
                case "readFromBytes": return null;
                case "toString": return "PersistentDataContainer" + values;
                case "hashCode": return values.hashCode();
                case "equals": return args[0] instanceof Backed b && b.horizonValues().equals(values);
                default: break;
            }
            if (m.isDefault()) return java.lang.reflect.InvocationHandler.invokeDefault(proxy, m, args);
            throw Report.cant("PersistentDataContainer." + name);
        });
    }
}
