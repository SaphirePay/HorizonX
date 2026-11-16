package horizon.javalayer;

import net.kyori.adventure.audience.Audience;
import net.kyori.adventure.text.event.ClickCallback;
import net.kyori.adventure.text.event.ClickEvent;

import java.time.Instant;
import java.util.Map;
import java.util.UUID;
import java.util.concurrent.ConcurrentHashMap;

/**
 * Adventure's `ClickEvent.callback(...)`, as Paper does it: the click runs `/paper:callback <id>`, which runs the
 * plugin's code for the player who clicked (within its lifetime and number of uses).
 */
public final class Callbacks {
    private Callbacks() {}

    private record Entry(ClickCallback<Audience> callback, Instant expires, int[] uses) {}

    private static final Map<UUID, Entry> CALLBACKS = new ConcurrentHashMap<>();
    private static volatile boolean registered;

    @SuppressWarnings("unchecked")
    static ClickEvent make(Object callback, Object options) {
        register();
        ClickCallback.Options o = (ClickCallback.Options) options;
        UUID id = UUID.randomUUID();
        int uses = o.uses() == ClickCallback.UNLIMITED_USES ? Integer.MAX_VALUE : o.uses();
        CALLBACKS.put(id, new Entry((ClickCallback<Audience>) callback, Instant.now().plus(o.lifetime()), new int[]{uses}));
        return ClickEvent.runCommand("/paper:callback " + id);
    }

    private static final Map<String, Object[]> DIALOG = new ConcurrentHashMap<>();

    /** DialogAction.customClick(callback, options): a custom click whose id the layer answers (PlayerCustomClickEvent). */
    static Object dialogAction(Object callback, Object options) {
        String id = "paper:callback/" + UUID.randomUUID();
        DIALOG.put(id, new Object[]{callback, options});
        Native.wantEvent("io.papermc.paper.event.player.PlayerCustomClickEvent", 0);
        Object raw = Native.tryStatic("DialogAction", "customClick", new Object[]{id, null});
        if (raw == Native.MISSING) throw Report.cant("DialogAction.customClick");
        return Conv.fromNative(raw, io.papermc.paper.registry.data.dialog.action.DialogAction.CustomClickAction.class);
    }

    /** A custom click the layer made: runs its callback. */
    @SuppressWarnings("unchecked")
    static boolean dialogClicked(org.bukkit.event.Event event) {
        if (!(event instanceof io.papermc.paper.event.player.PlayerCustomClickEvent click)) return false;
        Object[] entry = DIALOG.get(click.getIdentifier().asString());
        if (entry == null) return false;
        ((io.papermc.paper.registry.data.dialog.action.DialogActionCallback) entry[0]).accept(click.getDialogResponseView(), click.getCommonConnection() instanceof Audience a ? a : null);
        return true;
    }

    private static void register() {
        if (registered) return;
        synchronized (Callbacks.class) {
            if (registered) return;
            org.bukkit.command.Command command = new org.bukkit.command.Command("callback") {
                @Override
                public boolean execute(org.bukkit.command.CommandSender sender, String label, String[] args) {
                    if (args.length != 1) return true;
                    UUID id;
                    try {
                        id = UUID.fromString(args[0]);
                    } catch (IllegalArgumentException e) {
                        return true;
                    }
                    Entry e = CALLBACKS.get(id);
                    if (e == null || Instant.now().isAfter(e.expires())) {
                        CALLBACKS.remove(id);
                        return true;
                    }
                    synchronized (e) {
                        if (e.uses()[0] <= 0) return true;
                        if (--e.uses()[0] <= 0) CALLBACKS.remove(id);
                    }
                    e.callback().accept(sender);
                    return true;
                }
            };
            Commands.MAP.register("callback", "paper", command);
            registered = true;
        }
    }
}
