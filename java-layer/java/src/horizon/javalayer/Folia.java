package horizon.javalayer;

import io.papermc.paper.threadedregions.scheduler.*;
import org.bukkit.World;
import org.bukkit.entity.Entity;
import org.bukkit.plugin.Plugin;
import org.bukkit.scheduler.BukkitTask;

import java.util.concurrent.TimeUnit;
import java.util.concurrent.atomic.AtomicReference;
import java.util.function.Consumer;

/**
 * Paper's region schedulers (Folia's API, which Paper has too): on this server every region is the server thread,
 * so they are Bukkit's scheduler underneath; an entity's tasks stop (and say "retired") once it is gone.
 */
// Public, as Paper's are: plugins call these through reflection (getClass().getMethod(...).invoke).
public final class Folia {
    private Folia() {}

    static final GlobalRegionScheduler GLOBAL = new Global();
    static final RegionScheduler REGION = new Region();
    static final AsyncScheduler ASYNC = new Async();

    static EntityScheduler entity(Entity entity) { return new ForEntity(entity); }

    /** CraftEntity.getScheduler() (patched in by the build's PatchServer): the layer's scheduler for the entity. */
    public static EntityScheduler entityScheduler(Entity entity) { return new ForEntity(entity); }

    /** One task: Bukkit's underneath. */
    public static final class Task implements ScheduledTask {
        final Plugin plugin;
        final boolean repeating;
        final AtomicReference<BukkitTask> bukkit = new AtomicReference<>();
        volatile ExecutionState state = ExecutionState.IDLE;

        Task(Plugin plugin, boolean repeating) {
            this.plugin = plugin;
            this.repeating = repeating;
        }

        @Override public Plugin getOwningPlugin() { return plugin; }
        @Override public boolean isRepeatingTask() { return repeating; }
        @Override public ExecutionState getExecutionState() { return state; }

        @Override
        public CancelledState cancel() {
            if (state == ExecutionState.CANCELLED || state == ExecutionState.CANCELLED_RUNNING) return CancelledState.CANCELLED_ALREADY;
            if (state == ExecutionState.FINISHED) return CancelledState.ALREADY_EXECUTED;
            boolean running = state == ExecutionState.RUNNING;
            state = running ? ExecutionState.CANCELLED_RUNNING : ExecutionState.CANCELLED;
            BukkitTask t = bukkit.get();
            if (t != null) t.cancel();
            return running ? (repeating ? CancelledState.NEXT_RUNS_CANCELLED : CancelledState.RUNNING) : CancelledState.CANCELLED_BY_CALLER;
        }

        Runnable body(Consumer<ScheduledTask> consumer, Runnable gate) {
            return () -> {
                if (state == ExecutionState.CANCELLED) return;
                if (gate != null) {
                    gate.run();
                    if (state == ExecutionState.CANCELLED) return;
                }
                state = ExecutionState.RUNNING;
                try {
                    consumer.accept(this);
                } finally {
                    if (state == ExecutionState.RUNNING) state = repeating ? ExecutionState.IDLE : ExecutionState.FINISHED;
                    else if (state == ExecutionState.CANCELLED_RUNNING) state = ExecutionState.CANCELLED;
                }
            };
        }
    }

    static Task schedule(Plugin plugin, Consumer<ScheduledTask> consumer, long delay, long period, boolean sync, Runnable gate) {
        Task task = new Task(plugin, period > 0);
        Runnable body = task.body(consumer, gate);
        Scheduler s = Scheduler.INSTANCE;
        BukkitTask t = period > 0
            ? (sync ? s.runTaskTimer(plugin, body, Math.max(1, delay), period) : s.runTaskTimerAsynchronously(plugin, body, delay, period))
            : (sync ? s.runTaskLater(plugin, body, Math.max(1, delay)) : s.runTaskLaterAsynchronously(plugin, body, delay));
        task.bukkit.set(t);
        return task;
    }

