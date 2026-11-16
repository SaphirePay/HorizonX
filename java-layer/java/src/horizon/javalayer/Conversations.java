package horizon.javalayer;

import org.bukkit.conversations.Conversation;
import org.bukkit.conversations.ConversationAbandonedEvent;
import org.bukkit.conversations.ManuallyAbandonedConversationCanceller;
import org.bukkit.permissions.PermissionAttachmentInfo;

import java.util.*;
import java.util.concurrent.ConcurrentHashMap;

/**
 * Bukkit's conversations on players and the console (Conversable): who is in one, and what they type goes to it, not
 * to chat (Paper routes chat the same way, before chat events).
 */
public final class Conversations {
    private Conversations() {}

    private static final Map<Object, LinkedList<Conversation>> ACTIVE = new ConcurrentHashMap<>();
    private static volatile boolean listening;

    private static Object who(Object conversable) {
        if (conversable instanceof org.bukkit.entity.Player p) return p.getUniqueId();
        return conversable;
    }

    static boolean isConversing(Object conversable) {
        LinkedList<Conversation> list = ACTIVE.get(who(conversable));
        return list != null && !list.isEmpty();
    }

    static boolean begin(Object conversable, Conversation conversation) {
        LinkedList<Conversation> list = ACTIVE.computeIfAbsent(who(conversable), k -> new LinkedList<>());
        synchronized (list) {
            if (!list.contains(conversation)) {
                list.addLast(conversation);
                if (list.getFirst() == conversation) {
                    conversation.begin();
                    conversation.outputNextPrompt();
                    listen();
                    return true;
                }
            }
        }
        return true;
    }

    static void abandon(Object conversable, Conversation conversation, ConversationAbandonedEvent details) {
        LinkedList<Conversation> list = ACTIVE.get(who(conversable));
        if (list == null) return;
        Conversation next = null;
        synchronized (list) {
            if (!list.isEmpty() && list.getFirst() == conversation) {
                list.removeFirst();
                conversation.abandon(details);
                if (!list.isEmpty()) next = list.getFirst();
            } else {
                list.remove(conversation);
            }
        }
        if (next != null) {
            next.begin();
            next.outputNextPrompt();
        }
    }

    static void abandonAll(Object conversable) {
        LinkedList<Conversation> list = ACTIVE.remove(who(conversable));
        if (list == null) return;
        for (Conversation c : list) c.abandon(new ConversationAbandonedEvent(c, new ManuallyAbandonedConversationCanceller()));
    }

    static void accept(Object conversable, String input) {
        LinkedList<Conversation> list = ACTIVE.get(who(conversable));
        if (list == null || list.isEmpty()) return;
        list.getFirst().acceptInput(input);
    }

    /** Chat events are wanted (at the first slot) once a conversation begins. */
    private static void listen() {
        if (listening) return;
        listening = true;
        Native.wantEvent("org.bukkit.event.player.AsyncPlayerChatEvent", 0);
        Native.wantEvent("io.papermc.paper.event.player.AsyncChatEvent", 0);
    }

    static boolean wants(Class<?> eventType) {
        return listening && !ACTIVE.isEmpty() && (eventType == org.bukkit.event.player.AsyncPlayerChatEvent.class || eventType == io.papermc.paper.event.player.AsyncChatEvent.class);
    }

    /** A chat line of a player in a conversation: handed to it, and the chat event cancelled. */
    static boolean consume(org.bukkit.event.Event event) {
        org.bukkit.entity.Player player;
        String text;
        if (event instanceof org.bukkit.event.player.AsyncPlayerChatEvent chat) {
            player = chat.getPlayer();
            text = chat.getMessage();
        } else if (event instanceof io.papermc.paper.event.player.AsyncChatEvent chat) {
            player = chat.getPlayer();
            text = net.kyori.adventure.text.serializer.plain.PlainTextComponentSerializer.plainText().serialize(chat.originalMessage());
        } else {
            return false;
        }
        if (player == null || !isConversing(player)) return false;
        ((org.bukkit.event.Cancellable) event).setCancelled(true);
        Scheduler.INSTANCE.runTask(Loader.all().isEmpty() ? null : Loader.all().get(0), () -> accept(player, text));
        return true;
    }

    /** Permissible.getEffectivePermissions: every permission Bukkit knows, as this one has it. */
    static Set<PermissionAttachmentInfo> effectivePermissions(org.bukkit.permissions.Permissible p) {
        // PermissibleBase.recalculatePermissions: the default permissions for this one's op status and their
        // children (a false child inverts its own children), then what the server says is set for it.
        java.util.Map<String, PermissionAttachmentInfo> out = new java.util.LinkedHashMap<>();
        org.bukkit.plugin.PluginManager pm = org.bukkit.Bukkit.getPluginManager();
        boolean op = false;
        try {
            op = p.isOp();
        } catch (RuntimeException ignored) {
            // not an operator, then
        }
        for (org.bukkit.permissions.Permission perm : pm.getDefaultPermissions(op)) {
            out.put(perm.getName().toLowerCase(java.util.Locale.ROOT), new PermissionAttachmentInfo(p, perm.getName().toLowerCase(java.util.Locale.ROOT), null, true));
            children(p, pm, out, perm.getChildren(), false);
        }
        for (org.bukkit.permissions.Permission perm : pm.getPermissions()) {
            String name = perm.getName().toLowerCase(java.util.Locale.ROOT);
            if (!out.containsKey(name) && p.isPermissionSet(perm)) out.put(name, new PermissionAttachmentInfo(p, name, null, p.hasPermission(perm)));
        }
        return new HashSet<>(out.values());
    }

    private static void children(org.bukkit.permissions.Permissible p, org.bukkit.plugin.PluginManager pm, java.util.Map<String, PermissionAttachmentInfo> out,
                                 java.util.Map<String, Boolean> children, boolean invert) {
        for (java.util.Map.Entry<String, Boolean> e : children.entrySet()) {
            String name = e.getKey().toLowerCase(java.util.Locale.ROOT);
            boolean value = e.getValue() ^ invert;
            out.put(name, new PermissionAttachmentInfo(p, name, null, value));
            org.bukkit.permissions.Permission perm = pm.getPermission(name);
            if (perm != null) children(p, pm, out, perm.getChildren(), !value);
        }
    }
}
