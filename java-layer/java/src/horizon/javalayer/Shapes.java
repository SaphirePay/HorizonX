package horizon.javalayer;

import org.bukkit.Bukkit;
import org.bukkit.World;
import org.bukkit.WorldCreator;
import org.bukkit.command.CommandSender;
import org.bukkit.entity.Player;

import java.util.ArrayList;
import java.util.List;

/**
 * Calls whose Java shape differs from the server's (the SDK takes fewer or other arguments, or answers it on another
 * object): each written out here once, by what Paper does.
 */
public final class Shapes {
    private Shapes() {}

    static final Object NONE = new Object();

    /** Server methods. NONE when not one of these. */
    static Object server(String name, Object[] args, Class<?> returns) {
        int n = args == null ? 0 : args.length;
        switch (name) {
            case "selectEntities":
                // Vanilla's entity selectors from the sender's place (Selectors).
                if (n == 2 && args[1] instanceof String selector) return Selectors.select((CommandSender) args[0], selector);
                break;
            case "broadcast":
                if (n == 2 && args[1] instanceof String permission) {
                    int count = 0;
                    for (Player p : Bukkit.getOnlinePlayers()) {
                        if (!p.hasPermission(permission)) continue;
                        if (args[0] instanceof String s) p.sendMessage(s);
                        else p.sendMessage((net.kyori.adventure.text.Component) args[0]);
                        count++;
                    }
                    CommandSender console = Bukkit.getConsoleSender();
                    if (args[0] instanceof String s) console.sendMessage(s);
                    else console.sendMessage((net.kyori.adventure.text.Component) args[0]);
                    return count;
                }
                break;
            case "createWorld":
                if (n == 1 && args[0] instanceof WorldCreator c) return createWorld(c);
                break;
            case "sendPluginMessage":
                if (n == 3 && args[2] instanceof byte[] bytes) {
                    for (Player p : Bukkit.getOnlinePlayers()) p.sendPluginMessage((org.bukkit.plugin.Plugin) args[0], (String) args[1], bytes);
                    return null;
                }
                break;
            case "createBlockData":
                // Paper: the block's default state, every property set (CraftBlockData.newData), and a string is parsed
                // on top of that default (BlockStateParser), so unnamed properties keep their default values.
                if (n == 1 && args[0] instanceof org.bukkit.Material mat && mat.isBlock() && !mat.isLegacy()) {
                    org.bukkit.block.data.BlockData full = defaultState(mat);
                    if (full != null) return full;
                }
                if (n == 1 && args[0] instanceof String text && !text.isEmpty()) {
                    Object raw = Native.tryStatic("Bukkit", "createBlockData", Conv.args(args));
                    if (raw != Native.MISSING && raw != null) {
                        org.bukkit.block.data.BlockData parsed = (org.bukkit.block.data.BlockData) Conv.fromNative(raw, org.bukkit.block.data.BlockData.class);
                        return withDefaults(parsed);
                    }
                }
                if (n == 2 && args[0] instanceof org.bukkit.Material mat) {
                    if (args[1] instanceof String data) {
                        String text = data.isEmpty() ? mat.getKey().toString() : data.startsWith("[") ? mat.getKey() + data : data.contains(":") || data.contains("[") ? data : mat.getKey() + "[" + data + "]";
                        return Bukkit.createBlockData(text);
                    }
                    if (args[1] instanceof java.util.function.Consumer<?> c) {
                        org.bukkit.block.data.BlockData d = Bukkit.createBlockData(mat);
                        ((java.util.function.Consumer<org.bukkit.block.data.BlockData>) c).accept(d);
                        return d;
                    }
                }
                break;
            case "createInventory":
                if (n >= 2 && args[1] instanceof org.bukkit.event.inventory.InventoryType type) {
                    Object raw = Native.tryStatic("Bukkit", "createInventory", Conv.args(args));
                    if (raw instanceof H h) return Proxies.inventory(h, type);
                }
                break;
            case "audiences": {
                List<net.kyori.adventure.audience.Audience> out = new ArrayList<>(Bukkit.getOnlinePlayers());
                out.add(Bukkit.getConsoleSender());
                return out;
            }
            case "getServerConfig":
                if (n == 0) {
                    return java.lang.reflect.Proxy.newProxyInstance(Shapes.class.getClassLoader(), new Class<?>[]{io.papermc.paper.configuration.ServerConfiguration.class}, (p, m, a) -> switch (m.getName()) {
                        case "isProxyOnlineMode" -> Bukkit.getOnlineMode();
                        case "isProxyEnabled" -> Bukkit.spigot().getConfig().getBoolean("settings.bungeecord", false);
                        case "hashCode" -> System.identityHashCode(p);
                        case "equals" -> p == a[0];
                        default -> "io.papermc.paper.configuration.PaperServerConfiguration@" + Integer.toHexString(System.identityHashCode(p));
                    });
                }
                break;
            default:
                break;
        }
        return NONE;
    }

