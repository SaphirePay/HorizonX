package horizon.javalayer.nms;

import net.minecraft.nbt.CompoundTag;
import net.minecraft.server.level.ServerLevel;
import net.minecraft.util.AbortableIterationConsumer;
import net.minecraft.util.ProblemReporter;
import net.minecraft.world.entity.Entity;
import net.minecraft.world.entity.EntityType;
import net.minecraft.world.level.entity.EntityInLevelCallback;
import net.minecraft.world.level.entity.EntityTypeTest;
import net.minecraft.world.level.entity.LevelEntityGetter;
import net.minecraft.world.level.storage.TagValueOutput;
import net.minecraft.world.phys.AABB;

import java.util.Collections;
import java.util.UUID;
import java.util.function.Consumer;

/**
 * Entities a plugin makes from the game's own classes and adds to a level itself (GSit's seat: an NMS armor stand the
 * player rides, added through the level's entity lookup): spawned in the Rust server from their saved tree, the same
 * UUID, with their passengers on; discarding the NMS entity removes the server's.
 */
public final class NmsEntities {
    private NmsEntities() {}

    /**
     * The level's entity lookup (Moonrise's, as Level.getEntities and an entity's removal reach it): nothing to find
     * in a hollow level, any entity may go, and `addNewEntity(Entity)` (which GSit calls by reflection) spawns in the
     * server. Made by allocation (Moonrise's constructor wants a live level): see `of`.
     */
    public static final class Getter extends ca.spottedleaf.moonrise.patches.chunk_system.level.entity.EntityLookup {
        private ServerLevel level;

        private Getter() { super(null, null); }

        static Getter of(ServerLevel level) {
            Getter getter = horizon.javalayer.Craft.allocate(Getter.class);
            getter.level = level;
            return getter;
        }

        @Override public boolean addNewEntity(Entity entity) { return spawn(level, entity); }
        @Override public boolean addNewEntity(Entity entity, boolean fromDisk) { return spawn(level, entity); }
        @Override public boolean canRemoveEntity(Entity entity) { return true; }

        @Override public Entity get(int id) { return null; }
        @Override public Entity get(UUID id) { return null; }
        @Override public Iterable<Entity> getAll() { return Collections.emptyList(); }
        @Override public <U extends Entity> void get(EntityTypeTest<Entity, U> test, AbortableIterationConsumer<U> consumer) {}
        @Override public void get(AABB box, Consumer<Entity> consumer) {}
        @Override public <U extends Entity> void get(EntityTypeTest<Entity, U> test, AABB box, AbortableIterationConsumer<U> consumer) {}

        @Override protected Boolean blockTicketUpdates() { return Boolean.FALSE; }
        @Override protected void setBlockTicketUpdates(Boolean value) {}
        @Override protected void checkThread(int chunkX, int chunkZ, String reason) {}
        @Override protected void checkThread(Entity entity, String reason) {}
        @Override protected ca.spottedleaf.moonrise.patches.chunk_system.level.entity.ChunkEntitySlices createEntityChunk(int chunkX, int chunkZ, boolean transientChunk) { return null; }
        @Override protected void onEmptySlices(int chunkX, int chunkZ) {}
        @Override protected void entitySectionChangeCallback(Entity entity, int oldX, int oldY, int oldZ, int newX, int newY, int newZ) {}
        @Override protected void addEntityCallback(Entity entity) {}
        @Override protected void removeEntityCallback(Entity entity) {}
        @Override protected void entityStartLoaded(Entity entity) {}
        @Override protected void entityEndLoaded(Entity entity) {}
        @Override protected void entityStartTicking(Entity entity) {}
        @Override protected void entityEndTicking(Entity entity) {}
        @Override protected boolean screenEntity(Entity entity, boolean fromDisk, boolean event) { return true; }
    }

    /** NMS entities in the server now (by identity): the server's entity for each. */
    private static final java.util.Map<Entity, org.bukkit.entity.Entity> SPAWNED = java.util.Collections.synchronizedMap(new java.util.IdentityHashMap<>());
    /** Players that rode an NMS entity before it was added (GSit seats them first, then adds the seat). */
    private static final java.util.Map<Entity, java.util.List<Entity>> WAITING = java.util.Collections.synchronizedMap(new java.util.IdentityHashMap<>());
    /** Each hollow player's NMS vehicle. */
    private static final java.util.Map<Entity, Entity> VEHICLES = java.util.Collections.synchronizedMap(new java.util.IdentityHashMap<>());

    /** A hollow player's NMS startRiding: on the server's entity when it's there, else once it's added. */
    static boolean ride(Entity rider, Entity vehicle) {
        if (vehicle == null) return false;
        VEHICLES.put(rider, vehicle);
        org.bukkit.entity.Entity made = SPAWNED.get(vehicle);
        if (made != null) return mount(made, rider);
        WAITING.computeIfAbsent(vehicle, k -> new java.util.ArrayList<>()).add(rider);
        return true;
    }

