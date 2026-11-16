package org.bukkit.event;

import java.util.ArrayList;
import java.util.Collection;
import java.util.EnumMap;
import java.util.EnumSet;
import java.util.List;
import java.util.ListIterator;
import java.util.Map;
import java.util.Set;
import java.util.concurrent.ConcurrentHashMap;
import org.bukkit.plugin.Plugin;
import org.bukkit.plugin.RegisteredListener;

/**
 * Paper 26.3's HandlerList, as its API has it, standing in for that one: the layer keeps the listeners plugins register
 * through the plugin manager itself (horizon.javalayer.Events), so each unregister here also takes them out there
 * (SMPCore unregisters its one-shot join listener; menu libraries unregister a menu's listener when it closes).
 */
public class HandlerList {
    private volatile RegisteredListener[] handlers = null;
    private final EnumMap<EventPriority, ArrayList<RegisteredListener>> handlerslots;
    private static final ArrayList<HandlerList> allLists = new ArrayList<>();
    private static final Set<String> EVENT_TYPES = ConcurrentHashMap.newKeySet();

    public static void bakeAll() {
        synchronized (allLists) {
            for (HandlerList h : allLists) h.bake();
        }
    }

    public static void unregisterAll() {
        synchronized (allLists) {
            for (HandlerList h : allLists) {
                synchronized (h) {
                    for (List<RegisteredListener> list : h.handlerslots.values()) list.clear();
                    h.handlers = null;
                }
            }
        }
        horizon.javalayer.Events.unregisteredEverything();
    }

    public static void unregisterAll(Plugin plugin) {
        synchronized (allLists) {
            for (HandlerList h : allLists) h.removePlugin(plugin);
        }
        horizon.javalayer.Events.unregisteredEverywhere(plugin);
    }

    public static void unregisterAll(Listener listener) {
        synchronized (allLists) {
            for (HandlerList h : allLists) h.removeListener(listener);
        }
        horizon.javalayer.Events.unregisteredEverywhere(listener);
    }

    public HandlerList() {
        StackWalker.getInstance(EnumSet.of(StackWalker.Option.RETAIN_CLASS_REFERENCE), 4)
                .walk(s -> s.filter(f -> Event.class.isAssignableFrom(f.getDeclaringClass())).findFirst())
                .map(f -> f.getDeclaringClass().getName())
                .ifPresent(EVENT_TYPES::add);
        handlerslots = new EnumMap<>(EventPriority.class);
        for (EventPriority o : EventPriority.values()) handlerslots.put(o, new ArrayList<>());
        synchronized (allLists) {
            allLists.add(this);
        }
    }

    public synchronized void register(RegisteredListener listener) {
        if (handlerslots.get(listener.getPriority()).contains(listener)) {
            throw new IllegalStateException("This listener is already registered to priority " + listener.getPriority().toString());
        }
        handlers = null;
        handlerslots.get(listener.getPriority()).add(listener);
    }

    public void registerAll(Collection<RegisteredListener> listeners) {
        for (RegisteredListener listener : listeners) register(listener);
    }

    public synchronized void unregister(RegisteredListener listener) {
        if (handlerslots.get(listener.getPriority()).remove(listener)) handlers = null;
    }

    public void unregister(Plugin plugin) {
        removePlugin(plugin);
        horizon.javalayer.Events.unregistered(this, plugin);
    }

    public void unregister(Listener listener) {
        removeListener(listener);
        horizon.javalayer.Events.unregistered(this, listener);
    }

    private synchronized void removePlugin(Plugin plugin) {
        boolean changed = false;
        for (List<RegisteredListener> list : handlerslots.values()) {
            for (ListIterator<RegisteredListener> i = list.listIterator(); i.hasNext(); ) {
                if (i.next().getPlugin().equals(plugin)) {
                    i.remove();
                    changed = true;
                }
            }
        }
        if (changed) handlers = null;
    }

    private synchronized void removeListener(Listener listener) {
        boolean changed = false;
        for (List<RegisteredListener> list : handlerslots.values()) {
            for (ListIterator<RegisteredListener> i = list.listIterator(); i.hasNext(); ) {
                if (i.next().getListener().equals(listener)) {
                    i.remove();
                    changed = true;
                }
            }
        }
        if (changed) handlers = null;
    }

    public synchronized void bake() {
        if (handlers != null) return;
        List<RegisteredListener> entries = new ArrayList<>();
        for (Map.Entry<EventPriority, ArrayList<RegisteredListener>> entry : handlerslots.entrySet()) entries.addAll(entry.getValue());
        handlers = entries.toArray(new RegisteredListener[0]);
    }

    public RegisteredListener [] getRegisteredListeners() {
        RegisteredListener[] handlers;
        while ((handlers = this.handlers) == null) bake();
        return handlers;
    }

    public boolean hasListeners() {
        return getRegisteredListeners().length != 0;
    }

    public static ArrayList<RegisteredListener> getRegisteredListeners(Plugin plugin) {
        ArrayList<RegisteredListener> listeners = new ArrayList<>();
        synchronized (allLists) {
            for (HandlerList h : allLists) {
                synchronized (h) {
                    for (List<RegisteredListener> list : h.handlerslots.values()) {
                        for (RegisteredListener listener : list) if (listener.getPlugin().equals(plugin)) listeners.add(listener);
                    }
                }
            }
        }
        return listeners;
    }

    @SuppressWarnings("unchecked")
    public static ArrayList<HandlerList> getHandlerLists() {
        synchronized (allLists) {
            return (ArrayList<HandlerList>) allLists.clone();
        }
    }
}