    /** WorldCreator.createWorld / Server.createWorld: the server's WorldCreator, filled from Java's. */
    static World createWorld(WorldCreator c) {
        Object w = Native.construct("WorldCreator", new Object[]{c.name()});
        if (w == null || w == Native.MISSING) throw Report.cant("Server.createWorld");
        if (c.environment() != null) Native.tryCall(w, "environment", new Object[]{c.environment().name()});
        if (c.type() != null) Native.tryCall(w, "type", new Object[]{c.type().name()});
        Native.tryCall(w, "seed", new Object[]{c.seed()});
        Native.tryCall(w, "generateStructures", new Object[]{c.generateStructures()});
        Native.tryCall(w, "hardcore", new Object[]{c.hardcore()});
        if (c.generator() != null) {
            Generators.created(c);
            if (Native.tryCall(w, "generator", Conv.args(new Object[]{c.generator()})) == Native.MISSING) throw Report.cant("WorldCreator.generator");
        }
        if (c.biomeProvider() != null && Native.tryCall(w, "biomeProvider", Conv.args(new Object[]{c.biomeProvider()})) == Native.MISSING) throw Report.cant("WorldCreator.biomeProvider");
        Object raw = Native.tryStatic("Bukkit", "createWorld", new Object[]{w});
        if (raw == Native.MISSING) throw Report.cant("Server.createWorld");
        return (World) Conv.fromNative(raw, World.class);
    }

