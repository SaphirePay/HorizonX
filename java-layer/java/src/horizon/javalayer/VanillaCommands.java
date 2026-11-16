package horizon.javalayer;

import com.mojang.brigadier.CommandDispatcher;
import com.mojang.brigadier.ParseResults;
import com.mojang.brigadier.builder.LiteralArgumentBuilder;
import com.mojang.brigadier.exceptions.CommandSyntaxException;
import com.mojang.brigadier.suggestion.Suggestion;
import com.mojang.brigadier.tree.LiteralCommandNode;
import net.minecraft.commands.CommandSource;
import net.minecraft.commands.CommandSourceStack;
import net.minecraft.server.MinecraftServer;
import org.bukkit.command.Command;
import org.bukkit.command.CommandSender;

import java.util.ArrayList;
import java.util.List;

/**
 * The game's own command dispatcher as plugins reach it (MinecraftServer.getCommands().getDispatcher(): BetterHud
 * registers its commands there in onLoad, with vanilla's CommandSourceStack). What is registered on it becomes a
 * server command: run, it is parsed and executed by this dispatcher with a vanilla source for the sender (its
 * getBukkitSender() is the sender; its messages go to the sender), as Paper's Commands.performCommand does.
 *
 * The dispatcher holds the plugins' commands only (the game's own run in the Rust server).
 */
public final class VanillaCommands {
    private VanillaCommands() {}

    /** The hollow server's Commands, MinecraftServer.resources.managers.commands, with this dispatcher. */
    public static void install(MinecraftServer server, net.minecraft.core.HolderLookup.Provider registries) {
        try {
            var commands = Craft.allocate(net.minecraft.commands.Commands.class);
            Craft.set(commands, net.minecraft.commands.Commands.class, "dispatcher", new Dispatcher());
            var managers = Craft.allocate(net.minecraft.server.ReloadableServerResources.class);
            Craft.set(managers, net.minecraft.server.ReloadableServerResources.class, "commands", commands);
            // Its advancements: none (the Rust server keeps the players'); an NMS player a plugin makes needs the
            // manager (GSit's pose bodies: PlayerList.getPlayerAdvancements).
            try {
                var advancements = Craft.allocate(net.minecraft.server.ServerAdvancementManager.class);
                Craft.set(advancements, net.minecraft.server.ServerAdvancementManager.class, "advancements", java.util.Map.of());
                Craft.set(advancements, net.minecraft.server.ServerAdvancementManager.class, "tree", new net.minecraft.advancements.AdvancementTree());
                Craft.set(managers, net.minecraft.server.ReloadableServerResources.class, "advancements", advancements);
            } catch (RuntimeException | LinkageError e) {
                Native.log(1, "[JavaLayer] the hollow server's advancements could not be made: " + e);
            }
            // A record: made with its own constructor (Unsafe can't set a record's fields). No resource manager.
            Class<?> record = Class.forName("net.minecraft.server.MinecraftServer$ReloadableResources");
            var make = record.getDeclaredConstructor(net.minecraft.server.packs.resources.CloseableResourceManager.class, net.minecraft.server.ReloadableServerResources.class);
            make.setAccessible(true);
            Object resources = make.newInstance(null, managers);
            Craft.set(server, MinecraftServer.class, "resources", resources);
        } catch (ReflectiveOperationException | RuntimeException | LinkageError e) {
            Native.log(1, "[JavaLayer] the game's command dispatcher could not be made for plugins: " + e);
        }
    }

    /** Vanilla's dispatcher type; each root registered also becomes a server command. */
    static final class Dispatcher extends CommandDispatcher<CommandSourceStack> {
        @Override
        public LiteralCommandNode<CommandSourceStack> register(LiteralArgumentBuilder<CommandSourceStack> command) {
            LiteralCommandNode<CommandSourceStack> node = super.register(command);
            if (Commands.MAP.getCommand(node.getName()) == null) Commands.MAP.register("minecraft", new Run(this, node.getName()));
            return node;
        }
    }

    /** A root of the game's dispatcher as the server runs it. */
    static final class Run extends Command {
        private final Dispatcher dispatcher;

        Run(Dispatcher dispatcher, String name) {
            super(name, "", "/" + name, List.of());
            this.dispatcher = dispatcher;
        }

        @Override
        public boolean execute(CommandSender sender, String label, String[] args) {
            String line = args.length == 0 ? getName() : getName() + " " + String.join(" ", args);
            try {
                dispatcher.execute(line, stack(sender));
            } catch (CommandSyntaxException e) {
                Brigadier.Root.failure(sender, e, line);
            }
            return true;
        }

        @Override
        public List<String> tabComplete(CommandSender sender, String alias, String[] args) {
            String line = getName() + " " + String.join(" ", args);
            ParseResults<CommandSourceStack> parsed = dispatcher.parse(line, stack(sender));
            List<String> out = new ArrayList<>();
            for (Suggestion s : dispatcher.getCompletionSuggestions(parsed).join().getList()) out.add(s.getText());
            return out;
        }

        @Override
        public boolean testPermissionSilent(CommandSender target) {
            var node = dispatcher.getRoot().getChild(getName());
            return node == null || node.canUse(stack(target));
        }
    }

    /** A vanilla command source for a Bukkit sender: where the sender is, at an operator's level if they are one. */
    static CommandSourceStack stack(CommandSender sender) {
        MinecraftServer server = horizon.javalayer.nms.Hollow.server();
        org.bukkit.Location at = sender instanceof org.bukkit.entity.Entity e ? e.getLocation()
                : org.bukkit.Bukkit.getWorlds().isEmpty() ? null : org.bukkit.Bukkit.getWorlds().get(0).getSpawnLocation();
        var level = at == null || at.getWorld() == null ? null : horizon.javalayer.nms.Hollow.level(at.getWorld());
        var pos = at == null ? net.minecraft.world.phys.Vec3.ZERO : new net.minecraft.world.phys.Vec3(at.getX(), at.getY(), at.getZ());
        var rot = at == null ? net.minecraft.world.phys.Vec2.ZERO : new net.minecraft.world.phys.Vec2(at.getPitch(), at.getYaw());
        var permission = sender.isOp() ? net.minecraft.server.permissions.LevelBasedPermissionSet.OWNER : net.minecraft.server.permissions.LevelBasedPermissionSet.ALL;
        CommandSourceStack stack = new CommandSourceStack(new Source(sender), pos, rot, level, permission,
                io.papermc.paper.adventure.PaperAdventure.asVanilla(net.kyori.adventure.text.Component.text(sender.getName())), server);
        if (sender instanceof org.bukkit.craftbukkit.entity.CraftPlayer player) stack = stack.withEntity(horizon.javalayer.nms.Hollow.player(player));
        return stack;
    }

    /** Where a vanilla command's output goes: the Bukkit sender. */
    record Source(CommandSender sender) implements CommandSource {
        @Override public void sendSystemMessage(net.minecraft.network.chat.Component message) { sender.sendMessage(io.papermc.paper.adventure.PaperAdventure.asAdventure(message)); }
        @Override public boolean acceptsSuccess() { return true; }
        @Override public boolean acceptsFailure() { return true; }
        @Override public boolean shouldInformAdmins() { return false; }
        @Override public CommandSender getBukkitSender(CommandSourceStack stack) { return sender; }
    }
}
