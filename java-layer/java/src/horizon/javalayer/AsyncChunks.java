package horizon.javalayer;

import org.bukkit.Chunk;
import org.bukkit.World;

import java.util.List;
import java.util.concurrent.CompletableFuture;
import java.util.concurrent.CopyOnWriteArrayList;

/**
 * Paper's getChunkAtAsync for a chunk that isn't loaded: the server loads it (generating it if new) with its own
 * ticks, held by a ticket meanwhile, and the future completes (the callback is called) on the server's thread once
 * it's in, as Paper's does. Made at once on the server's thread before, a far chunk held that tick for seconds
 * (WildRTP's random teleport). A plugin that waits for the future on the server's thread (join, get) has the chunk
 * made then, as it would wait for ever otherwise.
 */
final class AsyncChunks {
    private AsyncChunks() {}

    /** Longest wait before the chunk is made at once all the same. */
    private static final long GIVE_UP_NANOS = 30_000_000_000L;

    private static final List<Loading> PENDING = new CopyOnWriteArrayList<>();

    static final class Loading extends CompletableFuture<Chunk> {
        final World world;
        final int x, z;
        final Object callback;
        final long started = System.nanoTime();
        boolean ticket;

        Loading(World world, int x, int z, Object callback) {
            this.world = world;
            this.x = x;
            this.z = z;
            this.callback = callback;
        }

        @Override
        public Chunk join() {
            nowIfWaitingOnServer();
            return super.join();
        }

        @Override
        public Chunk get() throws InterruptedException, java.util.concurrent.ExecutionException {
            nowIfWaitingOnServer();
            return super.get();
        }

        @Override
        public Chunk get(long timeout, java.util.concurrent.TimeUnit unit) throws InterruptedException, java.util.concurrent.ExecutionException, java.util.concurrent.TimeoutException {
            nowIfWaitingOnServer();
            return super.get(timeout, unit);
        }

        private void nowIfWaitingOnServer() {
            if (!isDone() && Thread.currentThread() == Bridge.mainThread()) finish(true);
        }

        synchronized void finish(boolean makeNow) {
            if (isDone()) return;
            PENDING.remove(this);
            if (makeNow) world.loadChunk(x, z, true);
            if (ticket) world.setChunkForceLoaded(x, z, false);
            Chunk chunk = world.getChunkAt(x, z);
            try {
                deliver(callback, chunk);
            } finally {
                complete(chunk);
            }
        }
    }

    /** The callback of a getChunkAtAsync form, with the chunk (null: none, for a chunk not generated). */
    @SuppressWarnings("unchecked")
    static void deliver(Object callback, Chunk chunk) {
        if (callback instanceof java.util.function.Consumer<?> c) ((java.util.function.Consumer<Object>) c).accept(chunk);
        else if (callback instanceof World.ChunkLoadCallback c && chunk != null) c.onLoad(chunk);
    }

    /** A chunk the server loads with its ticks: the future (null for the callback forms, which return nothing). */
    static Object request(World world, int x, int z, Object callback) {
        Loading loading = new Loading(world, x, z, callback);
        loading.ticket = !world.isChunkForceLoaded(x, z);
        if (loading.ticket) world.setChunkForceLoaded(x, z, true);
        PENDING.add(loading);
        return callback instanceof java.util.function.Consumer<?> || callback instanceof World.ChunkLoadCallback ? null : loading;
    }

    /** Each tick: the waiting chunks that are in now. */
    static void tick() {
        if (PENDING.isEmpty()) return;
        long now = System.nanoTime();
        for (Loading loading : PENDING) {
            try {
                if (loading.world.isChunkLoaded(loading.x, loading.z)) loading.finish(false);
                else if (now - loading.started > GIVE_UP_NANOS) loading.finish(true);
            } catch (RuntimeException e) {
                PENDING.remove(loading);
                loading.completeExceptionally(e);
                Logs.severe("[JavaLayer] chunk " + loading.x + "," + loading.z + " for getChunkAtAsync", e);
            }
        }
    }
}