    /** Calls on any game object with a known other shape. NONE when not one of these. */
    static Object object(Object proxy, H h, String name, Object[] args) {
        int n = args == null ? 0 : args.length;
        Object named = slot(proxy, name, args);
        if (named != NONE) return named;
        switch (name) {
            case "spawnParticle":
                // 26.3's one abstract form, which the shorter overloads reach through default methods.
                if (n == 14 && proxy instanceof org.bukkit.entity.Player p && args[0] instanceof org.bukkit.Particle) return Particles.forPlayer(p, args);
                break;
            case "setItem":
                // PlayerInventory.setItem(EquipmentSlot, item): the slot's own setter.
                if (n == 2 && proxy instanceof org.bukkit.inventory.PlayerInventory inv && args[0] instanceof org.bukkit.inventory.EquipmentSlot slot) {
                    org.bukkit.inventory.ItemStack it = (org.bukkit.inventory.ItemStack) args[1];
                    switch (slot) {
                        case HAND -> inv.setItemInMainHand(it);
                        case OFF_HAND -> inv.setItemInOffHand(it);
                        case FEET -> inv.setBoots(it);
                        case LEGS -> inv.setLeggings(it);
                        case CHEST -> inv.setChestplate(it);
                        case HEAD -> inv.setHelmet(it);
                        default -> { }
                    }
                    return null;
                }
                break;
            case "getItem":
                if (n == 1 && proxy instanceof org.bukkit.inventory.PlayerInventory inv && args[0] instanceof org.bukkit.inventory.EquipmentSlot slot) {
                    org.bukkit.inventory.ItemStack it = switch (slot) {
                        case HAND -> inv.getItemInMainHand();
                        case OFF_HAND -> inv.getItemInOffHand();
                        case FEET -> inv.getBoots();
                        case LEGS -> inv.getLeggings();
                        case CHEST -> inv.getChestplate();
                        case HEAD -> inv.getHelmet();
                        default -> null;
                    };
                    return it != null ? it : org.bukkit.inventory.ItemStack.empty();
                }
                break;
            case "playerListName":
                // The Adventure getter over the legacy one the server answers.
                if (n == 0 && proxy instanceof org.bukkit.entity.Player p) {
                    Object raw = Native.tryCall(h, "getPlayerListName", new Object[0]);
                    String text = raw instanceof String s ? s : p.getName();
                    return net.kyori.adventure.text.serializer.legacy.LegacyComponentSerializer.legacySection().deserialize(text);
                }
                break;
            case "locale":
                // Player.locale(): the client's language as a Locale (Paper's Translator.parseLocale of `en_us`).
                if (n == 0 && proxy instanceof org.bukkit.entity.Player p) {
                    java.util.Locale parsed = net.kyori.adventure.translation.Translator.parseLocale(p.getLocale());
                    return parsed != null ? parsed : java.util.Locale.US;
                }
                break;
            case "sendBlockChange":
                // The BlockData form: the server takes the block as its state text.
                if (n == 2 && proxy instanceof org.bukkit.entity.Player && args[0] instanceof org.bukkit.Location at && args[1] instanceof org.bukkit.block.data.BlockData data) {
                    Object raw = Native.tryCall(h, "sendBlockChange", new Object[]{Conv.toNative(at), data.getAsString()});
                    if (raw != Native.MISSING) return null;
                }
                break;
            case "setSpawnLocation":
                if (n == 4 && proxy instanceof World w && args[0] instanceof Integer x) {
                    org.bukkit.Location l = new org.bukkit.Location(w, x, (Integer) args[1], (Integer) args[2], ((Number) args[3]).floatValue(), 0);
                    return w.setSpawnLocation(l);
                }
                if (n == 3 && proxy instanceof World w && args[0] instanceof Integer x) return w.setSpawnLocation(new org.bukkit.Location(w, x, (Integer) args[1], (Integer) args[2]));
                break;
            case "isPermissionSet":
                if (n == 1 && proxy instanceof org.bukkit.permissions.Permissible) {
                    String perm = args[0] instanceof org.bukkit.permissions.Permission p ? p.getName() : String.valueOf(args[0]);
                    Object raw = Native.tryCall(h, "isPermissionSet", new Object[]{perm});
                    return raw instanceof Boolean b ? b : false;
                }
                break;
            case "getPersistentDataContainer":
                // An item meta's and an entity's: their own stored values (custom_data / BukkitValues), read back in
                // the type asked for as Paper does (a BYTE as a Byte).
                if (n == 0 && proxy instanceof org.bukkit.inventory.meta.ItemMeta) return HolderPdc.ofItem(h);
                if (n == 0 && proxy instanceof org.bukkit.entity.Entity e && !(proxy instanceof Player)) return HolderPdc.ofEntity(h, e);
                // Worlds and chunks: the server keeps no data for them, the layer does (FilePdc).
                if (n == 0 && proxy instanceof World w) {
                    Object raw = Native.tryCall(h, name, new Object[0]);
                    if (raw != Native.MISSING) return Conv.fromNative(raw, org.bukkit.persistence.PersistentDataContainer.class);
                    return FilePdc.of(w, "world:" + w.getName());
                }
                if (n == 0 && proxy instanceof org.bukkit.block.TileState t) {
                    Object raw = Native.tryCall(h, name, new Object[0]);
                    if (raw != Native.MISSING) return Conv.fromNative(raw, org.bukkit.persistence.PersistentDataContainer.class);
                    return FilePdc.of(t.getWorld(), "block:" + t.getX() + ":" + t.getY() + ":" + t.getZ());
                }
                if (n == 0 && proxy instanceof org.bukkit.Chunk c) {
                    Object raw = Native.tryCall(h, name, new Object[0]);
                    if (raw != Native.MISSING) return Conv.fromNative(raw, org.bukkit.persistence.PersistentDataContainer.class);
                    return FilePdc.of(c.getWorld(), "chunk:" + c.getX() + ":" + c.getZ());
                }
                break;
            case "multiline":
                if (n == 1 && args[0] instanceof io.papermc.paper.registry.data.dialog.input.TextDialogInput.MultilineOptions o) {
                    Native.tryCall(h, "multiline", new Object[]{o.maxLines(), o.height()});
                    return proxy;
                }
                break;
            case "getSnapshotInventory":
            case "getInventory":
            case "getBlockInventory":
                if (n == 0 && proxy instanceof org.bukkit.block.BlockState state) {
                    Object raw = Native.tryCall(h, name.equals("getSnapshotInventory") ? "getInventory" : name, new Object[0]);
                    if (raw == Native.MISSING && name.equals("getBlockInventory")) raw = Native.tryCall(h, "getInventory", new Object[0]);
                    org.bukkit.event.inventory.InventoryType type = BlockStates.inventoryType(state.getType());
                    if (raw instanceof H ih && type != null) return Proxies.inventory(ih, type);
                    if (raw != Native.MISSING) return Conv.fromNative(raw, org.bukkit.inventory.Inventory.class);
                }
                break;
            case "setArmorContents":
                if (n == 1 && args[0] instanceof org.bukkit.inventory.ItemStack[] armor) {
                    org.bukkit.inventory.EntityEquipment eq = proxy instanceof org.bukkit.inventory.EntityEquipment e ? e : null;
                    org.bukkit.inventory.PlayerInventory inv = proxy instanceof org.bukkit.inventory.PlayerInventory p ? p : null;
                    for (int i = 0; i < 4 && i < armor.length; i++) {
                        org.bukkit.inventory.ItemStack it = armor[i];
                        switch (i) {
                            case 0 -> { if (eq != null) eq.setBoots(it); else if (inv != null) inv.setBoots(it); }
                            case 1 -> { if (eq != null) eq.setLeggings(it); else if (inv != null) inv.setLeggings(it); }
                            case 2 -> { if (eq != null) eq.setChestplate(it); else if (inv != null) inv.setChestplate(it); }
                            default -> { if (eq != null) eq.setHelmet(it); else if (inv != null) inv.setHelmet(it); }
                        }
                    }
                    return null;
                }
                break;
            case "setProperty":
                if (n == 1 && args[0] instanceof com.destroystokyo.paper.profile.ProfileProperty p) {
                    Native.tryCall(h, "setProperty", new Object[]{p.getName(), p.getValue(), p.getSignature()});
                    return null;
                }
                break;
            case "addProperty":
            case "addProperties":
                // ResolvableProfile.Builder: profile properties as the server reads them, {name, value, signature}.
                if (n == 1 && (args[0] instanceof com.destroystokyo.paper.profile.ProfileProperty || args[0] instanceof java.util.Collection<?>)) {
                    java.util.function.Function<Object, Object> one = o -> {
                        com.destroystokyo.paper.profile.ProfileProperty pp = (com.destroystokyo.paper.profile.ProfileProperty) o;
                        java.util.Map<String, Object> m = new java.util.LinkedHashMap<>();
                        m.put("name", pp.getName());
                        m.put("value", pp.getValue());
                        if (pp.getSignature() != null) m.put("signature", pp.getSignature());
                        return m;
                    };
                    Object arg = args[0] instanceof java.util.Collection<?> c ? c.stream().map(one).toList() : one.apply(args[0]);
                    Object raw = Native.tryCall(h, name, new Object[]{Conv.toNative(arg)});
                    if (raw != Native.MISSING) return proxy;
                }
                break;
            case "setProperties":
                if (n == 1 && args[0] instanceof java.util.Collection<?> c) {
                    for (Object o : c) if (o instanceof com.destroystokyo.paper.profile.ProfileProperty p) Native.tryCall(h, "setProperty", new Object[]{p.getName(), p.getValue(), p.getSignature()});
                    return null;
                }
                break;
            case "getProperties":
                if (n == 0 && proxy instanceof com.destroystokyo.paper.profile.PlayerProfile) {
                    java.util.Set<com.destroystokyo.paper.profile.ProfileProperty> out = new java.util.LinkedHashSet<>();
                    String json = Native.profileProperties(h);
                    if (json != null) for (com.google.gson.JsonElement e : com.google.gson.JsonParser.parseString(json).getAsJsonArray()) {
                        com.google.gson.JsonArray a = e.getAsJsonArray();
                        out.add(new com.destroystokyo.paper.profile.ProfileProperty(a.get(0).getAsString(), a.get(1).getAsString(), a.get(2).isJsonNull() ? null : a.get(2).getAsString()));
                    }
                    return out;
                }
                break;
            case "getMaxStackSize":
                if (n == 0 && proxy instanceof org.bukkit.inventory.ItemStack is) {
                    Object raw = Native.tryCall(h, "getMaxStackSize", new Object[0]);
                    int v = raw instanceof Number num ? num.intValue() : 0;
                    return v > 0 ? v : is.getType().getMaxStackSize();
                }
                break;
            case "rayTrace":
                if (n == 1 && args[0] instanceof java.util.function.Consumer<?> c && proxy instanceof World w) return RayTraces.configured(w, c);
                break;
            case "getType":
                if (n == 0 && proxy instanceof org.bukkit.inventory.Inventory) {
                    org.bukkit.event.inventory.InventoryType known = Proxies.INVENTORY_TYPES.get(Proxies.invKey(h));
                    if (known != null) return known;
                }
                break;
            case "sendBlockChanges":
                if (n >= 1 && proxy instanceof Player p && args[0] instanceof java.util.Collection<?> states) {
                    for (Object o : states) if (o instanceof org.bukkit.block.BlockState s) p.sendBlockChange(s.getLocation(), s.getBlockData());
                    return null;
                }
                break;
            case "sendMultiBlockChange":
                if (n >= 1 && proxy instanceof Player p && args[0] instanceof java.util.Map<?, ?> changes) {
                    for (java.util.Map.Entry<?, ?> e : changes.entrySet()) {
                        if (e.getKey() instanceof org.bukkit.Location l && e.getValue() instanceof org.bukkit.block.data.BlockData d) p.sendBlockChange(l, d);
                        else if (e.getKey() instanceof org.bukkit.Location l && e.getValue() instanceof org.bukkit.block.BlockState s) p.sendBlockChange(l, s.getBlockData());
                    }
                    return null;
                }
                break;
            case "setPlayerProfile":
                if (n == 1 && proxy instanceof Player && args[0] instanceof com.destroystokyo.paper.profile.PlayerProfile profile) {
                    java.util.List<Object> props = new java.util.ArrayList<>();
                    for (com.destroystokyo.paper.profile.ProfileProperty pp : profile.getProperties()) props.add(new Object[]{pp.getName(), pp.getValue(), pp.getSignature()});
                    Object raw = Native.tryCall(h, "setPlayerProfile", Conv.args(new Object[]{props}));
                    if (raw != Native.MISSING) return null;
                }
                break;
            case "sendLinks":
                if (n == 1 && proxy instanceof Player && args[0] instanceof org.bukkit.ServerLinks links) {
                    java.util.List<Object> rows = new java.util.ArrayList<>();
                    for (org.bukkit.ServerLinks.ServerLink l : links.getLinks()) rows.add(new Object[]{l.displayName(), l.getUrl().toString()});
                    Object raw = Native.tryCall(h, "sendLinks", Conv.args(new Object[]{rows}));
                    if (raw != Native.MISSING) return null;
                }
                break;
            case "setReputations":
                if (n == 1 && args[0] instanceof java.util.Map<?, ?> reps && proxy instanceof org.bukkit.entity.Villager v) {
                    for (java.util.Map.Entry<?, ?> e : reps.entrySet()) v.setReputation((java.util.UUID) e.getKey(), (com.destroystokyo.paper.entity.villager.Reputation) e.getValue());
                    return null;
                }
                break;
            case "getAttributeModifiers":
                if (n == 1 && proxy instanceof org.bukkit.inventory.meta.ItemMeta meta) {
                    com.google.common.collect.Multimap<org.bukkit.attribute.Attribute, org.bukkit.attribute.AttributeModifier> all = meta.getAttributeModifiers();
                    if (args[0] instanceof org.bukkit.attribute.Attribute a) return all == null ? java.util.List.of() : new java.util.ArrayList<>(all.get(a));
                    if (args[0] instanceof org.bukkit.inventory.EquipmentSlot slot) {
                        com.google.common.collect.Multimap<org.bukkit.attribute.Attribute, org.bukkit.attribute.AttributeModifier> out = com.google.common.collect.LinkedHashMultimap.create();
                        if (all != null) for (var e : all.entries()) if (e.getValue().getSlotGroup().test(slot)) out.put(e.getKey(), e.getValue());
                        return out;
                    }
                }
                break;
            case "setAttributeModifiers":
                if (n == 1 && proxy instanceof org.bukkit.inventory.meta.ItemMeta meta) {
                    if (meta.hasAttributeModifiers()) for (org.bukkit.attribute.Attribute a : new java.util.ArrayList<>(meta.getAttributeModifiers().keySet())) meta.removeAttributeModifier(a);
                    if (args[0] instanceof com.google.common.collect.Multimap<?, ?> mm) for (java.util.Map.Entry<?, ?> e : mm.entries()) meta.addAttributeModifier((org.bukkit.attribute.Attribute) e.getKey(), (org.bukkit.attribute.AttributeModifier) e.getValue());
                    return null;
                }
                break;
            case "setTransformationMatrix":
                if (n == 1 && args[0] instanceof org.joml.Matrix4f m && proxy instanceof org.bukkit.entity.Display d) {
                    // Decomposed as the game does: translation, left rotation, scale (right rotation identity).
                    org.joml.Vector3f t = m.getTranslation(new org.joml.Vector3f());
                    org.joml.Vector3f s = m.getScale(new org.joml.Vector3f());
                    org.joml.Quaternionf r = m.getUnnormalizedRotation(new org.joml.Quaternionf()).normalize();
                    d.setTransformation(new org.bukkit.util.Transformation(t, r, s, new org.joml.Quaternionf()));
                    return null;
                }
                break;
            case "getBiome":
                // World.getBiome(x, y, z) (RegionAccessor's): the block's, as a world's biome is its blocks'.
                if (n == 3 && proxy instanceof World w && args[0] instanceof Integer x) return w.getBlockAt(x, (Integer) args[1], (Integer) args[2]).getBiome();
                break;
            case "setBiome":
                if (n == 4 && proxy instanceof World w && args[0] instanceof Integer x && args[3] instanceof org.bukkit.block.Biome b) {
                    w.getBlockAt(x, (Integer) args[1], (Integer) args[2]).setBiome(b);
                    return null;
                }
                break;
            case "canEnchantItem":
                // The game's Enchantment.canEnchant: the item is one of the enchantment's supported items.
                if (n == 1 && proxy instanceof org.bukkit.enchantments.Enchantment e) {
                    // CraftEnchantment: handle.canEnchant(asNMSCopy(item)); null is the empty stack there.
                    if (!(args[0] instanceof org.bukkit.inventory.ItemStack item) || item.isEmpty()) return false;
                    try {
                        return e.getSupportedItems().contains(io.papermc.paper.registry.TypedKey.create(io.papermc.paper.registry.RegistryKey.ITEM, item.getType().getKey()));
                    } catch (RuntimeException unknown) {
                        return false;
                    }
                }
                break;
            case "contains":
            case "containsAtLeast":
            case "all":
            case "first":
            case "clear":
            case "removeItemAnySlot": {
                // CraftInventory's queries over the contents: equals is the exact stack (amount too), isSimilar
                // ignores the amount, a Material any stack of that type.
                if (!(proxy instanceof org.bukkit.inventory.Inventory inv)) break;
                Object answer = InventoryQueries.answer(inv, name, args);
                if (answer != NONE) return answer;
                break;
            }
            case "getTileEntities":
                // Paper's (useSnapshot) and (Predicate<Block>, useSnapshot) forms: the chunk's block entities,
                // those whose block the predicate takes.
                if ((n == 1 || n == 2) && proxy instanceof org.bukkit.Chunk c && args[n - 1] instanceof Boolean) {
                    Object raw = Native.tryCall(h, "getTileEntities", new Object[0]);
                    if (raw == Native.MISSING) break;
                    Object all = Conv.fromNative(raw, org.bukkit.block.BlockState[].class);
                    List<org.bukkit.block.BlockState> out = new ArrayList<>();
                    if (all instanceof org.bukkit.block.BlockState[] states) {
                        @SuppressWarnings("unchecked") java.util.function.Predicate<org.bukkit.block.Block> keep = n == 2 ? (java.util.function.Predicate<org.bukkit.block.Block>) args[0] : null;
                        for (org.bukkit.block.BlockState s : states) if (keep == null || keep.test(s.getBlock())) out.add(s);
                    }
                    // (Predicate, useSnapshot) gives a Collection, (useSnapshot) an array, as Paper's Chunk.
                    return n == 2 ? out : out.toArray(new org.bukkit.block.BlockState[0]);
                }
                break;
            case "getChunkAtAsync":
                // A loaded chunk (or none: not generated, generating not asked for) at once, as Paper's; one to load
                // the server loads with its ticks (AsyncChunks) and the future completes then.
            case "getChunkAtAsyncUrgently":
                if (proxy instanceof World w && n >= 1) {
                    // Every overload: (x, z | Location | Block) [, generate] [, urgent] [, callback].
                    int x, z, i;
                    if (args[0] instanceof Integer ax) { x = ax; z = (Integer) args[1]; i = 2; }
                    else if (args[0] instanceof org.bukkit.Location l) { x = l.getBlockX() >> 4; z = l.getBlockZ() >> 4; i = 1; }
                    else if (args[0] instanceof org.bukkit.block.Block b) { x = b.getX() >> 4; z = b.getZ() >> 4; i = 1; }
                    else break;
                    boolean generate = !(i < n && args[i] instanceof Boolean g) || g;
                    Object callback = n > 0 ? args[n - 1] : null;
                    boolean loaded = w.isChunkLoaded(x, z);
                    if (!loaded && (generate || w.isChunkGenerated(x, z))) return AsyncChunks.request(w, x, z, callback);
                    org.bukkit.Chunk chunk = loaded ? w.getChunkAt(x, z) : null;
                    AsyncChunks.deliver(callback, chunk);
                    if (callback instanceof java.util.function.Consumer<?> || callback instanceof World.ChunkLoadCallback) return null;
                    return java.util.concurrent.CompletableFuture.completedFuture(chunk);
                }
                break;
            case "getChunkSnapshot":
                // Paper's abstract one takes four flags (the others are defaults calling it): the server's snapshot
                // has everything they ask for.
                if ((n == 4 || n == 3) && proxy instanceof org.bukkit.Chunk) {
                    Object raw = Native.tryCall(h, "getChunkSnapshot", new Object[0]);
                    if (raw != Native.MISSING) return Conv.fromNative(raw, org.bukkit.ChunkSnapshot.class);
                }
                break;
            case "getGameRuleValue":
            case "getGameRuleDefault":
                // The rule's own type (Boolean, Integer): its T is erased, and the server answers game rules as text.
                if (n == 1 && args[0] instanceof org.bukkit.GameRule<?> rule) {
                    Object raw = Native.tryCall(h, name, Conv.args(args));
                    if (raw == Native.MISSING) break;
                    if (raw == null) return null;
                    Class<?> type = rule.getType();
                    if (type == Boolean.class) return raw instanceof Boolean b ? b : Boolean.valueOf(String.valueOf(raw));
                    if (type == Integer.class) return raw instanceof Number x ? x.intValue() : Integer.valueOf(String.valueOf(raw).trim());
                    return Conv.fromNative(raw, type);
                }
                break;
            case "getResult":
                // A recipe with no one result (dyeing, map cloning...): Paper's is the empty stack, never null.
                if (n == 0 && proxy instanceof org.bukkit.inventory.Recipe) {
                    Object raw = Native.tryCall(h, "getResult", new Object[0]);
                    Object item = raw == Native.MISSING || raw == null ? null : Conv.fromNative(raw, org.bukkit.inventory.ItemStack.class);
                    return item != null ? item : org.bukkit.inventory.ItemStack.empty();
                }
                break;
            case "getPopulators":
                if (n == 0 && proxy instanceof World) return new Generators.Populators(h);
                break;
            case "vanillaBiomeProvider":
                if (n == 0 && proxy instanceof org.bukkit.generator.WorldInfo) return Generators.VANILLA;
                break;
            case "getOffers":
                if (n == 0 && proxy instanceof org.bukkit.inventory.view.EnchantmentView) {
                    Object raw = Native.tryCall(h, "getOffers", new Object[0]);
                    org.bukkit.enchantments.EnchantmentOffer[] out = new org.bukkit.enchantments.EnchantmentOffer[3];
                    Object list = Conv.fromNative(raw, java.util.List.class);
                    if (list instanceof java.util.List<?> l) for (int i = 0; i < 3 && i < l.size(); i++) {
                        if (l.get(i) instanceof java.util.List<?> t && t.size() == 3) {
                            org.bukkit.enchantments.Enchantment e = org.bukkit.Registry.ENCHANTMENT.get(org.bukkit.NamespacedKey.fromString(String.valueOf(t.get(0))));
                            if (e != null) out[i] = new org.bukkit.enchantments.EnchantmentOffer(e, ((Number) t.get(1)).intValue(), ((Number) t.get(2)).intValue());
                        }
                    }
                    return out;
                }
                break;
            case "setOffers":
                if (n == 1 && proxy instanceof org.bukkit.inventory.view.EnchantmentView && args[0] instanceof org.bukkit.enchantments.EnchantmentOffer[] offers) {
                    java.util.List<Object> rows = new java.util.ArrayList<>();
                    for (int i = 0; i < 3; i++) {
                        org.bukkit.enchantments.EnchantmentOffer o = i < offers.length ? offers[i] : null;
                        rows.add(o == null ? null : java.util.List.of(o.getEnchantment().getKey().toString(), o.getEnchantmentLevel(), o.getCost()));
                    }
                    Native.tryCall(h, "setOffers", Conv.args(new Object[]{rows}));
                    return null;
                }
                break;
            case "getBlockData":
                // The falling block's state as the game holds it: every property (its saved BlockState names only some).
                if (n == 0 && proxy instanceof org.bukkit.entity.FallingBlock) {
                    Object raw = Native.tryCall(h, "getBlockData", new Object[0]);
                    if (raw != Native.MISSING && raw != null) return withDefaults((org.bukkit.block.data.BlockData) Conv.fromNative(raw, org.bukkit.block.data.BlockData.class));
                }
                break;
            case "getBlockState":
                if (n == 0 && proxy instanceof org.bukkit.entity.FallingBlock fb) {
                    org.bukkit.block.data.BlockData data = fb.getBlockData();
                    return BlockStates.detached(data, fb.getLocation());
                }
                break;
            case "setBlockState":
                if (n == 1 && proxy instanceof org.bukkit.entity.FallingBlock fb && args[0] instanceof org.bukkit.block.BlockState s) {
                    fb.setBlockData(s.getBlockData());
                    return null;
                }
                break;
            case "getCustomTagContainer":
                if (n == 0 && proxy instanceof org.bukkit.inventory.meta.ItemMeta meta) return new org.bukkit.craftbukkit.inventory.tags.DeprecatedCustomTagContainer(meta.getPersistentDataContainer());
                break;
            case "getCustomModelDataComponent":
                if (n == 0 && proxy instanceof org.bukkit.inventory.meta.ItemMeta) return ModelData.of(h);
                break;
            case "setCustomModelDataComponent":
                if (n == 1 && proxy instanceof org.bukkit.inventory.meta.ItemMeta) { ModelData.set(h, args[0]); return null; }
                break;
            default:
                break;
        }
        return NONE;
    }

