package com.comphenix.protocol.events;

import org.bukkit.plugin.Plugin;

public interface PacketListener {
    void onPacketSending(PacketEvent event);
    void onPacketReceiving(PacketEvent event);
    ListeningWhitelist getSendingWhitelist();
    ListeningWhitelist getReceivingWhitelist();
    Plugin getPlugin();
}
