package horizon.javalayer.nms;

import net.minecraft.core.BlockPos;
import net.minecraft.server.level.FullChunkStatus;
import net.minecraft.server.level.ServerChunkCache;
import net.minecraft.server.level.ServerLevel;
import net.minecraft.server.level.ThreadedLevelLightEngine;
import net.minecraft.world.level.ChunkPos;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.world.level.chunk.ChunkAccess;
import net.minecraft.world.level.chunk.LevelChunk;
import net.minecraft.world.level.chunk.LevelChunkSection;
import net.minecraft.world.level.material.FluidState;

import java.util.Map;
import java.util.concurrent.ConcurrentHashMap;

/**
 * The game's own chunks for the hollow levels, live: a block read is the Rust server's block now, a block write is
 * the Rust server's (which updates neighbours, light and the players who see it, so vanilla's own side effects on
 * the level do nothing here). What plugins reach this way: WorldEdit's version adapter (getChunk -> getBlockState /
 * setBlockState), AntiXray, Orebfuscator.
 */
public final class LiveChunks {
    private LiveChunks() {}

    static final java.util.concurrent.atomic.AtomicLong REFRESHES = new java.util.concurrent.atomic.AtomicLong();
    static final java.util.concurrent.atomic.AtomicLong REFRESH_NANOS = new java.util.concurrent.atomic.AtomicLong();
    static final java.util.concurrent.atomic.AtomicLong QUERY_NANOS = new java.util.concurrent.atomic.AtomicLong();

    /** Chunk reads from the Rust server so far, and their time. */
    public static String stats() {
        long n = REFRESHES.get();
        return n + " chunk reads, " + (n == 0 ? 0 : REFRESH_NANOS.get() / n / 1000) + " us each (" + (n == 0 ? 0 : QUERY_NANOS.get() / n / 1000) + " us asking the server), " + FROM_PACKETS.get() + " from chunk packets";
    }

    /** BlockState by the block data's text: parsing is the slow part of a read. */
    private static final Map<String, BlockState> STATES = new ConcurrentHashMap<>();

    static BlockState read(ServerLevel level, int x, int y, int z) {
        org.bukkit.World world = level.getWorld();
        String text = world.getBlockAt(x, y, z).getBlockData().getAsString();
        return STATES.computeIfAbsent(text, Hollow::blockState);
    }

    /** A block written through the Rust server (`physics`: neighbours told, as vanilla's flag 1). */
    static BlockState write(ServerLevel level, BlockPos pos, BlockState state, boolean physics) {
        BlockState old = read(level, pos.getX(), pos.getY(), pos.getZ());
        if (old == state) return null;
        String text = net.minecraft.commands.arguments.blocks.BlockStateParser.serialize(state);
        level.getWorld().getBlockAt(pos.getX(), pos.getY(), pos.getZ()).setBlockData(org.bukkit.Bukkit.createBlockData(text), physics);
        return old;
    }

    /** A level's chunk at (x, z): one object each, kept with its level. */
    static LevelChunk chunk(ServerLevel level, int x, int z) {
        if (!(level.getChunkSource() instanceof Cache cache)) return null;
        used = true;
        return cache.chunks.computeIfAbsent(ChunkPos.pack(x, z), k -> Chunk.make(level, x, z, true));
    }

    /** Whether a plugin has read the game's chunks here at all (then the chunks sent to players are kept too). */
    static volatile boolean used;

    static final java.util.concurrent.atomic.AtomicLong FROM_PACKETS = new java.util.concurrent.atomic.AtomicLong();

    /**
     * A chunk the game sends a player (its packet, as the game made it, before any plugin's handler): its sections are
     * the chunk as it is now, kept as a fresh read of it. Orebfuscator and AntiXray read each chunk they hide ores in
     * and its neighbours; this way most of those reads need no query (each waited for the game's thread).
     */
    public static void sent(java.util.UUID player, net.minecraft.network.protocol.game.ClientboundLevelChunkWithLightPacket packet) {
        if (!used) return;
        // On a thread of its own: the writer that sends the packet doesn't wait for the player's world.
        FILLER.execute(() -> {
            try {
                ServerLevel level = levelOf(player);
                if (level == null || !(level.getChunkSource() instanceof Cache cache)) return;
                int x = packet.x(), z = packet.z();
                LevelChunk made = cache.chunks.computeIfAbsent(ChunkPos.pack(x, z), k -> Chunk.make(level, x, z, false));
                if (made instanceof Chunk chunk) chunk.fill(packet.chunkData().getReadBuffer());
            } catch (RuntimeException | LinkageError e) {
                // A read from the server answers for this chunk instead.
            }
        });
    }