    /** Typed inventories' named slots, as CraftBukkit's inventories number them: interface, getter, setter, slot. */
    private static final Object[][] SLOTS = {
        {org.bukkit.inventory.FurnaceInventory.class, "getSmelting", "setSmelting", 0}, {org.bukkit.inventory.FurnaceInventory.class, "getFuel", "setFuel", 1},
        {org.bukkit.inventory.FurnaceInventory.class, "getResult", "setResult", 2},
        {org.bukkit.inventory.BrewerInventory.class, "getIngredient", "setIngredient", 3}, {org.bukkit.inventory.BrewerInventory.class, "getFuel", "setFuel", 4},
        {org.bukkit.inventory.AnvilInventory.class, "getFirstItem", "setFirstItem", 0}, {org.bukkit.inventory.AnvilInventory.class, "getSecondItem", "setSecondItem", 1},
        {org.bukkit.inventory.AnvilInventory.class, "getResult", "setResult", 2},
        {org.bukkit.inventory.SmithingInventory.class, "getInputTemplate", "setInputTemplate", 0}, {org.bukkit.inventory.SmithingInventory.class, "getInputEquipment", "setInputEquipment", 1},
        {org.bukkit.inventory.SmithingInventory.class, "getInputMineral", "setInputMineral", 2}, {org.bukkit.inventory.SmithingInventory.class, "getResult", "setResult", 3},
        {org.bukkit.inventory.GrindstoneInventory.class, "getUpperItem", "setUpperItem", 0}, {org.bukkit.inventory.GrindstoneInventory.class, "getLowerItem", "setLowerItem", 1},
        {org.bukkit.inventory.GrindstoneInventory.class, "getResult", "setResult", 2},
        {org.bukkit.inventory.StonecutterInventory.class, "getInputItem", "setInputItem", 0}, {org.bukkit.inventory.StonecutterInventory.class, "getResult", "setResult", 1},
        {org.bukkit.inventory.BeaconInventory.class, "getItem", "setItem", 0},
        {org.bukkit.inventory.EnchantingInventory.class, "getItem", "setItem", 0}, {org.bukkit.inventory.EnchantingInventory.class, "getSecondary", "setSecondary", 1},
        {org.bukkit.inventory.LecternInventory.class, "getBook", "setBook", 0},
        {org.bukkit.inventory.JukeboxInventory.class, "getRecord", "setRecord", 0},
        {org.bukkit.inventory.CraftingInventory.class, "getResult", "setResult", 0},
        {org.bukkit.inventory.CartographyInventory.class, "getResult", "setResult", 2},
        {org.bukkit.inventory.LoomInventory.class, "getResult", "setResult", 3},
    };

