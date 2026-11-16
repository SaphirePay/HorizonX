package com.comphenix.protocol.events;

import com.comphenix.protocol.PacketType;
import org.bukkit.plugin.Plugin;

import java.util.*;

/** ProtocolLib's PacketAdapter: its packet types are registered with the server's bridge, which calls back here. */
public abstract class PacketAdapter implements PacketListener {
    protected Plugin plugin;
    protected ListenerPriority priority;
    protected ListeningWhitelist sendingWhitelist;
    protected ListeningWhitelist receivingWhitelist;
    private Object horizonAdapter;

    public PacketAdapter(Plugin plugin, PacketType... types) { this(plugin, ListenerPriority.NORMAL, Arrays.asList(types)); }
    public PacketAdapter(Plugin plugin, ListenerPriority priority, PacketType... types) { this(plugin, priority, Arrays.asList(types)); }
    public PacketAdapter(Plugin plugin, Iterable<? extends PacketType> types) { this(plugin, ListenerPriority.NORMAL, types); }
    public PacketAdapter(Plugin plugin, ListenerPriority priority, Iterable<? extends PacketType> types) {
        this.plugin = plugin;
        this.priority = priority == null ? ListenerPriority.NORMAL : priority;
        List<PacketType> server = new ArrayList<>(), client = new ArrayList<>();
        for (PacketType t : types) (t.isServer() ? server : client).add(t);
        this.sendingWhitelist = new ListeningWhitelist(this.priority, server);
        this.receivingWhitelist = new ListeningWhitelist(this.priority, client);
    }
    public PacketAdapter(Plugin plugin, ListenerPriority priority, Iterable<? extends PacketType> types, ListenerOptions... options) { this(plugin, priority, types); }
    public PacketAdapter(AdapterParameteters params) { this(params.plugin, params.priority, params.types); }

    /** The server's adapter for this listener's packet types (the bridge asks for it by this name). */
    public Object __adapter() {
        if (horizonAdapter == null) {
            List<Object> args = new ArrayList<>();
            args.add(plugin);
            args.add(priority.name());
            for (PacketType t : sendingWhitelist.getTypes()) args.add(t.horizonHandle());
            horizonAdapter = horizon.javalayer.Native.construct("PacketAdapter", horizon.javalayer.Conv.args(args.toArray()));
        }
        return horizonAdapter;
    }

    @Override public void onPacketSending(PacketEvent event) {}
    @Override public void onPacketReceiving(PacketEvent event) {}
    @Override public ListeningWhitelist getSendingWhitelist() { return sendingWhitelist; }
    @Override public ListeningWhitelist getReceivingWhitelist() { return receivingWhitelist; }
    @Override public Plugin getPlugin() { return plugin; }

    public static AdapterParameteters params() { return new AdapterParameteters(); }
    public static AdapterParameteters params(Plugin plugin, PacketType... types) { return new AdapterParameteters().plugin(plugin).types(types); }

    public static class AdapterParameteters {
        Plugin plugin;
        ListenerPriority priority = ListenerPriority.NORMAL;
        final List<PacketType> types = new ArrayList<>();
        public AdapterParameteters plugin(Plugin p) { plugin = p; return this; }
        public AdapterParameteters listenerPriority(ListenerPriority p) { priority = p; return this; }
        public AdapterParameteters types(PacketType... t) { types.addAll(Arrays.asList(t)); return this; }
        public AdapterParameteters types(Set<PacketType> t) { types.addAll(t); return this; }
        public AdapterParameteters serverSide() { return this; }
        public AdapterParameteters clientSide() { return this; }
        public AdapterParameteters optionAsync() { return this; }
        public AdapterParameteters options(ListenerOptions... o) { return this; }
        public AdapterParameteters connectionSide(Object side) { return this; }
        public AdapterParameteters gamePhase(Object phase) { return this; }
    }
}