    private static final java.util.concurrent.ExecutorService FILLER = java.util.concurrent.Executors.newSingleThreadExecutor(r -> {
        Thread t = new Thread(r, "JavaLayer chunk packets");
        t.setDaemon(true);
        return t;
    });

    /** By player: [when asked, their level], a second at most (a world change shows within it). */
    private static final Map<java.util.UUID, Object[]> LEVELS = new ConcurrentHashMap<>();

    private static ServerLevel levelOf(java.util.UUID player) {
        long now = System.nanoTime();
        Object[] kept = LEVELS.get(player);
        if (kept != null && now - (long) kept[0] < 1_000_000_000L) return (ServerLevel) kept[1];
        org.bukkit.entity.Player online = org.bukkit.Bukkit.getPlayer(player);
        if (online == null || !(online.getWorld() instanceof org.bukkit.craftbukkit.CraftWorld craft)) return null;
        ServerLevel level = craft.getHandle();
        LEVELS.put(player, new Object[]{now, level});
        return level;
    }

    /** ServerChunkCache: every chunk is there (the Rust server loads what is asked for). */
    public static final class Cache extends ServerChunkCache {
        final Map<Long, LevelChunk> chunks = new ConcurrentHashMap<>();
        ServerLevel owner;

        private Cache() { super(null, null, null, null, null, null, 0, 0, false, null, null); }

        static Cache make(ServerLevel level) {
            Cache cache = horizon.javalayer.Craft.allocate(Cache.class);
            horizon.javalayer.Craft.set(cache, Cache.class, "chunks", new ConcurrentHashMap<Long, LevelChunk>());
            cache.owner = level;
            return cache;
        }

        @Override public LevelChunk getChunkAtIfLoadedImmediately(int x, int z) { return chunk(owner, x, z); }
        @Override public LevelChunk getChunkAtIfCachedImmediately(int x, int z) { return chunk(owner, x, z); }
        @Override public LevelChunk getChunkNow(int x, int z) { return chunk(owner, x, z); }
        @Override public ChunkAccess getChunk(int x, int z, net.minecraft.world.level.chunk.status.ChunkStatus status, boolean load) { return chunk(owner, x, z); }
        /** The Rust server tells the players itself. */
        @Override public void blockChanged(BlockPos pos) {}
    }

    /** The sections' palettes, as the game makes them for its registries. */
    private static volatile net.minecraft.world.level.chunk.PalettedContainerFactory PALETTES;

    static net.minecraft.world.level.chunk.PalettedContainerFactory palettes() {
        if (PALETTES == null) PALETTES = net.minecraft.world.level.chunk.PalettedContainerFactory.create(Hollow.registryAccess());
        return PALETTES;
    }

    /**
     * LevelChunk with the Rust server's blocks: its sections are the chunk as it is (one bulk read, `blocks_read`, made
     * again when a second old), so what reads them (getBlockState, the chunk packet a plugin builds, heightmaps) sees
     * real blocks; what it sets goes to the Rust server and into its sections.
     */
    public static final class Chunk extends LevelChunk {
        ServerLevel owner;
        volatile long builtAt;
        /** 1 while a thread reads the chunk again (allocated: no initializers, so a plain int set by READING). */
        volatile int reading;

        private static final java.lang.invoke.VarHandle READING;
        static {
            try {
                READING = java.lang.invoke.MethodHandles.lookup().findVarHandle(Chunk.class, "reading", int.class);
            } catch (ReflectiveOperationException e) {
                throw new ExceptionInInitializerError(e);
            }
        }

