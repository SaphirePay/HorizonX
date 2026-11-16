package horizon.javalayer;

import java.util.ArrayList;
import java.util.Arrays;
import java.util.List;
import java.util.Map;
import java.util.Random;
import java.util.concurrent.ConcurrentHashMap;
import org.bukkit.Bukkit;
import org.bukkit.HeightMap;
import org.bukkit.Keyed;
import org.bukkit.Location;
import org.bukkit.NamespacedKey;
import org.bukkit.World;
import org.bukkit.block.Biome;
import org.bukkit.generator.BiomeParameterPoint;
import org.bukkit.generator.BiomeProvider;
import org.bukkit.generator.BlockPopulator;
import org.bukkit.generator.ChunkGenerator;
import org.bukkit.generator.LimitedRegion;
import org.bukkit.generator.WorldInfo;

/**
 * A plugin's ChunkGenerator, BiomeProvider and BlockPopulator, as the server's generator calls them (the Rust
 * adapters in adapters.rs send "$gen" with an operation). The SDK's handles become Bukkit's WorldInfo, ChunkData and
 * LimitedRegion; the SDK's random goes on through {@link Seeded}, so a seeded generator makes what it makes on Paper.
 */
final class Generators {
    private Generators() {}

    private static final long MULTIPLIER = 0x5DEECE66DL, MASK = (1L << 48) - 1;

    /** java.util.Random over the SDK's 48 bits: the same numbers, and the state read back after. */
    static final class Seeded extends Random {
        long state;
        private final boolean ready;

        Seeded(long state) {
            super(0L);
            this.state = state & MASK;
            this.ready = true;
        }

        @Override
        public synchronized void setSeed(long seed) {
            super.setSeed(seed);
            if (ready) state = (seed ^ MULTIPLIER) & MASK;
        }

        @Override
        protected int next(int bits) {
            state = (state * MULTIPLIER + 0xBL) & MASK;
            return (int) (state >>> (48 - bits));
        }
    }

    /** The world each generator was made for (WorldCreator.generator): what getDefaultBiomeProvider is told. */
    private static final Map<Object, WorldInfo> MADE_FOR = new ConcurrentHashMap<>();

    static void created(org.bukkit.WorldCreator c) {
        if (c.generator() == null) return;
        World.Environment env = c.environment() == null ? World.Environment.NORMAL : c.environment();
        boolean low = env == World.Environment.NORMAL;
        WorldInfo info = new Info(c.name(), c.seed(), env, low ? -64 : 0, low ? 320 : 256);
        MADE_FOR.put(c.generator(), info);
    }

    /** A world as a generator knows it before it stands. */
    record Info(String name, long seed, World.Environment environment, int min, int max) implements WorldInfo {
        public String getName() { return name; }
        public java.util.UUID getUID() { return java.util.UUID.nameUUIDFromBytes(("world:" + name).getBytes(java.nio.charset.StandardCharsets.UTF_8)); }
        public World.Environment getEnvironment() { return environment; }
        public long getSeed() { return seed; }
        public int getMinHeight() { return min; }
        public int getMaxHeight() { return max; }
        public BiomeProvider vanillaBiomeProvider() { return VANILLA; }
        public java.util.Set<org.bukkit.FeatureFlag> getFeatureFlags() { return Bukkit.getWorlds().isEmpty() ? java.util.Set.of() : Bukkit.getWorlds().get(0).getFeatureFlags(); }
        public NamespacedKey getKey() { return NamespacedKey.minecraft(name.toLowerCase(java.util.Locale.ROOT).replaceAll("[^a-z0-9/._-]", "_")); }
    }

