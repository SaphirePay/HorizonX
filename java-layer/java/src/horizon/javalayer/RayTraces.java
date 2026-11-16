package horizon.javalayer;

import org.bukkit.block.Block;
import org.bukkit.block.BlockFace;
import org.bukkit.entity.Entity;
import org.bukkit.util.RayTraceResult;
import org.bukkit.util.Vector;

final class RayTraces {
    private RayTraces() {}

    /** World.rayTrace(builder -> ...): Paper's configured ray trace, run as Bukkit's block and entity traces. */
    @SuppressWarnings({"unchecked", "rawtypes"})
    static RayTraceResult configured(org.bukkit.World w, java.util.function.Consumer consumer) {
        Object[] start = new Object[1], direction = new Object[1];
        double[] distance = {0}, size = {0};
        Object[] fluid = {org.bukkit.FluidCollisionMode.NEVER}, blockMode = {null};
        boolean[] passable = {false};
        Object[] targets = {java.util.EnumSet.allOf(io.papermc.paper.raytracing.RayTraceTarget.class)};
        Object[] entityFilter = {null}, blockFilter = {null};
        Object builder = java.lang.reflect.Proxy.newProxyInstance(RayTraces.class.getClassLoader(), new Class<?>[]{io.papermc.paper.raytracing.PositionedRayTraceConfigurationBuilder.class}, (proxy, m, a) -> {
            switch (m.getName()) {
                case "start": start[0] = a[0]; break;
                case "direction": direction[0] = a[0]; break;
                case "maxDistance": distance[0] = ((Number) a[0]).doubleValue(); break;
                case "raySize": size[0] = ((Number) a[0]).doubleValue(); break;
                case "fluidCollisionMode": fluid[0] = a[0]; break;
                case "blockCollisionMode": blockMode[0] = a[0]; break;
                case "ignorePassableBlocks": passable[0] = (Boolean) a[0]; break;
                case "entityFilter": entityFilter[0] = a[0]; break;
                case "blockFilter": blockFilter[0] = a[0]; break;
                case "targets": {
                    java.util.Set<Object> t = new java.util.HashSet<>();
                    for (Object x : a) { if (x instanceof Object[] arr) t.addAll(java.util.Arrays.asList(arr)); else t.add(x); }
                    targets[0] = t;
                    break;
                }
                case "hashCode": return System.identityHashCode(proxy);
                case "equals": return proxy == a[0];
                case "toString": return "PositionedRayTraceConfigurationBuilder";
                default: break;
            }
            return proxy;
        });
        consumer.accept(builder);
        org.bukkit.Location from = (org.bukkit.Location) start[0];
        Vector dir = (Vector) direction[0];
        java.util.Set<?> want = (java.util.Set<?>) targets[0];
        RayTraceResult block = null, entity = null;
        if (want.contains(io.papermc.paper.raytracing.RayTraceTarget.BLOCK)) block = w.rayTraceBlocks(from, dir, distance[0], (org.bukkit.FluidCollisionMode) fluid[0], passable[0]);
        if (want.contains(io.papermc.paper.raytracing.RayTraceTarget.ENTITY)) {
            java.util.function.Predicate<Entity> filter = entityFilter[0] == null ? e -> true : (java.util.function.Predicate<Entity>) entityFilter[0];
            entity = w.rayTraceEntities(from, dir, distance[0], size[0], filter);
        }
        if (block == null) return entity;
        if (entity == null) return block;
        return block.getHitPosition().distanceSquared(from.toVector()) <= entity.getHitPosition().distanceSquared(from.toVector()) ? block : entity;
    }

    static RayTraceResult of(Vector at, Block block, BlockFace face, Entity entity) {
        if (entity != null) return new RayTraceResult(at, entity, face);
        if (block != null) return new RayTraceResult(at, block, face);
        return new RayTraceResult(at, face);
    }
}
