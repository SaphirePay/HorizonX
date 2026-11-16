package horizon.javalayer;

import com.mojang.brigadier.LiteralMessage;
import com.mojang.brigadier.Message;
import io.papermc.paper.command.brigadier.MessageComponentSerializer;
import net.kyori.adventure.text.Component;
import net.kyori.adventure.text.serializer.plain.PlainTextComponentSerializer;

/** Paper's MessageComponentSerializer: Brigadier messages (command errors, tooltips) to and from components. */
public final class MessageSerializer implements MessageComponentSerializer {
    public MessageSerializer() {}

    @Override
    public Component deserialize(Message message) {
        return message instanceof ComponentMessage c ? c.component : Component.text(message.getString());
    }

    @Override
    public Message serialize(Component component) {
        return new ComponentMessage(component);
    }

    /** A Brigadier message that keeps its component (colours survive to the player). */
    static final class ComponentMessage extends LiteralMessage {
        final Component component;

        ComponentMessage(Component component) {
            super(PlainTextComponentSerializer.plainText().serialize(component));
            this.component = component;
        }
    }
}
