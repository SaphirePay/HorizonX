package horizon.javalayer;

import org.bukkit.craftbukkit.entity.CraftHumanEntity;
import org.bukkit.permissions.PermissibleBase;

import java.lang.reflect.Method;
import java.util.Set;

/** CraftPlayer's methods: a Permissible a plugin put in place (LuckPerms) answers permissions, the server the rest. */
public final class Players {
    /** CraftPlayer.sendMessage(Component): the component as one chat message (the SDK's tellraw), as Paper sends it. */
    /** Channels a plugin added for a player (CraftPlayer.addChannel), by player. */
    static final java.util.Map<java.util.UUID, java.util.Set<String>> ADDED_CHANNELS = new java.util.concurrent.ConcurrentHashMap<>();

    /** CraftPlayer.addChannel: the channel joins the player's listening ones, PlayerRegisterChannelEvent if it's new. */
    public static void addChannel(org.bukkit.entity.Player p, String channel) {
        if (channel == null) return;
        if (p.getListeningPluginChannels().contains(channel)) return;
        if (ADDED_CHANNELS.computeIfAbsent(p.getUniqueId(), k -> java.util.concurrent.ConcurrentHashMap.newKeySet()).add(channel)) {
            org.bukkit.Bukkit.getPluginManager().callEvent(new org.bukkit.event.player.PlayerRegisterChannelEvent(p, channel));
        }
    }

    /** CraftPlayer.removeChannel: PlayerUnregisterChannelEvent if it was one. */
    public static void removeChannel(org.bukkit.entity.Player p, String channel) {
        java.util.Set<String> mine = ADDED_CHANNELS.get(p.getUniqueId());
        if (mine != null && mine.remove(channel)) {
            org.bukkit.Bukkit.getPluginManager().callEvent(new org.bukkit.event.player.PlayerUnregisterChannelEvent(p, channel));
        }
    }

    public static void sendComponent(H h, net.kyori.adventure.text.Component message) {
        if (message == null) return;
        Native.tryCall(h, "sendMessage", Conv.args(new Object[]{message}));
    }

    private Players() {}

    private static final Set<String> PERMISSIBLE = Set.of("hasPermission", "isPermissionSet", "addAttachment", "removeAttachment", "recalculatePermissions", "getEffectivePermissions");

