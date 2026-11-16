package horizon.javalayer;

import org.bukkit.scheduler.BukkitRunnable;
import org.bukkit.scheduler.BukkitTask;

import java.lang.reflect.Field;

/** `scheduler.runTask(plugin, bukkitRunnable)`: the runnable learns its task, as CraftScheduler does it. */
final class Tasks {
    private Tasks() {}

    private static final Field TASK;

    static {
        Field f = null;
        try {
            f = BukkitRunnable.class.getDeclaredField("task");
            f.setAccessible(true);
        } catch (ReflectiveOperationException ignored) {}
        TASK = f;
    }

    static void bind(BukkitRunnable runnable, BukkitTask task) {
        if (TASK == null) return;
        try {
            if (TASK.get(runnable) == null) TASK.set(runnable, task);
        } catch (IllegalAccessException ignored) {}
    }
}
