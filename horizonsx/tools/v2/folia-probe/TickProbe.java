package hx;

import java.lang.reflect.Field;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.List;
import java.util.function.Consumer;
import org.bukkit.Bukkit;
import org.bukkit.World;
import org.bukkit.command.Command;
import org.bukkit.command.CommandSender;
import org.bukkit.plugin.java.JavaPlugin;

/**
 * HorizonsX's bench probe for Folia: `hxtick` prints Folia's own per-region tick times over the last 15 s
 * (TickRegionScheduler's report, the numbers Folia's /tps averages), as average, median, p95 and p99 of the
 * busiest region, and of every region's ticks pooled. Read-only: it only looks at Folia's statistics.
 */
public final class TickProbe extends JavaPlugin {
    @Override
    public boolean onCommand(CommandSender sender, Command command, String label, String[] args) {
        StringBuilder out = new StringBuilder("HXTICK");
        try {
            long now = System.nanoTime();
            List<long[]> perRegion = new ArrayList<>();
            double lowestTps = 20.0;
            for (World world : Bukkit.getWorlds()) {
                Object level = world.getClass().getMethod("getHandle").invoke(world);
                Object regioniser = field(level, "regioniser");
                List<Object> regions = new ArrayList<>();
                Consumer<Object> add = regions::add;
                regioniser.getClass().getMethod("computeForAllRegionsUnsynchronised", Consumer.class).invoke(regioniser, add);
                for (Object region : regions) {
                    Object data = call(region, "getData");
                    Object handle = call(data, "getRegionSchedulingHandle");
                    Object report = handle.getClass().getMethod("getTickReport15s", long.class).invoke(handle, now);
                    if (report == null) {
                        continue;
                    }
                    Object tps = call(call(report, "tpsData"), "segmentAll");
                    lowestTps = Math.min(lowestTps, (double) call(tps, "average"));
                    Object perTick = call(report, "timePerTickData");
                    long[] raw = (long[]) call(perTick, "rawData");
                    if (raw != null && raw.length > 0) {
                        perRegion.add(raw);
                    }
                }
            }
            out.append(" regions=").append(perRegion.size()).append(String.format(" lowest_tps=%.2f", lowestTps));
            long[] worst = null;
            double worstAverage = -1;
            int total = 0;
            for (long[] raw : perRegion) {
                double average = Arrays.stream(raw).average().orElse(0);
                if (average > worstAverage) {
                    worstAverage = average;
                    worst = raw;
                }
                total += raw.length;
            }
            long[] pooled = new long[total];
            int at = 0;
            for (long[] raw : perRegion) {
                System.arraycopy(raw, 0, pooled, at, raw.length);
                at += raw.length;
            }
            if (worst != null) {
                stats(out, "worst", worst);
                stats(out, "all", pooled);
            }
        } catch (Throwable t) {
            out.append(" error=").append(t);
        }
        getLogger().info(out.toString());
        sender.sendMessage(out.toString());
        return true;
    }

    private static void stats(StringBuilder out, String name, long[] nanos) {
        long[] sorted = nanos.clone();
        Arrays.sort(sorted);
        double average = Arrays.stream(sorted).average().orElse(0) / 1e6;
        out.append(String.format(" %s_ticks=%d %s_avg=%.2f %s_p50=%.2f %s_p95=%.2f %s_p99=%.2f %s_max=%.2f",
            name, sorted.length, name, average, name, pick(sorted, 0.5), name, pick(sorted, 0.95), name,
            pick(sorted, 0.99), name, sorted[sorted.length - 1] / 1e6));
    }

    private static double pick(long[] sorted, double q) {
        int i = Math.min(sorted.length - 1, (int) (sorted.length * q));
        return sorted[i] / 1e6;
    }

    private static Object call(Object on, String method) throws ReflectiveOperationException {
        var m = on.getClass().getMethod(method);
        m.setAccessible(true);
        return m.invoke(on);
    }

    private static Object field(Object on, String name) throws ReflectiveOperationException {
        for (Class<?> c = on.getClass(); c != null; c = c.getSuperclass()) {
            try {
                Field f = c.getDeclaredField(name);
                f.setAccessible(true);
                return f.get(on);
            } catch (NoSuchFieldException e) {
                // the superclass, then
            }
        }
        throw new NoSuchFieldException(name);
    }
}