        private Chunk() { super(null, null); }

        static Chunk make(ServerLevel level, int x, int z, boolean read) {
            Chunk chunk = horizon.javalayer.Craft.allocate(Chunk.class);
            chunk.owner = level;
            horizon.javalayer.Craft.set(chunk, ChunkAccess.class, "chunkPos", new ChunkPos(x, z));
            horizon.javalayer.Craft.set(chunk, ChunkAccess.class, "levelHeightAccessor", level);
            horizon.javalayer.Craft.set(chunk, LevelChunk.class, "level", level);
            // What ChunkAccess's and LevelChunk's constructors keep of the level's height and the chunk's place.
            int minY = level.getMinY(), height = level.getHeight(), maxY = minY + height - 1;
            horizon.javalayer.Craft.set(chunk, ChunkAccess.class, "locX", x);
            horizon.javalayer.Craft.set(chunk, ChunkAccess.class, "locZ", z);
            for (Class<?> owner : new Class<?>[]{ChunkAccess.class, LevelChunk.class}) {
                horizon.javalayer.Craft.set(chunk, owner, "minSection", minY >> 4);
                horizon.javalayer.Craft.set(chunk, owner, "maxSection", maxY >> 4);
            }
            horizon.javalayer.Craft.set(chunk, ChunkAccess.class, "minY", minY);
            horizon.javalayer.Craft.set(chunk, ChunkAccess.class, "height", height);
            horizon.javalayer.Craft.set(chunk, ChunkAccess.class, "maxY", maxY);
            horizon.javalayer.Craft.set(chunk, ChunkAccess.class, "minSectionY", minY >> 4);
            horizon.javalayer.Craft.set(chunk, ChunkAccess.class, "maxSectionY", maxY >> 4);
            horizon.javalayer.Craft.set(chunk, ChunkAccess.class, "sectionsCount", (maxY >> 4) - (minY >> 4) + 1);
            // Block entities: none of the Rust server's are mirrored here yet (a chunk packet a plugin builds has none).
            horizon.javalayer.Craft.set(chunk, ChunkAccess.class, "pendingBlockEntities", new java.util.HashMap<BlockPos, net.minecraft.nbt.CompoundTag>());
            horizon.javalayer.Craft.set(chunk, ChunkAccess.class, "blockEntities", new java.util.HashMap<BlockPos, net.minecraft.world.level.block.entity.BlockEntity>());
            if (read) chunk.refresh();
            return chunk;
        }

        /** The sections from a chunk packet's data (vanilla's layout: each section's block count, states, biomes). */
        void fill(net.minecraft.network.FriendlyByteBuf data) {
            if (!READING.compareAndSet(this, 0, 1)) return;
            try {
                int count = owner.getSectionsCount();
                LevelChunkSection[] sections = new LevelChunkSection[count];
                for (int i = 0; i < count; i++) {
                    sections[i] = new LevelChunkSection(palettes());
                    sections[i].read(data);
                }
                install(sections);
                FROM_PACKETS.incrementAndGet();
            } finally {
                reading = 0;
            }
        }

        /**
         * The chunk's blocks from the Rust server (air where it has none loaded), its heightmaps from them. One thread
         * reads at a time and the others go on with the blocks as they were: no lock is held while the read waits for
         * the game, since the game's thread may itself be reading this chunk (a writer thread's AntiXray handler and
         * the game's own events read the same chunks).
         */
        void refresh() {
            if (!READING.compareAndSet(this, 0, 1)) {
                if (builtAt != 0) return;
                // The first read, by another thread: its blocks are needed, so wait for them.
                while (reading == 1) horizon.javalayer.Native.pumpQueries();
                return;
            }
            long started = System.nanoTime();
            try {
                refreshTimed();
            } finally {
                reading = 0;
                REFRESHES.incrementAndGet();
                REFRESH_NANOS.addAndGet(System.nanoTime() - started);
            }
        }