    /** WorldInfo.vanillaBiomeProvider: the game's own biomes. While a chunk generates the engine's generator answers
     * (the cell it just asked about); outside generation the world's own blocks do, as their biomes are the game's. */
    static final BiomeProvider VANILLA = new BiomeProvider() {
        @Override
        public Biome getBiome(WorldInfo info, int x, int y, int z) {
            String name = unquote(Native.query("{\"op\":\"layer_vanilla_biome\",\"x\":" + x + ",\"y\":" + y + ",\"z\":" + z + "}"));
            if (name != null) return biome(name);
            World w = info == null ? null : Bukkit.getWorld(info.getName());
            return w != null ? w.getBlockAt(x, y, z).getBiome() : biome("minecraft:plains");
        }

        @Override
        public List<Biome> getBiomes(WorldInfo info) {
            String all = Native.query("{\"op\":\"layer_vanilla_biomes\"}");
            List<Biome> out = new ArrayList<>();
            if (all != null && !all.equals("null")) {
                for (String part : all.replaceAll("[\\[\\]\"\\s]", "").split(",")) {
                    if (part.isEmpty()) continue;
                    Biome b = biome(part);
                    if (b != null && !out.contains(b)) out.add(b);
                }
                return out;
            }
            // The biomes the dimension's own generator makes (the game's biome source for it).
            World.Environment env = info == null ? World.Environment.NORMAL : info.getEnvironment();
            for (Biome b : org.bukkit.Registry.BIOME) {
                String k = name(b);
                boolean nether = NETHER.contains(k), end = END.contains(k);
                if (k.equals("minecraft:the_void") || k.equals("minecraft:custom")) continue;
                if (env == World.Environment.NETHER ? nether : env == World.Environment.THE_END ? end : !nether && !end) out.add(b);
            }
            return out;
        }
    };

    private static final java.util.Set<String> NETHER = java.util.Set.of("minecraft:nether_wastes", "minecraft:soul_sand_valley", "minecraft:crimson_forest", "minecraft:warped_forest", "minecraft:basalt_deltas");
    private static final java.util.Set<String> END = java.util.Set.of("minecraft:the_end", "minecraft:small_end_islands", "minecraft:end_midlands", "minecraft:end_highlands", "minecraft:end_barrens");

    private static String unquote(String json) {
        if (json == null || json.equals("null")) return null;
        return json.replace("\"", "");
    }

    static Biome biome(String name) {
        NamespacedKey key = NamespacedKey.fromString(name);
        Biome b = key == null ? null : org.bukkit.Registry.BIOME.get(key);
        return b != null ? b : org.bukkit.Registry.BIOME.get(NamespacedKey.minecraft("plains"));
    }

    static String name(Biome b) {
        return b == null ? null : ((Keyed) b).getKey().toString();
    }

    private static long num(Object o) { return ((Number) o).longValue(); }
    private static int i(Object o) { return ((Number) o).intValue(); }
    private static WorldInfo info(Object raw) { return (WorldInfo) Conv.fromNative(raw, WorldInfo.class); }
    private static World world(Object raw) { return (World) Conv.fromNative(raw, World.class); }

    /** [the random's state after, a value]: what the adapters read back. */
    private static Object out(Seeded r, Object value) {
        return Conv.toNative(Arrays.asList(r == null ? null : r.state, value));
    }

    /** Whether a class gives its own version of a method Bukkit has a default for. */
    private static boolean overrides(Object o, Class<?> base, String name, Class<?>... types) {
        for (Class<?> c = o.getClass(); c != null && c != base; c = c.getSuperclass()) {
            try {
                c.getDeclaredMethod(name, types);
                return true;
            } catch (NoSuchMethodException ignored) {
            }
        }
        return false;
    }

    static Object call(Object target, Object[] a) {
        String op = String.valueOf(a[0]);
        if (target instanceof ChunkGenerator g) return generator(g, op, a);
        if (target instanceof BiomeProvider p) return provider(p, op, a);
        if (target instanceof BlockPopulator p && op.equals("populate")) {
            World w = world(a[1]);
            Seeded r = new Seeded(num(a[2]));
            LimitedRegion region = (LimitedRegion) Conv.fromNative(a[5], LimitedRegion.class);
            p.populate(w, r, i(a[3]), i(a[4]), region);
            return out(r, null);
        }
        if (target instanceof org.bukkit.util.BlockTransformer t && op.equals("transform_block")) return transformBlock(t, a);
        if (target instanceof org.bukkit.util.EntityTransformer t && op.equals("transform_entity")) return transformEntity(t, a);
        throw new IllegalArgumentException("not a world generator call: " + op + " on " + target.getClass().getName());
    }