    /** A named slot of a typed inventory (getFuel -> getItem(1)). NONE when not one. */
    static Object slot(Object proxy, String name, Object[] args) {
        if (!(proxy instanceof org.bukkit.inventory.Inventory inv)) return NONE;
        int n = args == null ? 0 : args.length;
        for (Object[] s : SLOTS) {
            if (!((Class<?>) s[0]).isInstance(proxy)) continue;
            if (n == 0 && name.equals(s[1])) return inv.getItem((Integer) s[3]);
            if (n == 1 && name.equals(s[2]) && (args[0] == null || args[0] instanceof org.bukkit.inventory.ItemStack)) {
                inv.setItem((Integer) s[3], (org.bukkit.inventory.ItemStack) args[0]);
                return null;
            }
        }
        // The size only for the matrix (asked for every call, getSize itself called back here for ever:
        // EssentialsX's InventoryCloseEvent listener, a StackOverflowError).
        if (proxy instanceof org.bukkit.inventory.CraftingInventory && (name.equals("getMatrix") && n == 0 || name.equals("setMatrix") && n == 1)) {
            int size = inv.getSize();
            if (name.equals("getMatrix") && n == 0) {
                org.bukkit.inventory.ItemStack[] out = new org.bukkit.inventory.ItemStack[Math.max(0, size - 1)];
                for (int i = 1; i < size; i++) out[i - 1] = inv.getItem(i);
                return out;
            }
            if (name.equals("setMatrix") && n == 1 && args[0] instanceof org.bukkit.inventory.ItemStack[] m) {
                for (int i = 0; i < m.length && i + 1 < size; i++) inv.setItem(i + 1, m[i]);
                return null;
            }
        }
        return NONE;
    }

