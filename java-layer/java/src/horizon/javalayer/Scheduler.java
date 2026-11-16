package horizon.javalayer;

import org.bukkit.plugin.Plugin;
import org.bukkit.scheduler.BukkitRunnable;
import org.bukkit.scheduler.BukkitScheduler;
import org.bukkit.scheduler.BukkitTask;
import org.bukkit.scheduler.BukkitWorker;

import java.util.*;
import java.util.concurrent.*;
import java.util.concurrent.atomic.AtomicInteger;
import java.util.function.Consumer;

/** Bukkit's scheduler: sync tasks run on the server thread at the tick they're due, async ones on a pool. */
public final class Scheduler implements BukkitScheduler {
    static final Scheduler INSTANCE = new Scheduler();

    private final AtomicInteger ids = new AtomicInteger(1);
    private final Map<Integer, Task> tasks = new ConcurrentHashMap<>();
    /** Sync tasks by the tick they run next. */
    private final PriorityQueue<Task> due = new PriorityQueue<>(Comparator.comparingLong((Task t) -> t.next).thenComparingInt(t -> t.id));
    private final ConcurrentLinkedQueue<Task> incoming = new ConcurrentLinkedQueue<>();
    private volatile long tick;
    private final ScheduledExecutorService async = Executors.newScheduledThreadPool(2, r -> {
        Thread t = new Thread(r, "Horizon Java async");
        t.setDaemon(true);
        return t;
    });

    public final class Task implements BukkitTask, Runnable {
        final int id;
        final Plugin owner;
        final Object body;
        final boolean sync;
        final long period;
        long next;
        volatile boolean cancelled;
        volatile boolean running;
        ScheduledFuture<?> future;

        Task(Plugin owner, Object body, boolean sync, long delay, long period) {
            this.id = ids.getAndIncrement();
            this.owner = owner;
            this.body = body;
            this.sync = sync;
            this.period = period;
            this.next = tick + Math.max(sync ? 1 : 0, delay);
        }

        @Override public int getTaskId() { return id; }
        @Override public Plugin getOwner() { return owner; }
        @Override public boolean isSync() { return sync; }
        @Override public boolean isCancelled() { return cancelled; }
        @Override public void cancel() { cancelTask(id); }

        @SuppressWarnings("unchecked")
        @Override
        public void run() {
            if (cancelled || !owner.isEnabled()) {
                if (!owner.isEnabled()) cancel();
                return;
            }
            running = true;
            try {
                if (body instanceof Consumer<?> c) ((Consumer<BukkitTask>) c).accept(this);
                else ((Runnable) body).run();
            } catch (Throwable e) {
                Logs.severe("Plugin " + owner.getName() + " generated an exception while executing task " + id, e);
                if (this instanceof org.bukkit.scheduler.BukkitTask task) Synth.exception(new com.destroystokyo.paper.exception.ServerSchedulerException("Plugin " + owner.getName() + " generated an exception while executing task " + id, e, task));
            } finally {
                running = false;
            }
            if (period <= 0) tasks.remove(id);
        }
    }

    private Task add(Plugin plugin, Object body, boolean sync, long delay, long period) {
        if (plugin == null) throw new IllegalArgumentException("Plugin cannot be null");
        if (body == null) throw new IllegalArgumentException("Task cannot be null");
        if (!plugin.isEnabled()) throw new org.bukkit.plugin.IllegalPluginAccessException("Plugin attempted to register task while disabled");
        Task task = new Task(plugin, body, sync, delay, period);
        if (body instanceof BukkitRunnable runnable) Tasks.bind(runnable, task);
        tasks.put(task.id, task);
        if (sync) {
            incoming.add(task);
        } else {
            long d = Math.max(0, delay) * 50L;
            task.future = period > 0 ? async.scheduleAtFixedRate(task, d, period * 50L, TimeUnit.MILLISECONDS) : async.schedule(task, d, TimeUnit.MILLISECONDS);
        }
        return task;
    }

    /** One server tick: every sync task that is due, in order. */
    void heartbeat() {
        tick++;
        // Listeners a framework puts on a HandlerList after onEnable (MCCoroutine registers from a coroutine a tick
        // later): the events they need get fired. Each second; only new ones cost anything.
        if (tick % 20 == 1) Events.syncExternal();
        Task t;
        while ((t = incoming.poll()) != null) due.add(t);
        while (!due.isEmpty() && due.peek().next <= tick) {
            Task task = due.poll();
            if (task.cancelled) continue;
            task.run();
            if (task.period > 0 && !task.cancelled) {
                task.next = tick + task.period;
                due.add(task);
            }
        }
        Runnable r;
        while ((r = mainQueue.poll()) != null) {
            try { r.run(); } catch (Throwable e) { Logs.severe("main thread task", e); }
        }
    }

    private final ConcurrentLinkedQueue<Runnable> mainQueue = new ConcurrentLinkedQueue<>();

    void shutdown() {
        for (Task task : tasks.values()) task.cancelled = true;
        tasks.clear();
        due.clear();
        incoming.clear();
    }

    long currentTick() { return tick; }

