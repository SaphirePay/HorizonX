package com.comphenix.protocol.events;

import com.comphenix.protocol.PacketType;
import horizon.javalayer.Conv;
import horizon.javalayer.H;
import horizon.javalayer.Native;
import horizon.javalayer.Proxies;
import org.bukkit.entity.Player;

/** A packet going to a player, as the server's bridge hands it. */
public class PacketEvent implements Proxies.Handled, org.bukkit.event.Cancellable {
    private final H h;
    private PacketContainer packet;

    protected PacketEvent(H h) { this.h = h; }

    @Override public H horizonHandle() { return h; }

    public Player getPlayer() { return (Player) Conv.fromNative(Native.call(h, "getPlayer", new Object[0]), Player.class); }

    public PacketContainer getPacket() {
        if (packet == null) packet = (PacketContainer) Conv.fromNative(Native.call(h, "getPacket", new Object[0]), PacketContainer.class);
        return packet;
    }

    public void setPacket(PacketContainer packet) { this.packet = packet; }
    public PacketType getPacketType() { return getPacket().getType(); }
    @Override public boolean isCancelled() { return Boolean.TRUE.equals(Native.call(h, "isCancelled", new Object[0])); }
    @Override public void setCancelled(boolean cancel) { Native.call(h, "setCancelled", new Object[]{cancel}); }
    public boolean isServerPacket() { return true; }
    public boolean isAsync() { return false; }
    public boolean isPlayerTemporary() { return false; }
    public boolean isReadOnly() { return false; }
    public void setReadOnly(boolean readOnly) {}
    public boolean isFiltered() { return true; }
}
