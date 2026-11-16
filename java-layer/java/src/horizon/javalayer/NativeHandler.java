package horizon.javalayer;

import org.bukkit.Location;
import org.bukkit.Server;
import org.bukkit.entity.Player;
import org.bukkit.metadata.MetadataValue;
import org.bukkit.plugin.Plugin;

import java.lang.reflect.InvocationHandler;
import java.lang.reflect.Method;
import java.util.*;
import java.util.concurrent.ConcurrentHashMap;

/** A Bukkit interface method on a game object: the server's answer, else the interface's default, else a clear error. */
public final class NativeHandler implements InvocationHandler {
    final H h;

    /** The handle (for the generated item meta classes, which live in CraftBukkit's package). */
    public H handle() { return h; }

    NativeHandler(H h) { this.h = h; }

    /** `setMetadata` values: Java objects, kept here by the object's key. */
    static final Map<String, Map<String, List<MetadataValue>>> METADATA = new ConcurrentHashMap<>();
    private static final Set<String> TOLD = ConcurrentHashMap.newKeySet();

    static void missing(String where) {
        Report.gap(where);
        if (TOLD.add(where)) Native.log(1, "[JavaLayer] not available yet: " + where + " (the plugin keeps running)");
    }

    private static String key(H h) { return h.key.isEmpty() ? "#" + h.id : h.key; }

    /** CraftPlayerProfile.complete(true) on a copy: an id from the user cache, or the offline id offline. */
    static org.bukkit.profile.PlayerProfile completed(org.bukkit.profile.PlayerProfile profile) {
        if (profile.getUniqueId() != null || profile.getName() == null || profile.getName().isEmpty()) return profile;
        String name = profile.getName();
        java.util.UUID id = null;
        org.bukkit.OfflinePlayer cached = org.bukkit.Bukkit.getOfflinePlayerIfCached(name);
        if (cached != null) id = cached.getUniqueId();
        if (id == null && !org.bukkit.Bukkit.getOnlineMode()) id = java.util.UUID.nameUUIDFromBytes(("OfflinePlayer:" + name).getBytes(java.nio.charset.StandardCharsets.UTF_8));
        if (id == null) return profile;
        org.bukkit.profile.PlayerProfile made = profile instanceof com.destroystokyo.paper.profile.PlayerProfile
                ? org.bukkit.Bukkit.createProfile(id, name) : org.bukkit.Bukkit.createPlayerProfile(id, name);
        try {
            made.setTextures(profile.getTextures());
        } catch (RuntimeException ignored) {}
        return made;
    }

    @Override
    public Object invoke(Object proxy, Method m, Object[] args) throws Throwable {
        return call(proxy, h, m, args);
    }