        private void refreshTimed() {
            int count = owner.getSectionsCount(), minY = owner.getMinY();
            LevelChunkSection[] sections = new LevelChunkSection[count];
            for (int i = 0; i < count; i++) sections[i] = new LevelChunkSection(palettes());
            ChunkPos pos = getPos();
            String dimension = owner.getWorld().getKey().toString();
            long asked = System.nanoTime();
            String answer = horizon.javalayer.Native.query("{\"op\":\"blocks_read\",\"dimension\":\"" + dimension + "\",\"chunk\":[" + pos.x() + "," + pos.z() + "]}");
            QUERY_NANOS.addAndGet(System.nanoTime() - asked);
            try {
                // The answer is {"bottom":B,"data":"<base64>","height":H} with ~400 KB of data: its three fields read
                // straight from the text (a JSON tree of it cost more than the blocks themselves).
                String data = answer == null ? null : stringField(answer, "data");
                if (data != null) {
                    int bottom = intField(answer, "bottom"), height = intField(answer, "height");
                    java.nio.ByteBuffer ids = java.nio.ByteBuffer.wrap(java.util.Base64.getDecoder().decode(data)).order(java.nio.ByteOrder.LITTLE_ENDIAN);
                    for (int dy = 0; dy < height; dy++) {
                        int y = bottom + dy, index = (y - minY) >> 4;
                        for (int i = 0; i < 256; i++) {
                            int id = ids.getInt();
                            if (id == 0 || index < 0 || index >= count) continue;
                            BlockState state = net.minecraft.world.level.block.Block.stateById(id);
                            if (state != null) sections[index].setBlockState(i & 15, y & 15, i >> 4, state, false);
                        }
                    }
                }
            } catch (RuntimeException e) {
                horizon.javalayer.Native.log(1, "[JavaLayer] chunk " + pos + " could not be read: " + e);
            }
            install(sections);
        }

        /** These sections as the chunk's, fresh from now, with heightmaps worked out from them. */
        private void install(LevelChunkSection[] sections) {
            for (LevelChunkSection section : sections) section.recalcBlockCounts();
            horizon.javalayer.Craft.set(this, ChunkAccess.class, "sections", sections);
            // Fresh from here on (the heightmaps below read these sections back).
            builtAt = System.nanoTime();
            horizon.javalayer.Craft.set(this, ChunkAccess.class, "heightmaps", new java.util.EnumMap<net.minecraft.world.level.levelgen.Heightmap.Types, net.minecraft.world.level.levelgen.Heightmap>(net.minecraft.world.level.levelgen.Heightmap.Types.class));
            net.minecraft.world.level.levelgen.Heightmap.primeHeightmaps(this, java.util.EnumSet.allOf(net.minecraft.world.level.levelgen.Heightmap.Types.class));
        }

        /** A string field of a flat JSON object with no escapes in it (base64), or null. */
        private static String stringField(String json, String name) {
            int at = json.indexOf("\"" + name + "\":\"");
            if (at < 0) return null;
            int start = at + name.length() + 4, end = json.indexOf('"', start);
            return end < 0 ? null : json.substring(start, end);
        }

        /** An integer field of a flat JSON object. */
        private static int intField(String json, String name) {
            int at = json.indexOf("\"" + name + "\":");
            if (at < 0) throw new IllegalArgumentException("no " + name);
            int i = at + name.length() + 3, end = i;
            while (end < json.length() && (json.charAt(end) == '-' || Character.isDigit(json.charAt(end)))) end++;
            return Integer.parseInt(json.substring(i, end));
        }

        private LevelChunkSection section(int y) {
            if (System.nanoTime() - builtAt > 1_000_000_000L) refresh();
            int index = (y - owner.getMinY()) >> 4;
            LevelChunkSection[] sections = getSections();
            return index < 0 || index >= sections.length ? null : sections[index];
        }

        @Override
        public BlockState getBlockState(int x, int y, int z) {
            LevelChunkSection section = section(y);
            return section == null ? net.minecraft.world.level.block.Blocks.AIR.defaultBlockState() : section.getBlockState(x & 15, y & 15, z & 15);
        }

