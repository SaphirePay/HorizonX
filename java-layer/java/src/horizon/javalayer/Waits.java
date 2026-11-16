package horizon.javalayer;

import java.lang.invoke.MethodHandle;
import java.lang.invoke.MethodHandles;
import java.lang.invoke.MethodType;
import java.util.concurrent.CompletableFuture;
import java.util.concurrent.ExecutionException;
import java.util.concurrent.Future;
import java.util.concurrent.TimeUnit;
import java.util.concurrent.TimeoutException;

/**
 * A plugin's main thread waiting on work of another thread (`supplyAsync(() -> world.getBlockAt(..)).join()`): on
 * Paper the other thread reads the world itself, here its calls are answered on the main thread, which is the one
 * waiting. So the wait answers them while it waits. Plugin classes reach these through an in-memory rewrite of their
 * Future.get / CompletableFuture.join / get calls (as Paper's Commodore rewrites, the jar is never changed); off the
 * main thread they are the plain calls.
 */
public final class Waits {
    private Waits() {}

    private static final long STEP = TimeUnit.MILLISECONDS.toNanos(1);

    private static boolean onMain() {
        try {
            return Native.isMainThread();
        } catch (Throwable e) {
            return false;
        }
    }

    /** Until `f` is done, or `deadline` (System.nanoTime) passes: the other threads' calls answered meanwhile. */
    private static void pumpUntil(Future<?> f, long deadline) {
        // A chunk getChunkAtAsync is loading with the server's ticks: the tick it waits for is this one, so it is
        // made now.
        if (f instanceof AsyncChunks.Loading loading) loading.finish(true);
        while (!f.isDone() && System.nanoTime() < deadline) {
            Native.pumpQueries();
            java.util.concurrent.locks.LockSupport.parkNanos(STEP);
        }
    }

    public static Object join(CompletableFuture<?> f) {
        if (!f.isDone() && onMain()) pumpUntil(f, Long.MAX_VALUE);
        return f.join();
    }

    public static Object get(CompletableFuture<?> f) throws InterruptedException, ExecutionException {
        if (!f.isDone() && onMain()) pumpUntil(f, Long.MAX_VALUE);
        return f.get();
    }

    public static Object get(CompletableFuture<?> f, long timeout, TimeUnit unit) throws InterruptedException, ExecutionException, TimeoutException {
        if (!f.isDone() && onMain()) {
            pumpUntil(f, System.nanoTime() + unit.toNanos(timeout));
            if (!f.isDone()) throw new TimeoutException();
        }
        return f.get(timeout, unit);
    }

    public static Object get(Future<?> f) throws InterruptedException, ExecutionException {
        if (!f.isDone() && onMain()) pumpUntil(f, Long.MAX_VALUE);
        return f.get();
    }

    public static Object get(Future<?> f, long timeout, TimeUnit unit) throws InterruptedException, ExecutionException, TimeoutException {
        if (!f.isDone() && onMain()) {
            pumpUntil(f, System.nanoTime() + unit.toNanos(timeout));
            if (!f.isDone()) throw new TimeoutException();
        }
        return f.get(timeout, unit);
    }

    // ---- The rewrite: horizon.javalayer.asm.WaitRewriter, defined in the server's loader where ASM is ----

    private static volatile boolean tried;
    private static MethodHandle rewrite;

    private static synchronized MethodHandle rewriter() {
        if (tried) return rewrite;
        tried = true;
        ClassLoader server = ServerClasses.loader();
        if (server == null) return null;
        String name = "horizon.javalayer.asm.WaitRewriter";
        ClassLoader layer = Waits.class.getClassLoader();
        try {
            // Its own loader over the server's, so ASM resolves there (the layer's loader can't see it): it defines the
            // rewriter and its inner classes itself, from the layer's jar.
            ClassLoader own = new ClassLoader(server) {
                @Override
                protected synchronized Class<?> loadClass(String n, boolean resolve) throws ClassNotFoundException {
                    if (!n.startsWith("horizon.javalayer.asm.")) return super.loadClass(n, resolve);
                    Class<?> done = findLoadedClass(n);
                    if (done != null) return done;
                    try (java.io.InputStream in = layer.getResourceAsStream(n.replace('.', '/') + ".class")) {
                        if (in == null) throw new ClassNotFoundException(n);
                        byte[] code = in.readAllBytes();
                        return defineClass(n, code, 0, code.length);
                    } catch (java.io.IOException e) {
                        throw new ClassNotFoundException(n, e);
                    }
                }
            };
            Class<?> type = own.loadClass(name);
            rewrite = MethodHandles.publicLookup().findStatic(type, "rewrite", MethodType.methodType(byte[].class, byte[].class));
        } catch (Throwable e) {
            Logs.warning("The main thread's waits can't be rewritten: " + e);
            rewrite = null;
        }
        return rewrite;
    }

    /** A plugin class's bytes with its waits on futures going through this class; as they were if it has none. */
    static byte[] rewrite(byte[] bytes) {
        if (!mentionsFutures(bytes)) return bytes;
        MethodHandle h = rewriter();
        if (h == null) return bytes;
        try {
            return (byte[]) h.invoke(bytes);
        } catch (Throwable e) {
            return bytes;
        }
    }

    private static final byte[] MARK = "java/util/concurrent/".getBytes(java.nio.charset.StandardCharsets.ISO_8859_1);

    private static boolean mentionsFutures(byte[] bytes) {
        outer:
        for (int i = 0; i + MARK.length <= bytes.length; i++) {
            for (int j = 0; j < MARK.length; j++) if (bytes[i + j] != MARK[j]) continue outer;
            return true;
        }
        return false;
    }
}