    /** One call on a game object (a proxy, or a generated class such as CraftPlayer). */
    static Object call(Object proxy, H h, Method m, Object[] args) throws Throwable {
        String name = m.getName();
        int n = args == null ? 0 : args.length;
        if (m.getDeclaringClass() == Object.class) {
            switch (name) {
                case "equals": {
                    H other = Proxies.handleOf(args[0]);
                    if (other == null) return false;
                    if (!h.key.isEmpty() || !other.key.isEmpty()) return h.key.equals(other.key);
                    if (other.id == h.id) return true;
                    // Item metas: equal when their data is (Paper's CraftMetaItem.equals), not by which item holds them.
                    if (proxy instanceof org.bukkit.inventory.meta.ItemMeta && args[0] instanceof org.bukkit.inventory.meta.ItemMeta) {
                        String a = Native.itemToJson(h), b = Native.itemToJson(other);
                        if (a != null && b != null) {
                            com.google.gson.JsonElement ca = com.google.gson.JsonParser.parseString(a).getAsJsonObject().get("components");
                            com.google.gson.JsonElement cb = com.google.gson.JsonParser.parseString(b).getAsJsonObject().get("components");
                            return java.util.Objects.equals(ca == null || ca.isJsonNull() ? new com.google.gson.JsonObject() : ca, cb == null || cb.isJsonNull() ? new com.google.gson.JsonObject() : cb);
                        }
                    }
                    Object same = Native.tryCall(h, "equals", new Object[]{other});
                    return same instanceof Boolean b && b;
                }
                case "hashCode":
                    return h.key.isEmpty() ? System.identityHashCode(proxy) : h.key.hashCode();
                case "toString": {
                    String paper = PaperStrings.of(proxy);
                    if (paper != null) return paper;
                    Object text = Native.tryCall(h, "toString", new Object[0]);
                    return text == Native.MISSING || text == null ? h.toString() : String.valueOf(text);
                }
                default:
                    return null;
            }
        }
        Object own = own(proxy, h, m, name, n, args);
        if (own != NOT_MINE) return own;
        if (proxy instanceof org.bukkit.inventory.meta.ItemMeta) {
            Object meta = Shapes.meta(h, name, args);
            if (meta != Shapes.NONE) return meta;
        }
        Object shaped = Shapes.object(proxy, h, name, args);
        if (shaped != Shapes.NONE) return shaped;
        if (proxy instanceof org.bukkit.inventory.meta.BookMeta) {
            Object book = Books.call(h, name, args, m.getReturnType());
            if (book != Shapes.NONE) return book;
        }
        Object[] nargs = Conv.args(args);
        // A null item where the SDK takes one (`setItemInMainHand(null)`): air, as CraftBukkit reads it.
        Class<?>[] params = m.getParameterTypes();
        for (int i = 0; i < n; i++) if (args[i] == null && params[i] == org.bukkit.inventory.ItemStack.class) nargs[i] = Conv.toNative(Items.create(org.bukkit.Material.AIR, 1));
        Object raw = Native.tryCall(h, name, nargs);
        // Bukkit's longer overloads (`teleport(to, cause)`, `closeInventory(reason)`, `giveExp(n, mending)`): the
        // SDK has the plain form. A trailing option (an enum, a flag, nothing) is dropped and the call tried again.
        for (int drop = 1; raw == Native.MISSING && drop <= 2 && drop <= n; drop++) {
            Object last = args[n - drop];
            if (!(last == null || last instanceof Enum<?> || last instanceof Boolean || last instanceof Enum<?>[] || last.getClass().getSimpleName().endsWith("Cause") || last.getClass().getSimpleName().endsWith("Reason"))) break;
            raw = Native.tryCall(h, name, java.util.Arrays.copyOf(nargs, n - drop));
        }
        // Vectors as their three numbers (the server's `[f64; 3]` forms: EntityPortalExitEvent.setAfter...).
        if (raw == Native.MISSING) {
            Object[] arrays = null;
            for (int i = 0; i < n; i++) {
                if (args[i] instanceof org.bukkit.util.Vector v) {
                    if (arrays == null) arrays = nargs.clone();
                    arrays[i] = new double[]{v.getX(), v.getY(), v.getZ()};
                }
            }
            if (arrays != null) raw = Native.tryCall(h, name, arrays);
        }
        // Keys as text: many of the server's forms take ids as strings.
        if (raw == Native.MISSING) {
            Object[] text = Conv.keysAsText(nargs);
            if (text != null) raw = Native.tryCall(h, name, text);
        }
        // A method the server has as a static of the same class (ScoreboardManager.getMainScoreboard...).
        if (raw == Native.MISSING) raw = Native.tryStatic(m.getDeclaringClass().getSimpleName(), name, nargs);
        // Paper's adventure-style setter `x(value)` where the server has `setX(value)`.
        if (raw == Native.MISSING && n == 1 && !name.startsWith("set") && m.getReturnType() == void.class) {
            raw = Native.tryCall(h, "set" + Character.toUpperCase(name.charAt(0)) + name.substring(1), nargs);
        }
        if (raw == Native.MISSING && n == 1 && args[0] == null && name.startsWith("set")) {
            // `meta.setDisplayName(null)`: Paper's Adventure form clears it the same way.
            String bare = Character.toLowerCase(name.charAt(3)) + name.substring(4);
            raw = Native.tryCall(h, bare, nargs);
        }
        // What the SDK has on OfflinePlayer only (getFirstPlayed, getLastPlayed...): asked of the player's offline self.
        if (raw == Native.MISSING && proxy instanceof Player p) {
            Object offline = Native.tryStatic("Bukkit", "getOfflinePlayer", new Object[]{p.getUniqueId().toString()});
            if (offline instanceof H off) {
                raw = Native.tryCall(off, name, nargs);
                Native.release(off.id);
            }
        }
        if (raw == Native.MISSING) {
            if (name.equals("updateInventory") && n == 0) return null;
            if (name.equals("getViewers") && n == 0) return new ArrayList<>();
            if (name.equals("getType") && n == 0 && proxy instanceof org.bukkit.inventory.Inventory) {
                org.bukkit.event.inventory.InventoryType known = Proxies.INVENTORY_TYPES.get(Proxies.invKey(h));
                if (known != null) return known;
            }
            // SDK builders that are their own result (DialogBase, many components): build() is the object itself.
            if (name.equals("build") && n == 0 && m.getReturnType().isInterface()) return Conv.fromNative(h, m.getGenericReturnType());
            if (m.isDefault() && java.lang.reflect.Proxy.isProxyClass(proxy.getClass())) return InvocationHandler.invokeDefault(proxy, m, args);
            // A generated class (HorizonWorld) runs the interface's default itself, as a proxy would.
            if (m.isDefault() && proxy instanceof Proxies.Handled) return USE_DEFAULT;
            String where = m.getDeclaringClass().getSimpleName() + "." + name + "/" + n;
            missing(where);
            throw Report.cant(where);
        }
        if (proxy instanceof org.bukkit.scoreboard.Scoreboard) {
            Object board = BoardViews.board(proxy, h, m, name, args, raw);
            if (board != NOT_MINE) return board;
        }
        Object out = Conv.fromNative(raw, m.getGenericReturnType());
        // Paper's data component values are records: a list or map they don't hold is empty, never null
        // (CustomModelData.colors() of one built with floats only).
        if (out == null && m.getDeclaringClass().getName().startsWith("io.papermc.paper.datacomponent.")) {
            Class<?> r = m.getReturnType();
            if (r == List.class || r == java.util.Collection.class) return List.of();
            if (r == java.util.Set.class) return java.util.Set.of();
            if (r == java.util.Map.class) return java.util.Map.of();
        }
        return out;
    }