    // ---- BukkitScheduler ----
    @Override public int scheduleSyncDelayedTask(Plugin p, Runnable r, long delay) { return add(p, r, true, delay, -1).id; }
    @Override public int scheduleSyncDelayedTask(Plugin p, BukkitRunnable r, long delay) { return add(p, r, true, delay, -1).id; }
    @Override public int scheduleSyncDelayedTask(Plugin p, Runnable r) { return add(p, r, true, 0, -1).id; }
    @Override public int scheduleSyncDelayedTask(Plugin p, BukkitRunnable r) { return add(p, r, true, 0, -1).id; }
    @Override public int scheduleSyncRepeatingTask(Plugin p, Runnable r, long delay, long period) { return add(p, r, true, delay, Math.max(1, period)).id; }
    @Override public int scheduleSyncRepeatingTask(Plugin p, BukkitRunnable r, long delay, long period) { return add(p, r, true, delay, Math.max(1, period)).id; }
    @Override public int scheduleAsyncDelayedTask(Plugin p, Runnable r, long delay) { return add(p, r, false, delay, -1).id; }
    @Override public int scheduleAsyncDelayedTask(Plugin p, Runnable r) { return add(p, r, false, 0, -1).id; }
    @Override public int scheduleAsyncRepeatingTask(Plugin p, Runnable r, long delay, long period) { return add(p, r, false, delay, Math.max(1, period)).id; }

    @Override
    public <T> Future<T> callSyncMethod(Plugin plugin, Callable<T> task) {
        CompletableFuture<T> out = new CompletableFuture<>();
        add(plugin, (Runnable) () -> {
            try { out.complete(task.call()); } catch (Throwable e) { out.completeExceptionally(e); }
        }, true, 0, -1);
        return out;
    }

    @Override
    public void cancelTask(int id) {
        Task task = tasks.remove(id);
        if (task == null) return;
        task.cancelled = true;
        if (task.future != null) task.future.cancel(false);
    }

    @Override
    public void cancelTasks(Plugin plugin) {
        for (Task task : new ArrayList<>(tasks.values())) if (task.owner == plugin) cancelTask(task.id);
    }

    @Override public boolean isCurrentlyRunning(int id) { Task t = tasks.get(id); return t != null && t.running; }
    @Override public boolean isQueued(int id) { Task t = tasks.get(id); return t != null && !t.cancelled; }
    @Override public List<BukkitWorker> getActiveWorkers() { return new ArrayList<>(); }
    @Override public List<BukkitTask> getPendingTasks() { return new ArrayList<>(tasks.values()); }

    @Override public BukkitTask runTask(Plugin p, Runnable r) { return add(p, r, true, 0, -1); }
    @Override public void runTask(Plugin p, Consumer<? super BukkitTask> c) { add(p, c, true, 0, -1); }
    @Override public BukkitTask runTask(Plugin p, BukkitRunnable r) { return add(p, r, true, 0, -1); }
    @Override public BukkitTask runTaskAsynchronously(Plugin p, Runnable r) { return add(p, r, false, 0, -1); }
    @Override public void runTaskAsynchronously(Plugin p, Consumer<? super BukkitTask> c) { add(p, c, false, 0, -1); }
    @Override public BukkitTask runTaskAsynchronously(Plugin p, BukkitRunnable r) { return add(p, r, false, 0, -1); }
    @Override public BukkitTask runTaskLater(Plugin p, Runnable r, long d) { return add(p, r, true, d, -1); }
    @Override public void runTaskLater(Plugin p, Consumer<? super BukkitTask> c, long d) { add(p, c, true, d, -1); }
    @Override public BukkitTask runTaskLater(Plugin p, BukkitRunnable r, long d) { return add(p, r, true, d, -1); }
    @Override public BukkitTask runTaskLaterAsynchronously(Plugin p, Runnable r, long d) { return add(p, r, false, d, -1); }
    @Override public void runTaskLaterAsynchronously(Plugin p, Consumer<? super BukkitTask> c, long d) { add(p, c, false, d, -1); }
    @Override public BukkitTask runTaskLaterAsynchronously(Plugin p, BukkitRunnable r, long d) { return add(p, r, false, d, -1); }
    @Override public BukkitTask runTaskTimer(Plugin p, Runnable r, long d, long period) { return add(p, r, true, d, Math.max(1, period)); }
    @Override public void runTaskTimer(Plugin p, Consumer<? super BukkitTask> c, long d, long period) { add(p, c, true, d, Math.max(1, period)); }
    @Override public BukkitTask runTaskTimer(Plugin p, BukkitRunnable r, long d, long period) { return add(p, r, true, d, Math.max(1, period)); }
    @Override public BukkitTask runTaskTimerAsynchronously(Plugin p, Runnable r, long d, long period) { return add(p, r, false, d, Math.max(1, period)); }
    @Override public void runTaskTimerAsynchronously(Plugin p, Consumer<? super BukkitTask> c, long d, long period) { add(p, c, false, d, Math.max(1, period)); }
    @Override public BukkitTask runTaskTimerAsynchronously(Plugin p, BukkitRunnable r, long d, long period) { return add(p, r, false, d, Math.max(1, period)); }

    @Override
    public Executor getMainThreadExecutor(Plugin plugin) {
        return command -> {
            if (Native.isMainThread()) command.run();
            else mainQueue.add(command);
        };
    }
}
