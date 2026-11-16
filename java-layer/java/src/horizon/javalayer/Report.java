package horizon.javalayer;

import com.google.gson.GsonBuilder;

import java.io.File;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.StandardCopyOption;
import java.util.*;
import java.util.concurrent.ConcurrentHashMap;
import java.util.concurrent.Executors;
import java.util.concurrent.ScheduledExecutorService;
import java.util.concurrent.TimeUnit;

/**
 * What each Java plugin did on this server, for people and tools: plugins/JavaLayer/report.json.
 * Per plugin: its state (loaded, enabled, enable-failed...), each error with the plugin it came from (found by class
 * loader on the stack), every call the layer can't answer yet, and the classes it looked for and didn't find (NMS,
 * CraftBukkit internals, missing libraries). The sweep and the MCP tools read it instead of guessing from the log.
 */
public final class Report {
    private Report() {}

    static final class Entry {
        String jar;
        String version;
        String state = "found";
        String detail;
        final List<Map<String, Object>> errors = new ArrayList<>();
        final Map<String, Integer> gaps = new TreeMap<>();
        final Set<String> missingClasses = new TreeSet<>();
        final Deque<String> log = new ArrayDeque<>();
        int errorCount;
    }

    private static final Map<String, Entry> PLUGINS = new ConcurrentHashMap<>();
    private static final Map<String, Integer> UNOWNED_GAPS = new ConcurrentHashMap<>();
    private static final List<Map<String, Object>> UNOWNED_ERRORS = Collections.synchronizedList(new ArrayList<>());
    /** The plugin being loaded or enabled on this thread: errors with no plugin frame on the stack go to it. */
    static final ThreadLocal<String> CURRENT = new ThreadLocal<>();
    private static volatile boolean dirty;
    private static ScheduledExecutorService writer;

    static Entry entry(String plugin) {
        return PLUGINS.computeIfAbsent(plugin, k -> new Entry());
    }

    static void found(String plugin, File jar, String version) {
        Entry e = entry(plugin);
        e.jar = jar == null ? null : jar.getName();
        e.version = version;
        touch();
    }

    static void state(String plugin, String state, String detail) {
        if (plugin == null) return;
        Entry e = entry(plugin);
        synchronized (e) {
            e.state = state;
            if (detail != null) e.detail = detail;
        }
        touch();
    }

    static String state(String plugin) {
        Entry e = PLUGINS.get(plugin);
        return e == null ? null : e.state;
    }

    /** The plugin whose code is on this thread's stack (the innermost), else the one being loaded here. */
    static String ownerNow() {
        try {
            String found = StackWalker.getInstance(StackWalker.Option.RETAIN_CLASS_REFERENCE).walk(s -> s
                    .map(f -> f.getDeclaringClass().getClassLoader())
                    .filter(l -> l instanceof Loader.PluginLoaderImpl)
                    .map(l -> ((Loader.PluginLoaderImpl) l).description.getName())
                    .findFirst().orElse(null));
            return found != null ? found : CURRENT.get();
        } catch (Throwable t) {
            return CURRENT.get();
        }
    }

    /** The plugin a throwable came from: the first frame (of it or a cause) whose class loader is a plugin's. */
    static String owner(Throwable t) {
        Set<Throwable> seen = Collections.newSetFromMap(new IdentityHashMap<>());
        for (Throwable c = t; c != null && seen.add(c); c = c.getCause()) {
            for (StackTraceElement el : c.getStackTrace()) {
                String loader = el.getClassLoaderName();
                if (loader != null && PLUGINS.containsKey(loader)) return loader;
            }
        }
        String now = ownerNow();
        return now;
    }

    /** A call the layer can't answer: recorded against the plugin that made it; the exception to throw. */
    static UnsupportedOperationException cant(String where) {
        gap(where);
        return new UnsupportedOperationException("Horizon's Java layer can't do " + where + " yet");
    }

    static void gap(String where) {
        String plugin = ownerNow();
        if (plugin == null) {
            UNOWNED_GAPS.merge(where, 1, Integer::sum);
        } else {
            Entry e = entry(plugin);
            synchronized (e) { e.gaps.merge(where, 1, Integer::sum); }
        }
        touch();
    }