    static final Object NOT_MINE = new Object();

    /** Entity.setLastDamageCause by entity key: [event, System.nanoTime()]. */
    private static final Map<String, Object[]> LAST_DAMAGE = new ConcurrentHashMap<>();

    private static final java.util.Set<String> WORLD_CONSTANTS = java.util.Set.of("getMaxHeight", "getMinHeight", "getUID", "getName", "getKey", "getEnvironment", "getSeed", "getLogicalHeight", "getSeaLevel");
    private static final Map<String, Object[]> WORLD_KEPT = new ConcurrentHashMap<>();

    /** worldCall's answer for an interface default the layer has no answer to: the caller runs the default. */
    public static final Object USE_DEFAULT = new Object();

    /** What Java answers itself: the server object, metadata, `getLocation(into)`. */
    /** HorizonWorld's methods (Gen.world: the layer's worlds as Paper's CraftWorld). */
    public static Object worldCall(Object self, H h, Method m, Object[] args) {
        try {
            if ((args == null || args.length == 0) && WORLD_CONSTANTS.contains(m.getName()) && h.key.startsWith("w:")) {
                // What a loaded world never changes, read per block by some plugins (AntiXray's reveal: its height
                // and id for each block update, a whole level query each, most of a tick with 100 players). Kept 5 s,
                // as a world deleted and made again under its name is another world.
                String key = h.key + '|' + m.getName();
                long now = System.nanoTime();
                Object[] kept = WORLD_KEPT.get(key);
                if (kept != null && now - (long) kept[1] < 5_000_000_000L) return kept[0];
                Object answer = call(self, h, m, args);
                if (answer != null) WORLD_KEPT.put(key, new Object[]{answer, now});
                return answer;
            }
            return call(self, h, m, args);
        } catch (RuntimeException | Error e) {
            throw e;
        } catch (Throwable t) {
            throw Players.sneaky(t);
        }
    }

    /** A game mode a plugin just set, by player: [GameMode, System.nanoTime()] until the server's state has it. */
    static final Map<java.util.UUID, Object[]> PENDING_MODE = new ConcurrentHashMap<>();

    /** CraftLivingEntity.getLineOfSight(transparent, maxDistance, maxLength). */
    static List<org.bukkit.block.Block> lineOfSight(org.bukkit.entity.LivingEntity living, java.util.Set<org.bukkit.Material> transparent, int maxDistance, int maxLength) {
        if (transparent == null) transparent = java.util.Set.of(org.bukkit.Material.AIR, org.bukkit.Material.CAVE_AIR, org.bukkit.Material.VOID_AIR);
        if (maxDistance > 120) maxDistance = 120;
        List<org.bukkit.block.Block> blocks = new ArrayList<>();
        java.util.Iterator<org.bukkit.block.Block> itr = new org.bukkit.util.BlockIterator(living, maxDistance);
        while (itr.hasNext()) {
            org.bukkit.block.Block block = itr.next();
            blocks.add(block);
            if (maxLength != 0 && blocks.size() > maxLength) blocks.remove(0);
            if (!transparent.contains(block.getType())) break;
        }
        return blocks;
    }