    /** Item meta: Paper's component-style names (itemName, customName, itemModel) as the item's components. */
    static Object meta(H h, String name, Object[] args) {
        int n = args == null ? 0 : args.length;
        // Leather (and wolf armour) colour: the dyed_color component; unset, Paper's default leather brown.
        if ((name.equals("getColor") && n == 0 || name.equals("setColor") && n == 1 || name.equals("isDyed") && n == 0)
                && String.valueOf(Items.metaKind(h.extra == null ? "" : h.extra.toUpperCase(java.util.Locale.ROOT))).contains("LeatherArmorMeta")) {
            String json = Native.componentJson(h, "minecraft:dyed_color");
            if (name.equals("isDyed")) return json != null;
            if (name.equals("getColor")) {
                if (json == null) return org.bukkit.Color.fromRGB(0xA06540);
                com.google.gson.JsonElement e = com.google.gson.JsonParser.parseString(json);
                int rgb = e.isJsonObject() ? e.getAsJsonObject().get("rgb").getAsInt() : e.getAsInt();
                return org.bukkit.Color.fromRGB(rgb & 0xFFFFFF);
            }
            if (args[0] == null) {
                Native.tryCall(h, "unsetData", Conv.args(new Object[]{"minecraft:dyed_color"}));
                Native.tryCall(h, "removeComponent", new Object[]{"minecraft:dyed_color"});
            } else {
                Native.setComponentJson(h, "minecraft:dyed_color", String.valueOf(((org.bukkit.Color) args[0]).asRGB()));
            }
            return null;
        }
        String id = switch (name) {
            case "itemName" -> "minecraft:item_name";
            case "customName" -> "minecraft:custom_name";
            case "setItemModel", "getItemModel", "hasItemModel" -> "minecraft:item_model";
            default -> null;
        };
        if (id == null) return NONE;
        if (name.equals("getItemModel") || name.equals("hasItemModel")) {
            String json = Native.componentJson(h, id);
            if (name.equals("hasItemModel")) return json != null;
            return json == null ? null : org.bukkit.NamespacedKey.fromString(com.google.gson.JsonParser.parseString(json).getAsString());
        }
        if (n == 0) {
            String json = Native.componentJson(h, id);
            return json == null ? null : net.kyori.adventure.text.serializer.gson.GsonComponentSerializer.gson().deserialize(json);
        }
        if (n == 1) {
            String json = args[0] == null ? null : Components.json(args[0]);
            if (json == null && args[0] != null) return NONE;
            if (json == null) {
                Native.tryCall(h, "unsetData", Conv.args(new Object[]{id}));
                Native.tryCall(h, "removeComponent", new Object[]{id});
            } else if (!Native.setComponentJson(h, id, json)) {
                return NONE;
            }
            return null;
        }
        return NONE;
    }