    /** A top-level key's text value in SNBT or JSON ({id:"minecraft:pillager", ...}): nested ones (an item's id in
     * ArmorItems) are left alone. */
    static String topLevel(String snbt, String key) {
        int depth = 0;
        boolean quoted = false;
        for (int i = 0; i < snbt.length(); i++) {
            char c = snbt.charAt(i);
            if (quoted) {
                if (c == '\\') i++;
                else if (c == '"') quoted = false;
                continue;
            }
            if (c == '{' || c == '[') { depth++; continue; }
            if (c == '}' || c == ']') { depth--; continue; }
            if (depth != 1) { if (c == '"') quoted = true; continue; }
            // At depth 1: a key starts after '{' or ','.
            int k = i;
            while (k < snbt.length() && Character.isWhitespace(snbt.charAt(k))) k++;
            boolean q = k < snbt.length() && snbt.charAt(k) == '"';
            int start = q ? k + 1 : k;
            if (snbt.startsWith(key, start)) {
                int end = start + key.length();
                if (q && end < snbt.length() && snbt.charAt(end) == '"') end++;
                while (end < snbt.length() && Character.isWhitespace(snbt.charAt(end))) end++;
                boolean before = i == 0 || snbt.charAt(i - 1) == '{' || snbt.charAt(i - 1) == ',' || Character.isWhitespace(snbt.charAt(i - 1));
                if (before && end < snbt.length() && snbt.charAt(end) == ':') {
                    int v = end + 1;
                    while (v < snbt.length() && Character.isWhitespace(snbt.charAt(v))) v++;
                    boolean vq = v < snbt.length() && snbt.charAt(v) == '"';
                    int vs = vq ? v + 1 : v, ve = vs;
                    while (ve < snbt.length() && (vq ? snbt.charAt(ve) != '"' : ",}] ".indexOf(snbt.charAt(ve)) < 0)) ve++;
                    return snbt.substring(vs, ve);
                }
            }
            if (c == '"') quoted = true;
        }
        return null;
    }

    /** AsyncStructureGenerateEvent's block transformer: the structure's block (and the world's) as BlockStates. */
    private static Object transformBlock(org.bukkit.util.BlockTransformer t, Object[] a) {
        LimitedRegion region = (LimitedRegion) Conv.fromNative(a[1], LimitedRegion.class);
        int x = i(a[2]), y = i(a[3]), z = i(a[4]);
        org.bukkit.block.data.BlockData current = (org.bukkit.block.data.BlockData) Conv.fromNative(a[5], org.bukkit.block.data.BlockData.class);
        org.bukkit.block.data.BlockData original = (org.bukkit.block.data.BlockData) Conv.fromNative(a[6], org.bukkit.block.data.BlockData.class);
        org.bukkit.block.data.BlockData inWorld = (org.bukkit.block.data.BlockData) Conv.fromNative(a[7], org.bukkit.block.data.BlockData.class);
        Location at = new Location(null, x, y, z);
        org.bukkit.util.BlockTransformer.TransformationState state = (org.bukkit.util.BlockTransformer.TransformationState) java.lang.reflect.Proxy.newProxyInstance(
                Generators.class.getClassLoader(), new Class<?>[]{org.bukkit.util.BlockTransformer.TransformationState.class}, (proxy, m, args) -> switch (m.getName()) {
                    case "getOriginal" -> BlockStates.detached(original.clone(), at);
                    case "getWorld" -> BlockStates.detached(inWorld.clone(), at);
                    case "hashCode" -> System.identityHashCode(proxy);
                    case "equals" -> proxy == args[0];
                    case "toString" -> "TransformationState";
                    default -> null;
                });
        org.bukkit.block.BlockState out = t.transform(region, x, y, z, BlockStates.detached(current, at), state);
        return Conv.toNative(out == null ? current : out.getBlockData());
    }