        @Override public BlockState getBlockState(BlockPos pos) { return getBlockState(pos.getX(), pos.getY(), pos.getZ()); }
        @Override public FluidState getFluidState(BlockPos pos) { return getBlockState(pos).getFluidState(); }
        @Override public FluidState getFluidState(int x, int y, int z) { return getBlockState(x, y, z).getFluidState(); }

        @Override
        public BlockState setBlockState(BlockPos pos, BlockState state, int flags) {
            BlockState old = write(owner, pos, state, (flags & 1) != 0);
            LevelChunkSection section = section(pos.getY());
            if (section != null) section.setBlockState(pos.getX() & 15, pos.getY() & 15, pos.getZ() & 15, state, false);
            return old;
        }

        @Override public net.minecraft.world.level.block.entity.BlockEntity getBlockEntity(BlockPos pos) { return null; }
        @Override public net.minecraft.world.level.block.entity.BlockEntity getBlockEntity(BlockPos pos, EntityCreationType type) { return null; }
        @Override public FullChunkStatus getFullStatus() { return FullChunkStatus.BLOCK_TICKING; }
    }

    /**
     * ServerLevel whose block changes are the Rust server's: setBlock writes there, and what vanilla does after a change
     * (telling players, neighbour updates, comparators) the Rust server has done already.
     */
    public static final class Level extends ServerLevel {
        private Level() { super(null, null, null, null, null, null, false, 0L, null, false, null, null, null, null, null, null); }

        @Override
        public boolean setBlock(BlockPos pos, BlockState state, int flags, int recursionLeft) {
            LevelChunk c = chunk(this, pos.getX() >> 4, pos.getZ() >> 4);
            if (c != null) c.setBlockState(pos, state, flags);
            else write(this, pos, state, (flags & 1) != 0);
            return true;
        }

        @Override public BlockState getBlockState(BlockPos pos) { LevelChunk c = chunk(this, pos.getX() >> 4, pos.getZ() >> 4); return c == null ? read(this, pos.getX(), pos.getY(), pos.getZ()) : c.getBlockState(pos); }
        /** The level's entity lookup: an NMS entity a plugin adds goes to the server (NmsEntities). */
        @Override public net.minecraft.world.level.entity.LevelEntityGetter<net.minecraft.world.entity.Entity> getEntities() { return moonrise$getEntityLookup(); }
        @Override public FluidState getFluidState(BlockPos pos) { return getBlockState(pos).getFluidState(); }
        @Override public net.minecraft.world.level.block.entity.BlockEntity getBlockEntity(BlockPos pos) { return null; }
        @Override public void sendBlockUpdated(BlockPos pos, BlockState oldState, BlockState newState, int flags) {}
        @Override public void updateNeighborsAt(BlockPos pos, net.minecraft.world.level.block.Block block) {}
        @Override public void updateNeighborsAt(BlockPos pos, net.minecraft.world.level.block.Block block, net.minecraft.world.level.redstone.Orientation orientation) {}
        @Override public void updateNeighbourForOutputSignal(BlockPos pos, net.minecraft.world.level.block.Block block) {}
        @Override public void updateNeighborsAtExceptFromFacing(BlockPos pos, net.minecraft.world.level.block.Block block, net.minecraft.core.Direction skip, net.minecraft.world.level.redstone.Orientation orientation) {}
        @Override public void neighborChanged(BlockPos pos, net.minecraft.world.level.block.Block block, net.minecraft.world.level.redstone.Orientation orientation) {}
        @Override public void neighborChanged(BlockState state, BlockPos pos, net.minecraft.world.level.block.Block block, net.minecraft.world.level.redstone.Orientation orientation, boolean moved) {}
        @Override public void neighborShapeChanged(net.minecraft.core.Direction direction, BlockPos pos, BlockPos neighbor, BlockState state, int flags, int recursionLeft) {}
        @Override public void setBlocksDirty(BlockPos pos, BlockState oldState, BlockState newState) {}
        @Override public void addDestroyBlockEffect(BlockPos pos, BlockState state) {}
    }

    /** The light engine: the Rust server lights what changes. */
    public static final class Light extends ThreadedLevelLightEngine {
        private Light() { super(null, null, false, null, null); }

        @Override public void checkBlock(BlockPos pos) {}
    }
}