    /** The block's default state with every property (BlockType.createBlockData), or null. */
    /** Paper 26.3's default state of each block with properties (resources/horizon/block-defaults.txt, read from
     *  Paper: `minecraft:oak_stairs[facing=north,half=bottom,...]`). A table, not a server question: block data is
     *  made on world generation threads too, where waiting on the server thread would deadlock. */
    private static volatile java.util.Map<String, String> DEFAULTS;

    static java.util.Map<String, String> defaults() {
        java.util.Map<String, String> d = DEFAULTS;
        if (d != null) return d;
        java.util.Map<String, String> out = new java.util.HashMap<>();
        try (var in = Shapes.class.getResourceAsStream("/horizon/block-defaults.txt")) {
            if (in != null) for (String line : new String(in.readAllBytes(), java.nio.charset.StandardCharsets.UTF_8).split("\\R")) {
                int open = line.indexOf('[');
                if (open > 0) out.put(line.substring(0, open), line.trim());
            }
        } catch (java.io.IOException ignored) {
        }
        DEFAULTS = out;
        return out;
    }

    private static org.bukkit.block.data.BlockData parse(String text) {
        Object raw = Native.tryStatic("Bukkit", "createBlockData", new Object[]{text});
        if (raw == Native.MISSING || raw == null) return null;
        return (org.bukkit.block.data.BlockData) Conv.fromNative(raw, org.bukkit.block.data.BlockData.class);
    }