    static void error(String context, Throwable t) {
        if (t == null) return;
        String plugin = owner(t);
        Map<String, Object> row = new LinkedHashMap<>();
        row.put("where", context);
        Throwable root = t;
        Set<Throwable> seen = Collections.newSetFromMap(new IdentityHashMap<>());
        while (root.getCause() != null && seen.add(root)) root = root.getCause();
        row.put("exception", t.getClass().getName());
        row.put("message", String.valueOf(t.getMessage()));
        if (root != t) {
            row.put("root", root.getClass().getName());
            row.put("root_message", String.valueOf(root.getMessage()));
        }
        row.put("frames", frames(root));
        List<String> missing = new ArrayList<>();
        seen.clear();
        for (Throwable c = t; c != null && seen.add(c); c = c.getCause()) {
            if (c instanceof NoClassDefFoundError || c instanceof ClassNotFoundException) {
                String m = String.valueOf(c.getMessage()).replace('/', '.');
                if (m.startsWith("Could not initialize class ")) missing.add("init:" + m.substring(27).trim());
                else missing.add(m.split(" ")[0]);
            }
            if (c instanceof NoSuchMethodError || c instanceof NoSuchFieldError || c instanceof AbstractMethodError) {
                missing.add("#" + c.getMessage());
            }
        }
        if (plugin == null) {
            if (UNOWNED_ERRORS.size() < 200) UNOWNED_ERRORS.add(row);
        } else {
            Entry e = entry(plugin);
            synchronized (e) {
                e.errorCount++;
                if (e.errors.size() < 25) e.errors.add(row);
                e.missingClasses.addAll(missing);
            }
        }
        touch();
    }

    /** A plugin's own log line (its logger, or [Name] on stdout): the last few kept, warnings first. */
    static void log(String plugin, int level, String text) {
        if (plugin == null || text == null || !PLUGINS.containsKey(plugin)) return;
        Entry e = PLUGINS.get(plugin);
        String line = (level >= 2 ? "SEVERE " : level == 1 ? "WARN " : "") + text.lines().findFirst().orElse("");
        synchronized (e) {
            e.log.addLast(line.length() > 300 ? line.substring(0, 300) : line);
            while (e.log.size() > 14) {
                // Drop the oldest plain line first, so warnings outlive chatter.
                Iterator<String> it = e.log.iterator();
                boolean dropped = false;
                while (it.hasNext()) {
                    String s = it.next();
                    if (!s.startsWith("SEVERE ") && !s.startsWith("WARN ")) { it.remove(); dropped = true; break; }
                }
                if (!dropped) e.log.removeFirst();
            }
        }
        touch();
    }

    private static List<String> frames(Throwable t) {
        List<String> out = new ArrayList<>();
        for (StackTraceElement el : t.getStackTrace()) {
            out.add((el.getClassLoaderName() == null ? "" : el.getClassLoaderName() + "//") + el.getClassName() + "." + el.getMethodName() + ":" + el.getLineNumber());
            if (out.size() >= 8) break;
        }
        return out;
    }

    private static void touch() {
        dirty = true;
        if (writer == null) start();
    }

    private static synchronized void start() {
        if (writer != null) return;
        writer = Executors.newSingleThreadScheduledExecutor(r -> {
            Thread t = new Thread(r, "JavaLayer report");
            t.setDaemon(true);
            return t;
        });
        writer.scheduleWithFixedDelay(() -> {
            if (dirty) write();
        }, 1, 1, TimeUnit.SECONDS);
    }

    static synchronized void write() {
        dirty = false;
        File dir = Loader.pluginsDir == null ? null : new File(Loader.pluginsDir, "JavaLayer");
        if (dir == null) return;
        try {
            dir.mkdirs();
            Map<String, Object> out = new LinkedHashMap<>();
            out.put("time", System.currentTimeMillis());
            Map<String, Object> plugins = new TreeMap<>(String.CASE_INSENSITIVE_ORDER);
            for (Map.Entry<String, Entry> me : PLUGINS.entrySet()) {
                Entry e = me.getValue();
                synchronized (e) {
                    Map<String, Object> row = new LinkedHashMap<>();
                    row.put("jar", e.jar);
                    row.put("version", e.version);
                    row.put("state", e.state);
                    if (e.detail != null) row.put("detail", e.detail);
                    row.put("error_count", e.errorCount);
                    row.put("errors", new ArrayList<>(e.errors));
                    row.put("gaps", new TreeMap<>(e.gaps));
                    row.put("missing_classes", new ArrayList<>(e.missingClasses));
                    row.put("log", new ArrayList<>(e.log));
                    plugins.put(me.getKey(), row);
                }
            }
            out.put("plugins", plugins);
            out.put("unowned_gaps", new TreeMap<>(UNOWNED_GAPS));
            synchronized (UNOWNED_ERRORS) { out.put("unowned_errors", new ArrayList<>(UNOWNED_ERRORS)); }
            File tmp = new File(dir, "report.json.tmp");
            Files.writeString(tmp.toPath(), new GsonBuilder().setPrettyPrinting().disableHtmlEscaping().serializeNulls().create().toJson(out), StandardCharsets.UTF_8);
            Files.move(tmp.toPath(), new File(dir, "report.json").toPath(), StandardCopyOption.REPLACE_EXISTING, StandardCopyOption.ATOMIC_MOVE);
        } catch (Throwable ignored) {
            dirty = true;
        }
    }
}