    public static Object invoke(CraftHumanEntity self, H h, Method m, Object[] args) {
        // A plugin's own vanilla player (an NPC): Paper's code on its NMS player.
        if (h == null) return Npcs.invoke(self, m, args);
        try {
            PermissibleBase perm = self.horizonPermissible();
            // The player's Permissible answers (the server's own, ServerPermissible, or one a plugin put in place).
            if (perm != null && PERMISSIBLE.contains(m.getName())) {
                // Bukkit's own Permissible or the layer's (ServerPermissible): a plugin's own (LuckPerms') keeps itself.
                if (perm.getClass() == PermissibleBase.class || perm instanceof ServerPermissible) current(self, h, perm);
                // Bukkit's own PermissibleBase (the player's from the start): the server answers a permission
                // nothing attached here. A plugin's own Permissible (LuckPerms') answers everything itself.
                if (perm.getClass() == PermissibleBase.class && (m.getName().equals("hasPermission") || m.getName().equals("isPermissionSet"))) {
                    String node = args[0] instanceof org.bukkit.permissions.Permission p ? p.getName() : String.valueOf(args[0]);
                    // Bukkit's answer (attachments, else the permission's default for an op or not), or the server's
                    // own grant (a Rust permission plugin): either one says yes.
                    if (!perm.isPermissionSet(node)) {
                        boolean bukkit = m.getName().equals("hasPermission") ? perm.hasPermission(node) : perm.isPermissionSet(node);
                        if (bukkit) return true;
                        Object raw = Native.tryCall(h, m.getName(), new Object[]{node});
                        return raw instanceof Boolean b && b;
                    }
                }
                Method own = perm.getClass().getMethod(m.getName(), m.getParameterTypes());
                return own.invoke(perm, args);
            }
            if (m.getName().equals("getWorld") && (args == null || args.length == 0)) {
                // A player's world, asked for each player on each block change by some plugins (AntiXray's reveal):
                // on the game's thread the same until the game moves on (the next tick or event) or a plugin moves
                // the player. Another thread (AntiXray's handler, on each chunk packet) takes it up to a second old,
                // as its query waiting for the game would (jbridge keeps a player's state that long).
                long now = Bridge.epoch, at = System.nanoTime();
                boolean main = Thread.currentThread() == Bridge.mainThread();
                Object[] kept = WORLDS.get(h.key);
                if (kept != null && (main ? (long) kept[0] == now : at - (long) kept[2] < 1_000_000_000L)) return kept[1];
                Object world = NativeHandler.call(self, h, m, args);
                if (world != null) WORLDS.put(h.key, new Object[]{main ? now : -1L, world, at});
                return world;
            }
            if (m.getName().equals("getAddress") && (args == null || args.length == 0)) {
                // The player's address, fixed for their connection: ProtocolLib asks it for every packet its async
                // listeners hand back (its sending queues are by address), from worker threads that would each wait
                // for the game. Kept 30 s (a player who leaves and joins again may come from another port).
                long at = System.nanoTime();
                Object[] kept = ADDRESSES.get(h.key);
                if (kept != null && at - (long) kept[0] < 30_000_000_000L) return kept[1];
                Object address = NativeHandler.call(self, h, m, args);
                if (address != null) ADDRESSES.put(h.key, new Object[]{at, address});
                return address;
            }
            if (m.getName().startsWith("teleport")) WORLDS.remove(h.key);
            return NativeHandler.call(self, h, m, args);
        } catch (java.lang.reflect.InvocationTargetException e) {
            throw sneaky(e.getCause());
        } catch (RuntimeException | Error e) {
            throw e;
        } catch (Throwable t) {
            throw sneaky(t);
        }
    }

    private static final java.util.Map<String, Object[]> WORLDS = new java.util.concurrent.ConcurrentHashMap<>();

    /** By player: [when read, InetSocketAddress] (getAddress). */
    private static final java.util.Map<String, Object[]> ADDRESSES = new java.util.concurrent.ConcurrentHashMap<>();

    /** By player: [tick checked, op then, permissions version then] (Players.current). */
    private static final java.util.Map<String, Object[]> PERMS_AS_OF = new java.util.concurrent.ConcurrentHashMap<>();

    /**
     * The player's default permissions as their op status and the registered permissions are now: Paper's setOp and
     * addPermission work them out again (PermissibleBase.recalculatePermissions, the children of `default: op`
     * permissions: PvPManager's `pvpmanager.*` gives its commands' `default: false` ones to ops). Here an /op from the
     * console or another plugin never reaches Java, so each player's is checked once a tick, on the server's thread.
     */
    private static void current(CraftHumanEntity self, H h, PermissibleBase perm) {
        if (Thread.currentThread() != Bridge.mainThread()) return;
        long tick = Bridge.tickCount;
        Object[] as = PERMS_AS_OF.get(h.key);
        if (as != null && (long) as[0] == tick) return;
        // A hook that holds the server's hub may see no players: their op status then reads false, so nothing is
        // worked out again until the server answers for them.
        if (!(self instanceof org.bukkit.entity.Player player) || !player.isOnline()) return;
        boolean op = self.isOp();
        int version = PluginManagerImpl.permissionsVersion;
        if (as != null && (boolean) as[1] == op && (int) as[2] == version) {
            as[0] = tick;
            return;
        }
        // The first check for this player: their Permissible was worked out when it was made, maybe before plugins'
        // permissions were in or before an /op; once more is cheap.
        perm.recalculatePermissions();
        PERMS_AS_OF.put(h.key, new Object[]{tick, op, version});
    }

    @SuppressWarnings("unchecked")
    static <T extends Throwable> RuntimeException sneaky(Throwable t) throws T {
        throw (T) t;
    }
}
