package horizon.javalayer;

import com.destroystokyo.paper.event.server.ServerExceptionEvent;
import com.destroystokyo.paper.exception.ServerException;
import org.bukkit.event.Event;
import org.bukkit.event.player.PlayerJoinEvent;
import org.spigotmc.event.player.PlayerSpawnLocationEvent;

/**
 * Events Paper fires that the server has no hook for, made by the layer from what it sees: ServerExceptionEvent
 * (a plugin's listener, task, command or enable threw), PlayerSpawnLocationEvent (Spigot's, from the join).
 */
public final class Synth {
    private Synth() {}

    /** Event classes the layer fires itself: listening to them needs nothing from the server. */
    static boolean own(Class<?> type) {
        return type == ServerExceptionEvent.class || type == PlayerSpawnLocationEvent.class;
    }

    private static final ThreadLocal<Boolean> REPORTING = new ThreadLocal<>();

    /** Paper's ServerExceptionEvent for an exception it would report (never for one its own listeners throw). */
    static void exception(ServerException e) {
        if (Boolean.TRUE.equals(REPORTING.get()) || Events.listenersOf(ServerExceptionEvent.class).isEmpty()) return;
        REPORTING.set(true);
        try {
            Events.call(new ServerExceptionEvent(e));
        } catch (Throwable ignored) {
        } finally {
            REPORTING.remove();
        }
    }

    /** The server's join, at its first slot: Spigot's PlayerSpawnLocationEvent first, and its location kept. */
    static void beforeJoin(Event event) {
        if (!(event instanceof PlayerJoinEvent join) || Events.listenersOf(PlayerSpawnLocationEvent.class).isEmpty()) return;
        org.bukkit.Location at = join.getPlayer().getLocation();
        PlayerSpawnLocationEvent spawn = new PlayerSpawnLocationEvent(join.getPlayer(), at.clone());
        Events.call(spawn);
        if (!spawn.getSpawnLocation().equals(at)) join.getPlayer().teleport(spawn.getSpawnLocation());
    }

    /** What the layer needs fired for a listener of a class it makes itself. */
    static void want(Class<?> type) {
        if (type == PlayerSpawnLocationEvent.class) Native.wantEvent("org.bukkit.event.player.PlayerJoinEvent", 0);
    }
}