    @SuppressWarnings("unchecked")
    /** The entities riding one, from its saved data's `Passengers` (each with its UUID as four ints). */
    static List<org.bukkit.entity.Entity> passengers(org.bukkit.entity.Entity e) {
        List<org.bukkit.entity.Entity> out = new ArrayList<>();
        try {
            String q = Native.query("{\"op\":\"entity\",\"id\":" + e.getEntityId() + ",\"data\":true}");
            com.google.gson.JsonElement root = q == null ? null : com.google.gson.JsonParser.parseString(q);
            com.google.gson.JsonElement data = root != null && root.isJsonObject() ? root.getAsJsonObject().get("data") : null;
            com.google.gson.JsonElement riders = data != null && data.isJsonObject() ? data.getAsJsonObject().get("Passengers") : null;
            if (riders == null || !riders.isJsonArray()) return out;
            for (com.google.gson.JsonElement rider : riders.getAsJsonArray()) {
                com.google.gson.JsonElement id = rider.isJsonObject() ? rider.getAsJsonObject().get("UUID") : null;
                if (id == null || !id.isJsonArray() || id.getAsJsonArray().size() != 4) continue;
                com.google.gson.JsonArray a = id.getAsJsonArray();
                UUID uuid = new UUID(((long) a.get(0).getAsInt() << 32) | (a.get(1).getAsInt() & 0xFFFFFFFFL), ((long) a.get(2).getAsInt() << 32) | (a.get(3).getAsInt() & 0xFFFFFFFFL));
                org.bukkit.entity.Entity found = org.bukkit.Bukkit.getEntity(uuid);
                if (found != null) out.add(found);
            }
        } catch (RuntimeException ignored) {
        }
        return out;
    }

