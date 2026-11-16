package ca.spottedleaf.moonrise.common.util;

import java.util.concurrent.atomic.AtomicInteger;
import net.minecraft.core.BlockPos;
import net.minecraft.world.entity.Entity;
import net.minecraft.world.level.ChunkPos;
import net.minecraft.world.level.Level;
import net.minecraft.world.phys.AABB;
import net.minecraft.world.phys.Vec3;

/**
 * Paper's (Moonrise's) TickThread, as the Java layer has it: the server's ticks run on the engine's own thread, which
 * Java can't make a TickThread, so that thread is a tick thread too (Bridge.mainThread). Paper's server is one
 * tick thread for every world, so each isTickThreadFor is isTickThread. Replaces the server jar's copy (build.py).
 */
public class TickThread extends Thread {
    private static final AtomicInteger ID_GENERATOR = new AtomicInteger();

    public final int id;

    public TickThread(String name) {
        this(null, null, name);
    }

    public TickThread(Runnable run, String name) {
        this(null, run, name);
    }

    public TickThread(ThreadGroup group, Runnable run, String name) {
        this(group, run, name, ID_GENERATOR.incrementAndGet());
    }

    private TickThread(ThreadGroup group, Runnable run, String name, int id) {
        super(group, run, name);
        this.id = id;
    }

    private static String getThreadContext() {
        return "thread=" + Thread.currentThread().getName();
    }

    public static void ensureTickThread(String reason) {
        if (!isTickThread()) throw new IllegalStateException(reason + ", context=" + getThreadContext());
    }

    public static void ensureTickThread(Level world, BlockPos pos, String reason) { ensureTickThread(reason); }
    public static void ensureTickThread(Level world, BlockPos pos, int blockRadius, String reason) { ensureTickThread(reason); }
    public static void ensureTickThread(Level world, ChunkPos pos, String reason) { ensureTickThread(reason); }
    public static void ensureTickThread(Level world, int chunkX, int chunkZ, String reason) { ensureTickThread(reason); }
    public static void ensureTickThread(Entity entity, String reason) { ensureTickThread(reason); }
    public static void ensureTickThread(Level world, AABB box, String reason) { ensureTickThread(reason); }
    public static void ensureTickThread(Level world, double blockX, double blockZ, String reason) { ensureTickThread(reason); }

    public static TickThread getCurrentTickThread() {
        return Thread.currentThread() instanceof TickThread t ? t : null;
    }

    public static boolean isTickThread() {
        Thread now = Thread.currentThread();
        return now instanceof TickThread || now == horizon.javalayer.Bridge.mainThread();
    }

    public static boolean isShutdownThread() {
        net.minecraft.server.MinecraftServer server = net.minecraft.server.MinecraftServer.getServer();
        return server != null && Thread.currentThread() == server.shutdownThread;
    }

    public static boolean isTickThreadFor(Level world, BlockPos pos) { return isTickThread(); }
    public static boolean isTickThreadFor(Level world, BlockPos pos, int blockRadius) { return isTickThread(); }
    public static boolean isTickThreadFor(Level world, ChunkPos pos) { return isTickThread(); }
    public static boolean isTickThreadFor(Level world, Vec3 pos) { return isTickThread(); }
    public static boolean isTickThreadFor(Level world, int chunkX, int chunkZ) { return isTickThread(); }
    public static boolean isTickThreadFor(Level world, AABB box) { return isTickThread(); }
    public static boolean isTickThreadFor(Level world, double blockX, double blockZ) { return isTickThread(); }
    public static boolean isTickThreadFor(Level world, Vec3 position, Vec3 deltaMovement, int buffer) { return isTickThread(); }
    public static boolean isTickThreadFor(Level world, int fromChunkX, int fromChunkZ, int toChunkX, int toChunkZ) { return isTickThread(); }
    public static boolean isTickThreadFor(Level world, int chunkX, int chunkZ, int radius) { return isTickThread(); }
    public static boolean isTickThreadFor(Entity entity) { return isTickThread(); }
}