    /** AsyncStructureGenerateEvent's entity transformer: the entity not yet in the world, by its saved data. */
    private static Object transformEntity(org.bukkit.util.EntityTransformer t, Object[] a) {
        LimitedRegion region = (LimitedRegion) Conv.fromNative(a[1], LimitedRegion.class);
        int x = i(a[2]), y = i(a[3]), z = i(a[4]);
        String snbt = String.valueOf(a[5]);
        String id = topLevel(snbt, "id");
        NamespacedKey idKey = id == null ? null : NamespacedKey.fromString(id);
        org.bukkit.entity.EntityType type = idKey == null ? null : org.bukkit.Registry.ENTITY_TYPE.get(idKey);
        Class<?> face = type != null && type.getEntityClass() != null ? type.getEntityClass() : org.bukkit.entity.Entity.class;
        Location at = new Location(null, x, y, z);
        java.util.UUID uuid = java.util.UUID.nameUUIDFromBytes(snbt.getBytes(java.nio.charset.StandardCharsets.UTF_8));
        org.bukkit.entity.EntityType kind = type;
        org.bukkit.entity.Entity entity = (org.bukkit.entity.Entity) java.lang.reflect.Proxy.newProxyInstance(Generators.class.getClassLoader(), new Class<?>[]{face}, (proxy, m, args) -> switch (m.getName()) {
            case "getType" -> kind;
            case "getLocation" -> args == null || args.length == 0 ? at.clone() : ((Location) args[0]).set(x, y, z);
            case "getX" -> (double) x;
            case "getY" -> (double) y;
            case "getZ" -> (double) z;
            case "getUniqueId" -> uuid;
            case "isValid", "isDead", "isInWorld" -> false;
            case "getEntityId" -> -1;
            case "hashCode" -> System.identityHashCode(proxy);
            case "equals" -> proxy == args[0];
            case "toString" -> "Entity{" + kind + " (structure, not placed)}";
            default -> m.getReturnType() == boolean.class ? false : m.getReturnType().isPrimitive() ? Conv.zero(m.getReturnType()) : null;
        });
        boolean allowed = t.transform(region, x, y, z, entity, Boolean.TRUE.equals(a[6]));
        return Conv.toNative(Arrays.asList(allowed, null));
    }

    private static Object generator(ChunkGenerator g, String op, Object[] a) {
        switch (op) {
            case "noise", "surface", "bedrock", "caves" -> {
                WorldInfo info = info(a[1]);
                Seeded r = new Seeded(num(a[2]));
                int x = i(a[3]), z = i(a[4]);
                ChunkGenerator.ChunkData data = (ChunkGenerator.ChunkData) Conv.fromNative(a[5], ChunkGenerator.ChunkData.class);
                switch (op) {
                    case "noise" -> g.generateNoise(info, r, x, z, data);
                    case "surface" -> g.generateSurface(info, r, x, z, data);
                    case "bedrock" -> g.generateBedrock(info, r, x, z, data);
                    default -> g.generateCaves(info, r, x, z, data);
                }
                return out(r, null);
            }
            case "chunk" -> {
                // The old way, only when the class has it (Bukkit's own throws).
                Seeded r = new Seeded(num(a[2]));
                if (!overrides(g, ChunkGenerator.class, "generateChunkData", World.class, Random.class, int.class, int.class, ChunkGenerator.BiomeGrid.class)) return out(r, null);
                World w = Bukkit.getWorld(info(a[1]).getName());
                if (w == null) return out(r, null);
                ChunkGenerator.BiomeGrid grid = (ChunkGenerator.BiomeGrid) Conv.fromNative(a[5], ChunkGenerator.BiomeGrid.class);
                ChunkGenerator.ChunkData made = g.generateChunkData(w, r, i(a[3]), i(a[4]), grid);
                return out(r, made);
            }
            case "biome_provider" -> {
                return Conv.toNative(g.getDefaultBiomeProvider(MADE_FOR.getOrDefault(g, new Info("world", 0L, World.Environment.NORMAL, -64, 320))));
            }
            case "base_height" -> {
                Seeded r = new Seeded(num(a[2]));
                HeightMap map;
                try {
                    map = HeightMap.valueOf(String.valueOf(a[5]));
                } catch (IllegalArgumentException e) {
                    map = HeightMap.WORLD_SURFACE_WG;
                }
                return out(r, g.getBaseHeight(info(a[1]), r, i(a[3]), i(a[4]), map));
            }
            case "can_spawn" -> {
                return Conv.toNative(g.canSpawn(world(a[1]), i(a[2]), i(a[3])));
            }
            case "populators" -> {
                List<BlockPopulator> list = g.getDefaultPopulators(world(a[1]));
                return Conv.toNative(list == null ? List.of() : new ArrayList<>(list));
            }
            case "spawn" -> {
                Seeded r = new Seeded(num(a[2]));
                Location l = g.getFixedSpawnLocation(world(a[1]), r);
                return out(r, l == null ? null : Arrays.asList(l.getX(), l.getY(), l.getZ(), (double) l.getYaw(), (double) l.getPitch()));
            }
            default -> throw new IllegalArgumentException("not a generator call: " + op);
        }
    }

