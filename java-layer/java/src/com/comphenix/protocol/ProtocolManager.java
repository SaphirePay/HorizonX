package com.comphenix.protocol;

import com.comphenix.protocol.events.PacketContainer;
import com.comphenix.protocol.events.PacketListener;
import org.bukkit.entity.Player;

import java.util.Collection;
import java.util.List;

/** ProtocolLib's manager: answered by the server's ProtocolLib bridge. */
public interface ProtocolManager {
    void addPacketListener(PacketListener listener);
    void removePacketListener(PacketListener listener);
    void removePacketListeners(org.bukkit.plugin.Plugin plugin);
    Collection<PacketListener> getPacketListeners();
    PacketContainer createPacket(PacketType type);
    default PacketContainer createPacket(PacketType type, boolean forceDefaults) { return createPacket(type); }
    void sendServerPacket(Player receiver, PacketContainer packet);
    default void sendServerPacket(Player receiver, PacketContainer packet, boolean filters) { sendServerPacket(receiver, packet); }
    default void sendServerPacket(Player receiver, PacketContainer packet, Object marker, boolean filters) { sendServerPacket(receiver, packet); }
    void broadcastServerPacket(PacketContainer packet);
    default void broadcastServerPacket(PacketContainer packet, org.bukkit.entity.Entity tracker) { broadcastServerPacket(packet); }
    default void broadcastServerPacket(PacketContainer packet, org.bukkit.entity.Entity tracker, boolean includeTracker) { broadcastServerPacket(packet); }
    default void broadcastServerPacket(PacketContainer packet, org.bukkit.Location origin, int maxDistance) { broadcastServerPacket(packet); }
    default void broadcastServerPacket(PacketContainer packet, Collection<? extends Player> targets) { for (Player p : targets) sendServerPacket(p, packet); }
    default void receiveClientPacket(Player sender, PacketContainer packet) { throw new UnsupportedOperationException("Horizon's ProtocolLib can't inject client packets yet"); }
    default void receiveClientPacket(Player sender, PacketContainer packet, boolean filters) { receiveClientPacket(sender, packet); }
    boolean isClosed();
    String getMinecraftVersion();
    default int getProtocolVersion(Player player) { return 776; }
    default List<Player> getEntityTrackers(org.bukkit.entity.Entity entity) { return new java.util.ArrayList<>(org.bukkit.Bukkit.getOnlinePlayers()); }
    default void updateEntity(org.bukkit.entity.Entity entity, List<Player> observers) {}
}