    public static final class Global implements GlobalRegionScheduler {
        @Override public void execute(Plugin plugin, Runnable run) { Scheduler.INSTANCE.runTask(plugin, run); }
        @Override public ScheduledTask run(Plugin plugin, Consumer<ScheduledTask> task) { return schedule(plugin, task, 1, -1, true, null); }
        @Override public ScheduledTask runDelayed(Plugin plugin, Consumer<ScheduledTask> task, long delay) { return schedule(plugin, task, delay, -1, true, null); }
        @Override public ScheduledTask runAtFixedRate(Plugin plugin, Consumer<ScheduledTask> task, long delay, long period) { return schedule(plugin, task, delay, period, true, null); }
        @Override public void cancelTasks(Plugin plugin) { Scheduler.INSTANCE.cancelTasks(plugin); }
    }

    public static final class Region implements RegionScheduler {
        @Override public void execute(Plugin plugin, World world, int x, int z, Runnable run) { Scheduler.INSTANCE.runTask(plugin, run); }
        @Override public ScheduledTask run(Plugin plugin, World world, int x, int z, Consumer<ScheduledTask> task) { return schedule(plugin, task, 1, -1, true, null); }
        @Override public ScheduledTask runDelayed(Plugin plugin, World world, int x, int z, Consumer<ScheduledTask> task, long delay) { return schedule(plugin, task, delay, -1, true, null); }
        @Override public ScheduledTask runAtFixedRate(Plugin plugin, World world, int x, int z, Consumer<ScheduledTask> task, long delay, long period) { return schedule(plugin, task, delay, period, true, null); }
    }

    static long ticks(long amount, TimeUnit unit) {
        return Math.max(0, (unit.toMillis(amount) + 49) / 50);
    }

    public static final class Async implements AsyncScheduler {
        @Override public ScheduledTask runNow(Plugin plugin, Consumer<ScheduledTask> task) { return schedule(plugin, task, 0, -1, false, null); }
        @Override public ScheduledTask runDelayed(Plugin plugin, Consumer<ScheduledTask> task, long delay, TimeUnit unit) { return schedule(plugin, task, ticks(delay, unit), -1, false, null); }
        @Override public ScheduledTask runAtFixedRate(Plugin plugin, Consumer<ScheduledTask> task, long delay, long period, TimeUnit unit) { return schedule(plugin, task, ticks(delay, unit), Math.max(1, ticks(period, unit)), false, null); }
        @Override public void cancelTasks(Plugin plugin) { Scheduler.INSTANCE.cancelTasks(plugin); }
    }

    public static final class ForEntity implements EntityScheduler {
        final Entity entity;

        ForEntity(Entity entity) { this.entity = entity; }

        private Task gated(Plugin plugin, Consumer<ScheduledTask> task, Runnable retired, long delay, long period) {
            Task[] self = new Task[1];
            Runnable gate = () -> {
                boolean alive;
                try {
                    alive = entity.isValid();
                } catch (RuntimeException e) {
                    alive = false;
                }
                if (!alive) {
                    self[0].cancel();
                    if (retired != null) retired.run();
                }
            };
            self[0] = schedule(plugin, task, delay, period, true, gate);
            return self[0];
        }

        @Override
        public boolean execute(Plugin plugin, Runnable run, Runnable retired, long delay) {
            if (!entity.isValid()) return false;
            gated(plugin, t -> run.run(), retired, Math.max(1, delay), -1);
            return true;
        }

        @Override public ScheduledTask run(Plugin plugin, Consumer<ScheduledTask> task, Runnable retired) { return entity.isValid() ? gated(plugin, task, retired, 1, -1) : null; }
        @Override public ScheduledTask runDelayed(Plugin plugin, Consumer<ScheduledTask> task, Runnable retired, long delay) { return entity.isValid() ? gated(plugin, task, retired, delay, -1) : null; }
        @Override public ScheduledTask runAtFixedRate(Plugin plugin, Consumer<ScheduledTask> task, Runnable retired, long delay, long period) { return entity.isValid() ? gated(plugin, task, retired, delay, period) : null; }
    }
}
