package horizon.javalayer;

import net.kyori.adventure.text.Component;
import net.kyori.adventure.text.serializer.gson.GsonComponentSerializer;
import net.md_5.bungee.api.ChatMessageType;
import net.md_5.bungee.api.chat.BaseComponent;
import net.md_5.bungee.chat.ComponentSerializer;
import org.bukkit.command.CommandSender;
import org.bukkit.entity.Player;

import java.util.UUID;

/** `player.spigot().sendMessage(BaseComponent...)`: BungeeCord chat, which older plugins still send. */
final class Spigot {
    private Spigot() {}

    static Component of(BaseComponent... parts) {
        return GsonComponentSerializer.gson().deserialize(ComponentSerializer.toString(parts));
    }

    static Player.Spigot of(Player player) {
        return new Player.Spigot() {
            @Override public void sendMessage(BaseComponent component) { player.sendMessage(of(component)); }
            @Override public void sendMessage(BaseComponent... components) { player.sendMessage(of(components)); }
            @Override public void sendMessage(ChatMessageType type, BaseComponent component) { sendMessage(type, new BaseComponent[]{component}); }
            @Override public void sendMessage(ChatMessageType type, BaseComponent... components) {
                if (type == ChatMessageType.ACTION_BAR) player.sendActionBar(of(components));
                else player.sendMessage(of(components));
            }
            @Override public void sendMessage(ChatMessageType type, UUID sender, BaseComponent component) { sendMessage(type, component); }
            @Override public void sendMessage(ChatMessageType type, UUID sender, BaseComponent... components) { sendMessage(type, components); }
            @Override public void respawn() { Native.call(Proxies.handleOf(player), "respawn", new Object[0]); }
            @Override public int getPing() { return player.getPing(); }
        };
    }

    static CommandSender.Spigot sender(CommandSender sender) {
        return new CommandSender.Spigot() {
            @Override public void sendMessage(BaseComponent component) { sender.sendMessage(of(component)); }
            @Override public void sendMessage(BaseComponent... components) { sender.sendMessage(of(components)); }
            @Override public void sendMessage(UUID who, BaseComponent component) { sender.sendMessage(of(component)); }
            @Override public void sendMessage(UUID who, BaseComponent... components) { sender.sendMessage(of(components)); }
        };
    }
}