    /** The block's default state with every property, or null (a block with none: the plain name is it). */
    static org.bukkit.block.data.BlockData defaultState(org.bukkit.Material mat) {
        String full = defaults().get(mat.getKey().toString());
        return full == null ? null : parse(full);
    }

    /** A state with its unnamed properties at their defaults, as the game's states always are. */
    static org.bukkit.block.data.BlockData withDefaults(org.bukkit.block.data.BlockData data) {
        if (data == null) return null;
        String text = data.getAsString();
        int open = text.indexOf('[');
        String name = open < 0 ? text : text.substring(0, open);
        String full = defaults().get(name);
        if (full == null) return data;
        java.util.Map<String, String> props = props(full);
        java.util.Map<String, String> own = open < 0 ? java.util.Map.of() : props(text);
        if (own.keySet().containsAll(props.keySet())) return data;
        props.putAll(own);
        StringBuilder out = new StringBuilder(name).append('[');
        boolean first = true;
        for (var e : props.entrySet()) {
            if (!first) out.append(',');
            first = false;
            out.append(e.getKey()).append('=').append(e.getValue());
        }
        org.bukkit.block.data.BlockData made = parse(out.append(']').toString());
        return made == null ? data : made;
    }

    private static java.util.Map<String, String> props(String state) {
        java.util.Map<String, String> out = new java.util.LinkedHashMap<>();
        int open = state.indexOf('['), close = state.lastIndexOf(']');
        if (open < 0 || close < open) return out;
        for (String pair : state.substring(open + 1, close).split(",")) {
            int eq = pair.indexOf('=');
            if (eq > 0) out.put(pair.substring(0, eq).trim(), pair.substring(eq + 1).trim());
        }
        return out;
    }
}
