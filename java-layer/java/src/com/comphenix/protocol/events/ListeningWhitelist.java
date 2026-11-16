package com.comphenix.protocol.events;

import com.comphenix.protocol.PacketType;

import java.util.*;

public class ListeningWhitelist {
    public static final ListeningWhitelist EMPTY_WHITELIST = new ListeningWhitelist(ListenerPriority.LOW, List.of());
    private final ListenerPriority priority;
    private final Set<PacketType> types;

    public ListeningWhitelist(ListenerPriority priority, Collection<PacketType> types) {
        this.priority = priority;
        this.types = new LinkedHashSet<>(types);
    }

    public ListenerPriority getPriority() { return priority; }
    public Set<PacketType> getTypes() { return types; }
    public boolean isEnabled() { return !types.isEmpty(); }
    public int size() { return types.size(); }
    public Set<ListenerOptions> getOptions() { return EnumSet.noneOf(ListenerOptions.class); }
    public static boolean isEmpty(ListeningWhitelist whitelist) { return whitelist == null || whitelist.types.isEmpty(); }
    public static Builder newBuilder() { return new Builder(); }

    public static class Builder {
        private ListenerPriority priority = ListenerPriority.NORMAL;
        private final List<PacketType> types = new ArrayList<>();
        public Builder priority(ListenerPriority p) { priority = p; return this; }
        public Builder types(PacketType... t) { types.addAll(Arrays.asList(t)); return this; }
        public Builder types(Collection<PacketType> t) { types.addAll(t); return this; }
        public Builder normal() { return priority(ListenerPriority.NORMAL); }
        public Builder monitor() { return priority(ListenerPriority.MONITOR); }
        public Builder highest() { return priority(ListenerPriority.HIGHEST); }
        public Builder lowest() { return priority(ListenerPriority.LOWEST); }
        public Builder options(ListenerOptions... o) { return this; }
        public Builder gamePhase(Object phase) { return this; }
        public ListeningWhitelist build() { return new ListeningWhitelist(priority, types); }
    }
}