    static void dismount(Entity rider) {
        Entity vehicle = VEHICLES.remove(rider);
        if (vehicle == null) return;
        java.util.List<Entity> waiting = WAITING.get(vehicle);
        if (waiting != null) waiting.remove(rider);
        try {
            org.bukkit.entity.Entity player = rider.getBukkitEntity();
            if (player != null) player.leaveVehicle();
        } catch (RuntimeException ignored) {}
    }

    static Entity vehicleOf(Entity rider) { return VEHICLES.get(rider); }

    private static boolean mount(org.bukkit.entity.Entity made, Entity rider) {
        org.bukkit.entity.Entity player = rider.getBukkitEntity();
        if (player == null || !made.addPassenger(player)) return false;
        // A seat the server made a moment ago may not be where the player is yet: seat them again on the next ticks.
        if (!player.isInsideVehicle()) MOUNTS.add(new Object[] {made, player, 0});
        return true;
    }

    /** The server's entities for plugins' NMS entities that went, removed on the next tick. */
    private static final java.util.Queue<org.bukkit.entity.Entity> TO_REMOVE = new java.util.concurrent.ConcurrentLinkedQueue<>();

    /** Players still to be seated: {seat, player, ticks tried}. */
    private static final java.util.List<Object[]> MOUNTS = new java.util.concurrent.CopyOnWriteArrayList<>();

    /** Each server tick (Bridge.tick): the seats not taken yet, tried again for two seconds. */
    public static void tick() {
        // A plugin's NMS entity it took away (discard: GSit's seat as the player stands up): the server's goes too,
        // whatever callback the level gave the entity since.
        if (!SPAWNED.isEmpty()) {
            java.util.List<Entity> gone = new java.util.ArrayList<>();
            synchronized (SPAWNED) {
                for (Entity entity : SPAWNED.keySet()) if (entity.isRemoved()) gone.add(entity);
            }
            for (Entity entity : gone) {
                org.bukkit.entity.Entity made = SPAWNED.remove(entity);
                WAITING.remove(entity);
                VEHICLES.values().removeIf(v -> v == entity);
                if (made != null) TO_REMOVE.add(made);
            }
        }
        for (org.bukkit.entity.Entity made; (made = TO_REMOVE.poll()) != null; ) {
            try {
                made.remove();
            } catch (RuntimeException e) {
                horizon.javalayer.Native.log(1, "[JavaLayer] the server's " + made.getType() + " for a plugin's NMS entity could not be removed: " + e);
            }
        }
        if (MOUNTS.isEmpty()) return;
        for (Object[] row : MOUNTS) {
            org.bukkit.entity.Entity made = (org.bukkit.entity.Entity) row[0];
            org.bukkit.entity.Entity player = (org.bukkit.entity.Entity) row[1];
            int tries = (Integer) row[2] + 1;
            row[2] = tries;
            if (player.isInsideVehicle() || !made.isValid() || tries > 40) {
                MOUNTS.remove(row);
                continue;
            }
            made.addPassenger(player);
        }
    }

    /** The NMS entity in the Rust server, as Paper's level would add it. False when the server made none. */
    public static boolean spawn(ServerLevel level, Entity entity) {
        try {
            TagValueOutput out = TagValueOutput.createWithContext(ProblemReporter.DISCARDING, Hollow.registries());
            entity.saveWithoutId(out);
            CompoundTag tree = out.buildResult();
            tree.putString("id", EntityType.getKey(entity.getType()).toString());
            tree.remove("Passengers");
            java.io.ByteArrayOutputStream bytes = new java.io.ByteArrayOutputStream();
            // The plain named tree (what the server reads; Paper's own serializeEntity gzips it, which it reads too).
            net.minecraft.nbt.NbtIo.write(tree, new java.io.DataOutputStream(bytes));
            org.bukkit.World world = level.getWorld();
            org.bukkit.entity.Entity made = org.bukkit.Bukkit.getUnsafe().deserializeEntity(bytes.toByteArray(), world, true);
            if (made == null) {
                horizon.javalayer.Native.log(1, "[JavaLayer] the server made no " + entity.getType() + " from the plugin's: " + tree);
                return false;
            }
            SPAWNED.put(entity, made);
            for (Entity passenger : entity.getPassengers()) {
                org.bukkit.entity.Entity rider = passenger.getBukkitEntity();
                if (rider != null) made.addPassenger(rider);
            }
            java.util.List<Entity> riders = WAITING.remove(entity);
            if (riders != null) for (Entity rider : riders) mount(made, rider);
            // Discarded (the plugin takes its seat away): the server's goes too.
            entity.setLevelCallback(new EntityInLevelCallback() {
                @Override public void onMove() {}
                @Override public void onRemove(Entity.RemovalReason reason) {
                    SPAWNED.remove(entity);
                    WAITING.remove(entity);
                    VEHICLES.values().removeIf(v -> v == entity);
                    // On the next tick: a plugin discards its entity inside an event (GSit's seat in the player's
                    // EntityDismountEvent), while the server holds its entities out of the game.
                    TO_REMOVE.add(made);
                }
            });
            return true;
        } catch (Exception | LinkageError e) {
            horizon.javalayer.Native.log(1, "[JavaLayer] the plugin's " + entity.getType() + " could not be added to the server: " + e);
            return false;
        }
    }
}