    private static Object provider(BiomeProvider p, String op, Object[] a) {
        switch (op) {
            case "biome" -> {
                return name(p.getBiome(info(a[1]), i(a[2]), i(a[3]), i(a[4])));
            }
            case "biome_at" -> {
                double[] c = new double[6];
                if (Conv.fromNative(a[5], List.class) instanceof List<?> l) for (int k = 0; k < 6 && k < l.size(); k++) c[k] = ((Number) l.get(k)).doubleValue();
                return name(p.getBiome(info(a[1]), i(a[2]), i(a[3]), i(a[4]), point(c)));
            }
            case "biomes" -> {
                List<String> out = new ArrayList<>();
                List<Biome> all = p.getBiomes(info(a[1]));
                if (all != null) for (Biome b : all) if (b != null) out.add(name(b));
                return Conv.toNative(out);
            }
            default -> throw new IllegalArgumentException("not a biome provider call: " + op);
        }
    }

    /** The climate at a place: on Paper each min and max is the value itself. */
    static BiomeParameterPoint point(double[] c) {
        String[] names = {"Temperature", "Humidity", "Continentalness", "Erosion", "Depth", "Weirdness"};
        return (BiomeParameterPoint) java.lang.reflect.Proxy.newProxyInstance(Generators.class.getClassLoader(), new Class<?>[]{BiomeParameterPoint.class}, (proxy, m, args) -> {
            String n = m.getName();
            if (n.startsWith("get")) {
                String what = n.substring(3).replaceFirst("^(Min|Max)", "");
                for (int k = 0; k < 6; k++) if (names[k].equals(what)) return c[k];
            }
            return switch (n) {
                case "hashCode" -> System.identityHashCode(proxy);
                case "equals" -> proxy == args[0];
                case "toString" -> "BiomeParameterPoint" + Arrays.toString(c);
                default -> m.getReturnType() == double.class ? 0.0 : null;
            };
        });
    }

    /** World.getPopulators: a live list; what a plugin adds runs on the chunks made after. */
    static final class Populators extends java.util.AbstractList<BlockPopulator> {
        private final H h;

        Populators(H h) { this.h = h; }

        private List<BlockPopulator> now() {
            Object raw = Native.tryCall(h, "getPopulators", new Object[0]);
            List<BlockPopulator> out = new ArrayList<>();
            if (raw != Native.MISSING && Conv.fromNative(raw, List.class) instanceof List<?> l) for (Object o : l) if (o instanceof BlockPopulator p) out.add(p);
            return out;
        }

        @Override public BlockPopulator get(int index) { return now().get(index); }
        @Override public int size() { return now().size(); }

        @Override
        public boolean add(BlockPopulator p) {
            if (Native.tryCall(h, "addPopulator", Conv.args(new Object[]{p})) == Native.MISSING) throw Report.cant("World.getPopulators().add");
            return true;
        }

        @Override public void add(int index, BlockPopulator p) { add(p); }

        @Override
        public BlockPopulator remove(int index) {
            List<BlockPopulator> all = now();
            BlockPopulator gone = all.remove(index);
            Native.tryCall(h, "removePopulators", new Object[0]);
            // What's left now is the generator's own: the added ones but the gone one go back.
            List<BlockPopulator> kept = now();
            for (BlockPopulator p : all) if (!kept.contains(p)) add(p);
            return gone;
        }

        @Override
        public void clear() {
            Native.tryCall(h, "removePopulators", new Object[0]);
        }
    }
}