    private static Object own(Object proxy, H h, Method m, String name, int n, Object[] args) {
        switch (name) {
            case "setLastDamageCause":
                // Entity.setLastDamageCause (EssentialsX's /suicide sets its own EntityDamageEvent before the death):
                // kept a second for getLastDamageCause, as the death that follows reads it.
                if (n == 1 && proxy instanceof org.bukkit.entity.Entity) {
                    if (args[0] == null) LAST_DAMAGE.remove(h.key);
                    else LAST_DAMAGE.put(h.key, new Object[]{args[0], System.nanoTime()});
                    return null;
                }
                break;
            case "getLastDamageCause":
                if (n == 0 && proxy instanceof org.bukkit.entity.Entity) {
                    Object[] kept = LAST_DAMAGE.get(h.key);
                    if (kept != null && System.nanoTime() - (long) kept[1] < 1_000_000_000L) return kept[0];
                    if (kept != null) LAST_DAMAGE.remove(h.key);
                }
                break;
            case "getClientOption":
                // A player's or a connection's client options, as Paper's types (ClientOptions).
                if (n == 1 && args[0] instanceof com.destroystokyo.paper.ClientOption<?> option) {
                    Object value = ClientOptions.of(h, option);
                    if (value != null) return value;
                }
                break;
            case "teleportAsync":
                // Paper's teleportAsync(Location, TeleportCause, TeleportFlag...) (WildRTP): the teleport now, its
                // answer as the finished future (the chunks are the server's to load; the flags are its defaults).
                if (n >= 2 && args[0] instanceof org.bukkit.Location to && args[1] instanceof org.bukkit.event.player.PlayerTeleportEvent.TeleportCause cause
                        && proxy instanceof org.bukkit.entity.Entity e) {
                    return java.util.concurrent.CompletableFuture.completedFuture(e.teleport(to, cause));
                }
                break;
            case "getPassengers":
            case "getPassenger":
            case "isEmpty":
                // Entity riders as Bukkit's entities, from the entity's saved `Passengers` (their UUIDs): the SDK
                // gave the raw saved data (getPassengers) or, on players, a boolean (EliteMobs' getPassengers().isEmpty()).
                if (n == 0 && proxy instanceof org.bukkit.entity.Entity e) {
                    List<org.bukkit.entity.Entity> riders = passengers(e);
                    if (name.equals("isEmpty")) return riders.isEmpty();
                    if (name.equals("getPassenger")) return riders.isEmpty() ? null : riders.get(0);
                    return riders;
                }
                break;
            case "getState":
                // CraftBlockData.getState(): the game's BlockState, parsed from the data's text (NmsBlockState).
                if (n == 0 && proxy instanceof org.bukkit.block.data.BlockData data && proxy instanceof NmsBlockState) {
                    return horizon.javalayer.nms.Hollow.blockState(data.getAsString());
                }
                break;
            case "addPassenger":
                // Bukkit answers whether the rider got on (the SDK's says nothing): it does, unless it is the entity itself.
                if (n == 1 && proxy instanceof org.bukkit.entity.Entity e && args[0] instanceof org.bukkit.entity.Entity rider) {
                    if (rider.getUniqueId().equals(e.getUniqueId())) return false;
                    Object raw = Native.tryCall(h, name, Conv.args(args));
                    if (raw != Native.MISSING) return true;
                }
                break;
            case "setEnchantmentGlintOverride":
                // null takes the override away (Paper: the component unset); on meta without one that is nothing to do
                // (EliteMobs passes null for every custom item without a glint). One that has it gets false: no glint
                // forced either way is the closest the SDK's setter can say.
                if (n == 1 && args[0] == null && proxy instanceof org.bukkit.inventory.meta.ItemMeta meta) {
                    if (meta.hasEnchantmentGlintOverride()) meta.setEnchantmentGlintOverride(Boolean.FALSE);
                    return null;
                }
                break;
            case "getEntitiesByClass":
            case "getEntitiesByClasses":
                if (n == 1 && proxy instanceof org.bukkit.World w) {
                    List<Object> out = new ArrayList<>();
                    Class<?>[] wanted = args[0] instanceof Class<?>[] many ? many : new Class<?>[]{(Class<?>) args[0]};
                    for (org.bukkit.entity.Entity e : w.getEntities()) for (Class<?> c : wanted) if (c.isInstance(e)) { out.add(e); break; }
                    return out;
                }
                break;
            case "isTamed":
            case "getOwnerUniqueId":
            case "getOwner":
                // Tameable: the server's answer where it has one (wolves); else the entity's saved data, as the game
                // keeps it: `Owner` (a UUID) for tamable animals, `Tame` for horses and llamas (WorldGuard asks
                // isTamed of every spawning creature).
                if (n == 0 && proxy instanceof org.bukkit.entity.Tameable && proxy instanceof org.bukkit.entity.Entity e) {
                    Object raw = Native.tryCall(h, name, new Object[0]);
                    if (raw != Native.MISSING) return Conv.fromNative(raw, m.getGenericReturnType());
                    UUID owner = null;
                    boolean tame = false;
                    try {
                        String q = Native.query("{\"op\":\"entity\",\"id\":" + e.getEntityId() + ",\"data\":true}");
                        com.google.gson.JsonElement root = q == null ? null : com.google.gson.JsonParser.parseString(q);
                        com.google.gson.JsonElement data = root != null && root.isJsonObject() ? root.getAsJsonObject().get("data") : null;
                        if (data != null && data.isJsonObject()) {
                            com.google.gson.JsonElement o = data.getAsJsonObject().get("Owner");
                            if (o != null && o.isJsonArray() && o.getAsJsonArray().size() == 4) {
                                com.google.gson.JsonArray a = o.getAsJsonArray();
                                owner = new UUID((long) a.get(0).getAsInt() << 32 | (a.get(1).getAsInt() & 0xFFFFFFFFL), (long) a.get(2).getAsInt() << 32 | (a.get(3).getAsInt() & 0xFFFFFFFFL));
                            } else if (o != null && o.isJsonPrimitive()) {
                                try { owner = UUID.fromString(o.getAsString()); } catch (IllegalArgumentException ignored) { }
                            }
                            com.google.gson.JsonElement t = data.getAsJsonObject().get("Tame");
                            tame = owner != null || t != null && t.isJsonPrimitive() && t.getAsInt() != 0;
                        }
                    } catch (RuntimeException ignored) {
                        // untamed, then
                    }
                    if (name.equals("isTamed")) return tame;
                    if (name.equals("getOwnerUniqueId")) return owner;
                    return owner == null ? null : org.bukkit.Bukkit.getOfflinePlayer(owner);
                }
                break;
            case "getListeningPluginChannels":
                // What the client registered, and what plugins added for it (CraftPlayer.addChannel).
                if (n == 0 && proxy instanceof Player p) {
                    Set<String> mine = Players.ADDED_CHANNELS.get(p.getUniqueId());
                    Object raw = Native.tryCall(h, name, new Object[0]);
                    Set<String> out = new TreeSet<>();
                    Object list = raw == Native.MISSING ? null : Conv.fromNative(raw, List.class);
                    if (list instanceof Collection<?> c) for (Object x : c) out.add(String.valueOf(x));
                    if (mine != null) out.addAll(mine);
                    return out;
                }
                break;
            case "setRotation":
                // Entity.setRotation (CraftEntity: yaw normalized, pitch clamped, no event): the entity's saved
                // Rotation merged in place; a player (whose data can't be merged) turned where it stands.
                if (n == 2 && proxy instanceof org.bukkit.entity.Entity e && args[0] instanceof Float yawArg && args[1] instanceof Float pitchArg) {
                    Object raw = Native.tryCall(h, name, Conv.args(args));
                    if (raw != Native.MISSING) return null;
                    float yaw = Location.normalizeYaw(yawArg), pitch = Location.normalizePitch(pitchArg);
                    if (Float.isNaN(yaw) || Float.isNaN(pitch)) return null;
                    if (e instanceof Player p) {
                        Location at = p.getLocation();
                        at.setYaw(yaw);
                        at.setPitch(pitch);
                        p.teleport(at);
                        return null;
                    }
                    // As the SDK's Entity.merge: the live entity, else one the server keeps aside (displays).
                    String snbt = "{Rotation:[" + yaw + "f," + pitch + "f]}";
                    String done = Native.query("{\"op\":\"entity_merge\",\"id\":" + e.getEntityId() + ",\"snbt\":\"" + snbt + "\"}");
                    if (!"true".equals(String.valueOf(done).trim())) {
                        Native.query("{\"op\":\"entity_merge_kept\",\"id\":" + e.getEntityId() + ",\"dimension\":\"" + e.getWorld().getKey() + "\",\"snbt\":\"" + snbt + "\"}");
                    }
                    return null;
                }
                break;
            case "getPortalCooldown":
                // Entity.getPortalCooldown: the entity's saved PortalCooldown (ticks), 0 when it has none.
                if (n == 0 && proxy instanceof org.bukkit.entity.Entity e) {
                    try {
                        String q = Native.query("{\"op\":\"entity\",\"id\":" + e.getEntityId() + ",\"data\":true}");
                        com.google.gson.JsonElement root = q == null ? null : com.google.gson.JsonParser.parseString(q);
                        com.google.gson.JsonElement data = root != null && root.isJsonObject() ? root.getAsJsonObject().get("data") : null;
                        com.google.gson.JsonElement cd = data != null && data.isJsonObject() ? data.getAsJsonObject().get("PortalCooldown") : null;
                        return cd != null && cd.isJsonPrimitive() ? cd.getAsInt() : 0;
                    } catch (RuntimeException ignored) {
                        return 0;
                    }
                }
                break;
            case "setGameMode":
                // The server applies it on the player's own region thread: a read right after (EssentialsX's
                // /gamemode reports getGameMode() at once) would see the old mode. Paper's is immediate.
                if (n == 1 && proxy instanceof org.bukkit.entity.Player p && args[0] instanceof org.bukkit.GameMode mode) {
                    Object raw = Native.tryCall(h, name, Conv.args(args));
                    if (raw == Native.MISSING) break;
                    PENDING_MODE.put(p.getUniqueId(), new Object[]{mode, System.nanoTime()});
                    return null;
                }
                break;
            case "getGameMode":
                if (n == 0 && proxy instanceof org.bukkit.entity.Player p) {
                    Object[] pending = PENDING_MODE.get(p.getUniqueId());
                    if (pending == null) break;
                    Object raw = Native.tryCall(h, name, new Object[0]);
                    org.bukkit.GameMode now = raw == Native.MISSING ? null : (org.bukkit.GameMode) Conv.fromNative(raw, org.bukkit.GameMode.class);
                    // Until the server's state has it (or a second has passed: the change was refused).
                    if (now == pending[0] || System.nanoTime() - (long) pending[1] > 1_000_000_000L) {
                        PENDING_MODE.remove(p.getUniqueId(), pending);
                        return now;
                    }
                    return pending[0];
                }
                break;
            case "getLineOfSight":
            case "getTargetBlock":
            case "getLastTwoTargetBlocks":
                // CraftLivingEntity's, line for line: Bukkit's BlockIterator from the eyes, at most 120 blocks, the
                // given materials (or the three airs for null) seen through; the target is the last block of the line.
                if (n == 2 && proxy instanceof org.bukkit.entity.LivingEntity living && args[1] instanceof Integer max) {
                    int keep = name.equals("getTargetBlock") ? 1 : name.equals("getLastTwoTargetBlocks") ? 2 : 0;
                    List<org.bukkit.block.Block> line = lineOfSight(living, (java.util.Set<org.bukkit.Material>) args[0], max, keep);
                    return name.equals("getTargetBlock") ? (line.isEmpty() ? null : line.get(0)) : line;
                }
                break;
            case "getEntities":
            case "getLivingEntities":
                // Each entity once, as the world's entity list holds it (the server's answer can name one per region
                // or chunk section it touches).
                if (n == 0 && (proxy instanceof org.bukkit.World || proxy instanceof org.bukkit.Chunk)) {
                    Object raw = Native.tryCall(h, name, new Object[0]);
                    if (raw != Native.MISSING) {
                        Object list = Conv.fromNative(raw, m.getGenericReturnType());
                        if (list instanceof java.util.Collection<?> c) {
                            java.util.Map<java.util.UUID, Object> unique = new java.util.LinkedHashMap<>();
                            for (Object o : c) if (o instanceof org.bukkit.entity.Entity e) unique.putIfAbsent(e.getUniqueId(), e);
                            if (unique.size() == c.size()) return list;
                            if (list instanceof Object[]) return unique.values().toArray(new org.bukkit.entity.Entity[0]);
                            return new ArrayList<>(unique.values());
                        }
                        if (list instanceof org.bukkit.entity.Entity[] arr) {
                            java.util.Map<java.util.UUID, org.bukkit.entity.Entity> unique = new java.util.LinkedHashMap<>();
                            for (org.bukkit.entity.Entity e : arr) unique.putIfAbsent(e.getUniqueId(), e);
                            return unique.values().toArray(new org.bukkit.entity.Entity[0]);
                        }
                        return list;
                    }
                }
                break;
            case "getWorldFolder":
            case "getWorldPath":
                // Paper's paths are relative to the server folder (`.\world\dimensions\minecraft\overworld`): the
                // server's world container is ".", the folder it runs in.
                if (n == 0 && proxy instanceof org.bukkit.World) {
                    Object raw = Native.tryCall(h, name, new Object[0]);
                    if (raw != Native.MISSING && raw != null) {
                        java.nio.file.Path abs = java.nio.file.Paths.get(String.valueOf(raw)).toAbsolutePath().normalize();
                        java.nio.file.Path cwd = java.nio.file.Paths.get("").toAbsolutePath().normalize();
                        java.nio.file.Path rel = abs.startsWith(cwd) ? java.nio.file.Paths.get(".").resolve(cwd.relativize(abs)) : abs;
                        return name.equals("getWorldPath") ? rel : rel.toFile();
                    }
                }
                break;
            case "isOp":
                // ServerCommandSender.isOp: the console is always an operator.
                if (n == 0 && proxy instanceof org.bukkit.command.ConsoleCommandSender) return true;
                break;
            case "isConversing":
                if (n == 0) return Conversations.isConversing(proxy);
                break;
            case "beginConversation":
                if (n == 1 && args[0] instanceof org.bukkit.conversations.Conversation c) return Conversations.begin(proxy, c);
                break;
            case "abandonConversation":
                if (n >= 1 && args[0] instanceof org.bukkit.conversations.Conversation c) {
                    Conversations.abandon(proxy, c, n == 2 ? (org.bukkit.conversations.ConversationAbandonedEvent) args[1]
                            : new org.bukkit.conversations.ConversationAbandonedEvent(c, new org.bukkit.conversations.ManuallyAbandonedConversationCanceller()));
                    return null;
                }
                break;
            case "acceptConversationInput":
                if (n == 1) { Conversations.accept(proxy, String.valueOf(args[0])); return null; }
                break;
            case "getEffectivePermissions":
                if (n == 0 && proxy instanceof org.bukkit.permissions.Permissible p) return Conversations.effectivePermissions(p);
                break;
            case "getBlockState":
                if (n == 0 && proxy instanceof org.bukkit.inventory.meta.BlockStateMeta) return ItemContainers.state(h, proxy);
                break;
            case "setBlockState":
                if (n == 1 && proxy instanceof org.bukkit.inventory.meta.BlockStateMeta) { ItemContainers.setState(h, args[0]); return null; }
                break;
            case "hasBlockState":
                if (n == 0 && proxy instanceof org.bukkit.inventory.meta.BlockStateMeta) return Native.componentJson(h, "minecraft:container") != null || Native.componentJson(h, "minecraft:block_entity_data") != null;
                break;
            case "update":
                // PlayerProfile.update(): a completed copy, as CraftPlayerProfile.complete(true) makes it. A name
                // without an id gets the id the server knows it by (its user cache), else on an offline-mode server
                // the offline id, as Paper's completeFromCache gives it (AdvancedReports' /report <name> goes on
                // with that profile; with none it said the player wasn't found).
                if (n == 0 && proxy instanceof org.bukkit.profile.PlayerProfile profile) return java.util.concurrent.CompletableFuture.completedFuture(completed(profile));
                break;
            case "setVersion":
                // CraftMetaItem keeps the saved data version for its data fixer; the layer's metas are current already.
                if (n == 1 && proxy instanceof org.bukkit.inventory.meta.ItemMeta) return null;
                break;
            case "getBoundingBoxes":
                // VoxelShape.getBoundingBoxes(): the server's boxes (min x y z, max x y z) as Bukkit's BoundingBoxes
                // (GSit checks the block under a player before it seats them).
                if (n == 0 && proxy instanceof org.bukkit.util.VoxelShape) {
                    Object raw = Native.tryCall(h, "getBoundingBoxes", new Object[0]);
                    java.util.List<org.bukkit.util.BoundingBox> boxes = new java.util.ArrayList<>();
                    if (raw instanceof Iterable<?> all) {
                        for (Object one : all) {
                            if (!(one instanceof Iterable<?> six)) continue;
                            double[] d = new double[6];
                            int i = 0;
                            for (Object x : six) if (i < 6 && x instanceof Number num) d[i++] = num.doubleValue();
                            if (i == 6) boxes.add(new org.bukkit.util.BoundingBox(d[0], d[1], d[2], d[3], d[4], d[5]));
                        }
                    }
                    return boxes;
                }
                break;
            case "getRenderers":
                if (n == 0 && proxy instanceof org.bukkit.map.MapView) return MapRenderers.of(h);
                break;
            case "addRenderer":
                if (n == 1 && proxy instanceof org.bukkit.map.MapView mv) { MapRenderers.add(h, mv, (org.bukkit.map.MapRenderer) args[0]); return null; }
                break;
            case "removeRenderer":
                if (n == 1 && proxy instanceof org.bukkit.map.MapView) return MapRenderers.remove(h, (org.bukkit.map.MapRenderer) args[0]);
                break;
            case "showDialog":
                if (n == 1 && proxy instanceof Player p && args[0] instanceof Dialogs.Made) return Dialogs.show(p, args[0]) ? null : null;
                break;
            case "openInventory":
                if (n == 1 && m.getReturnType() == org.bukkit.inventory.InventoryView.class && proxy instanceof org.bukkit.entity.HumanEntity human) {
                    Native.call(h, "openInventory", Conv.args(args));
                    return human.getOpenInventory();
                }
                break;
            case "getStatistic":
            case "setStatistic":
            case "incrementStatistic":
            case "decrementStatistic":
                if (n >= 1 && args[0] instanceof org.bukkit.Statistic stat) return Stats.call(h, name, stat, args);
                break;
            case "spawn":
                if (n >= 2 && args[0] instanceof Location at && args[1] instanceof Class<?> type) return Stats.spawn(h, at, type, args);
                break;
            case "getAddress":
                if (n == 0 && m.getReturnType() == java.net.InetSocketAddress.class) {
                    Object raw = Native.tryCall(h, "getAddress", new Object[0]);
                    Object address = raw == Native.MISSING ? null : Conv.fromNative(raw, java.net.InetSocketAddress.class);
                    return address != null ? address : new java.net.InetSocketAddress(java.net.InetAddress.getLoopbackAddress(), 0);
                }
                break;
            case "getScheduler":
                if (n == 0 && proxy instanceof org.bukkit.entity.Entity e && m.getReturnType() == io.papermc.paper.threadedregions.scheduler.EntityScheduler.class) return Folia.entity(e);
                break;
            case "getServer":
                if (n == 0 && Server.class.isAssignableFrom(m.getReturnType())) return Bridge.server();
                break;
            case "setMetadata":
                if (n == 2) {
                    MetadataValue value = (MetadataValue) args[1];
                    List<MetadataValue> list = METADATA.computeIfAbsent(key(h), k -> new ConcurrentHashMap<>()).computeIfAbsent((String) args[0], k -> new ArrayList<>());
                    synchronized (list) {
                        list.removeIf(v -> v.getOwningPlugin() == value.getOwningPlugin());
                        list.add(value);
                    }
                    return null;
                }
                break;
            case "getMetadata":
                if (n == 1) {
                    Map<String, List<MetadataValue>> all = METADATA.get(key(h));
                    List<MetadataValue> list = all == null ? null : all.get((String) args[0]);
                    if (list == null) return new ArrayList<MetadataValue>();
                    synchronized (list) { return new ArrayList<>(list); }
                }
                break;
            case "hasMetadata":
                if (n == 1) {
                    Map<String, List<MetadataValue>> all = METADATA.get(key(h));
                    List<MetadataValue> list = all == null ? null : all.get((String) args[0]);
                    return list != null && !list.isEmpty();
                }
                break;
            case "removeMetadata":
                if (n == 2) {
                    Map<String, List<MetadataValue>> all = METADATA.get(key(h));
                    List<MetadataValue> list = all == null ? null : all.get((String) args[0]);
                    if (list != null) synchronized (list) { list.removeIf(v -> v.getOwningPlugin() == args[1]); }
                    return null;
                }
                break;
            case "getLocation":
                if (n == 1 && args[0] instanceof Location into) {
                    Location now = (Location) Conv.fromNative(Native.call(h, "getLocation", new Object[0]), Location.class);
                    if (now == null) return null;
                    into.setWorld(now.getWorld());
                    into.setX(now.getX());
                    into.setY(now.getY());
                    into.setZ(now.getZ());
                    into.setYaw(now.getYaw());
                    into.setPitch(now.getPitch());
                    return into;
                }
                break;
            case "spigot":
                if (n == 0 && proxy instanceof Player p) return Spigot.of(p);
                if (n == 0 && proxy instanceof org.bukkit.command.CommandSender s) return Spigot.sender(s);
                break;
            case "canSee":
                if (n == 1 && args[0] instanceof Plugin) break;
                break;
            case "isOnline":
                break;
            default:
                break;
        }
        return NOT_MINE;
    }
}
