package horizon.javalayer;

import com.mojang.brigadier.CommandDispatcher;
import com.mojang.brigadier.ParseResults;
import com.mojang.brigadier.StringReader;
import com.mojang.brigadier.arguments.ArgumentType;
import com.mojang.brigadier.arguments.StringArgumentType;
import com.mojang.brigadier.builder.LiteralArgumentBuilder;
import com.mojang.brigadier.builder.RequiredArgumentBuilder;
import com.mojang.brigadier.context.CommandContext;
import com.mojang.brigadier.exceptions.CommandSyntaxException;
import com.mojang.brigadier.exceptions.SimpleCommandExceptionType;
import com.mojang.brigadier.suggestion.Suggestion;
import com.mojang.brigadier.suggestion.Suggestions;
import com.mojang.brigadier.suggestion.SuggestionsBuilder;
import com.mojang.brigadier.tree.CommandNode;
import com.mojang.brigadier.tree.LiteralCommandNode;
import io.papermc.paper.command.brigadier.BasicCommand;
import io.papermc.paper.command.brigadier.CommandRegistrationFlag;
import io.papermc.paper.command.brigadier.CommandSourceStack;
import io.papermc.paper.plugin.configuration.PluginMeta;
import io.papermc.paper.plugin.lifecycle.event.registrar.ReloadableRegistrarEvent;
import net.kyori.adventure.text.Component;
import net.kyori.adventure.text.ComponentLike;
import net.kyori.adventure.text.format.NamedTextColor;
import org.bukkit.Bukkit;
import org.bukkit.Location;
import org.bukkit.command.Command;
import org.bukkit.command.CommandSender;
import org.bukkit.entity.Entity;
import org.bukkit.entity.Player;
import org.bukkit.plugin.Plugin;

import java.lang.reflect.InvocationHandler;
import java.lang.reflect.Method;
import java.lang.reflect.Proxy;
import java.util.*;
import java.util.concurrent.CompletableFuture;

/**
 * Paper's Brigadier commands: `LifecycleEvents.COMMANDS` handlers, `Commands.register(...)` and `BasicCommand`, run
 * on Brigadier itself in Java. Each root command is registered with the server as a plain command whose text comes
 * back here to be parsed and run, so arguments, permissions (requires) and suggestions behave as on Paper.
 */
public final class Brigadier {
    private Brigadier() {}

    static final CommandDispatcher<CommandSourceStack> DISPATCHER = new CommandDispatcher<>();
    private static final SimpleCommandExceptionType NOT_PLAYER = new SimpleCommandExceptionType(() -> "A player is required to run this command here");
    private static final SimpleCommandExceptionType NOT_ENTITY = new SimpleCommandExceptionType(() -> "An entity is required to run this command here");

    // ---- lifecycle events (LifecycleEventManager, event types, handler configurations) ----

    /** A lifecycle event type: its name, and the handler configurations it makes. */
    static final class TypeHandler implements InvocationHandler {
        final String name;

        TypeHandler(String name) { this.name = name; }

        @Override
        public Object invoke(Object proxy, Method m, Object[] args) throws Throwable {
            switch (m.getName()) {
                case "name": return name;
                case "toString": return "LifecycleEventType[" + name + "]";
                case "hashCode": return name.hashCode();
                case "equals": return proxy == args[0];
                case "newHandler": {
                    // Paper's event types are Prioritizable (COMMANDS...): their configuration is a prioritized one
                    // (CommandAPI casts to it), whichever interface's newHandler was called.
                    Class<?> made = m.getReturnType();
                    Class<?> prioritized = io.papermc.paper.plugin.lifecycle.event.handler.configuration.PrioritizedLifecycleEventHandlerConfiguration.class;
                    if (made.isAssignableFrom(prioritized)) made = prioritized;
                    return Proxy.newProxyInstance(Brigadier.class.getClassLoader(), new Class<?>[]{made}, new ConfigHandler(name, args[0]));
                }
                default: break;
            }
            if (m.isDefault()) return InvocationHandler.invokeDefault(proxy, m, args);
            return Conv.zero(m.getReturnType());
        }
    }

    static Object eventType(Class<?> type, String name) {
        return Proxy.newProxyInstance(Brigadier.class.getClassLoader(), new Class<?>[]{type}, new TypeHandler(name));
    }

    /** A handler configuration: which event, which handler; priority() and monitor() keep it. */
    static final class ConfigHandler implements InvocationHandler {
        final String type;
        final Object handler;

        ConfigHandler(String type, Object handler) {
            this.type = type;
            this.handler = handler;
        }

        @Override
        public Object invoke(Object proxy, Method m, Object[] args) throws Throwable {
            switch (m.getName()) {
                case "toString": return "LifecycleEventHandlerConfiguration[" + type + "]";
                case "hashCode": return System.identityHashCode(proxy);
                case "equals": return proxy == args[0];
                default: break;
            }
            if (m.getReturnType().isInstance(proxy)) return proxy;
            if (m.isDefault()) return InvocationHandler.invokeDefault(proxy, m, args);
            return Conv.zero(m.getReturnType());
        }
    }

    /** `plugin.getLifecycleManager()`: COMMANDS handlers run at once with the registrar of that plugin. */
    static Object lifecycleManager(Class<?> type, Plugin plugin) {
        return Proxy.newProxyInstance(Brigadier.class.getClassLoader(), new Class<?>[]{type}, (proxy, m, args) -> {
            switch (m.getName()) {
                case "toString": return "LifecycleEventManager[" + plugin.getName() + "]";
                case "hashCode": return System.identityHashCode(proxy);
                case "equals": return proxy == args[0];
                case "registerEventHandler":
                    if (args.length == 1) {
                        register(plugin, args[0]);
                        return null;
                    }
                    break;
                default: break;
            }
            if (m.isDefault()) return InvocationHandler.invokeDefault(proxy, m, args);
            return null;
        });
    }

    @SuppressWarnings({"unchecked", "rawtypes"})
    private static void register(Plugin plugin, Object configuration) {
        if (!Proxy.isProxyClass(configuration.getClass()) || !(Proxy.getInvocationHandler(configuration) instanceof ConfigHandler config)) return;
        if (config.type.equals("datapack_discovery")) {
            Object registrar = Datapacks.registrar(plugin);
            Object event = Proxy.newProxyInstance(Brigadier.class.getClassLoader(), new Class<?>[]{io.papermc.paper.plugin.lifecycle.event.registrar.RegistrarEvent.class}, (proxy, m, args) -> switch (m.getName()) {
                case "registrar" -> registrar;
                case "toString" -> "RegistrarEvent[datapack_discovery]";
                case "hashCode" -> System.identityHashCode(proxy);
                case "equals" -> proxy == args[0];
                default -> null;
            });
            ((io.papermc.paper.plugin.lifecycle.event.handler.LifecycleEventHandler) config.handler).run((io.papermc.paper.plugin.lifecycle.event.LifecycleEvent) event);
            return;
        }
        if (!config.type.equals("commands")) {
            NativeHandler.missing("lifecycle event " + config.type);
            return;
        }
        // Paper runs COMMANDS handlers once every plugin is enabled (the server's commands are built then), in the
        // order they were registered: a handler registered in onEnable must not run inside it (cloud locks its
        // registration in the handler, then goes on setting up). After start-up, at once.
        synchronized (PENDING) {
            if (!started) {
                PENDING.add(() -> runCommands(plugin, config));
                return;
            }
        }
        runCommands(plugin, config);
    }

    private static final List<Runnable> PENDING = new java.util.ArrayList<>();
    private static boolean started;

    /** Start-up's end (every plugin enabled): the COMMANDS handlers registered so far run, in order. */
    static void startupDone() {
        List<Runnable> run;
        synchronized (PENDING) {
            started = true;
            run = new java.util.ArrayList<>(PENDING);
            PENDING.clear();
        }
        for (Runnable r : run) r.run();
    }

    @SuppressWarnings({"unchecked", "rawtypes"})
    private static void runCommands(Plugin plugin, ConfigHandler config) {
        Registrar registrar = new Registrar(plugin);
        Object event = Proxy.newProxyInstance(Brigadier.class.getClassLoader(), new Class<?>[]{ReloadableRegistrarEvent.class}, (proxy, m, args) -> switch (m.getName()) {
            case "registrar" -> registrar;
            case "cause" -> ReloadableRegistrarEvent.Cause.INITIAL;
            case "toString" -> "ReloadableRegistrarEvent[commands]";
            case "hashCode" -> System.identityHashCode(proxy);
            case "equals" -> proxy == args[0];
            default -> null;
        });
        try {
            ((io.papermc.paper.plugin.lifecycle.event.handler.LifecycleEventHandler) config.handler).run((io.papermc.paper.plugin.lifecycle.event.LifecycleEvent) event);
        } catch (Throwable e) {
            Logs.severe(plugin.getName() + "'s command registration failed", e);
        }
    }

    // ---- the registrar ----

    /** Paper's `Commands` for one plugin. */
    static final class Registrar implements io.papermc.paper.command.brigadier.Commands {
        final Plugin plugin;

        Registrar(Plugin plugin) { this.plugin = plugin; }

        @Override public CommandDispatcher<CommandSourceStack> getDispatcher() { return DISPATCHER; }

        @Override
        public Set<String> register(LiteralCommandNode<CommandSourceStack> node, String description, Collection<String> aliases) {
            return register(plugin.getPluginMeta(), node, description, aliases);
        }

        @Override
        public Set<String> register(PluginMeta meta, LiteralCommandNode<CommandSourceStack> node, String description, Collection<String> aliases) {
            return root(meta == null ? plugin.getName() : meta.getName(), node, description, aliases == null ? List.of() : aliases);
        }

        @Override
        public Set<String> registerWithFlags(PluginMeta meta, LiteralCommandNode<CommandSourceStack> node, String description, Collection<String> aliases, Set<CommandRegistrationFlag> flags) {
            return register(meta, node, description, aliases);
        }

        @Override
        public Set<String> register(String label, String description, Collection<String> aliases, BasicCommand command) {
            return register(plugin.getPluginMeta(), label, description, aliases, command);
        }

        @Override
        public Set<String> register(PluginMeta meta, String label, String description, Collection<String> aliases, BasicCommand command) {
            LiteralArgumentBuilder<CommandSourceStack> root = LiteralArgumentBuilder.<CommandSourceStack>literal(label)
                .requires(source -> command.canUse(source.getSender()) && (command.permission() == null || source.getSender().hasPermission(command.permission())))
                .executes(context -> {
                    command.execute(context.getSource(), new String[0]);
                    return 1;
                })
                .then(RequiredArgumentBuilder.<CommandSourceStack, String>argument("args", StringArgumentType.greedyString())
                    .suggests((context, builder) -> basicSuggestions(command, context, builder))
                    .executes(context -> {
                        command.execute(context.getSource(), StringArgumentType.getString(context, "args").split(" ", -1));
                        return 1;
                    }));
            return register(meta, root.build(), description, aliases);
        }
    }

    private static CompletableFuture<Suggestions> basicSuggestions(BasicCommand command, CommandContext<CommandSourceStack> context, SuggestionsBuilder builder) {
        String all = builder.getRemaining();
        String[] args = all.split(" ", -1);
        int last = all.lastIndexOf(' ') + 1;
        SuggestionsBuilder at = builder.createOffset(builder.getStart() + last);
        for (String s : command.suggest(context.getSource(), args)) at.suggest(s);
        return at.buildFuture();
    }

    /** A root command and its aliases: in the dispatcher, and the server told of each. */
    private static Set<String> root(String owner, LiteralCommandNode<CommandSourceStack> node, String description, Collection<String> aliases) {
        Set<String> out = new LinkedHashSet<>();
        DISPATCHER.getRoot().addChild(node);
        out.add(node.getName());
        Commands.MAP.register(owner.toLowerCase(Locale.ROOT), new Root(node.getName(), description, new ArrayList<>(aliases)));
        for (String alias : aliases) {
            DISPATCHER.getRoot().addChild(LiteralArgumentBuilder.<CommandSourceStack>literal(alias).requires(node.getRequirement()).executes(node.getCommand()).redirect(node).build());
            out.add(alias);
        }
        return out;
    }

    /** The server's view of a Brigadier root: text in, parsed and run by the dispatcher. */
    static final class Root extends Command {
        Root(String name, String description, List<String> aliases) {
            super(name, description == null ? "" : description, "/" + name, aliases);
        }

        @Override
        public boolean execute(CommandSender sender, String label, String[] args) {
            String line = args.length == 0 ? label : label + " " + String.join(" ", args);
            Source source = new Source(sender, null, null);
            try {
                DISPATCHER.execute(line, source);
            } catch (CommandSyntaxException e) {
                failure(sender, e, line);
            }
            return true;
        }

        /** A syntax error as Paper 26.3 shows a player one (its Commands.performCommand, the UnknownCommandEvent
         *  message): ONE red message, the error in the game's words where it has them (BrigadierExceptions) and,
         *  when the error knows where in the line it is, a new line with the context: up to 10 characters before
         *  that point, the rest red and underlined, "<--[HERE]" (cmdparity: one chat entry on Paper). */
        static void failure(CommandSender sender, CommandSyntaxException e, String line) {
            String raw = e.getRawMessage() == null ? "" : e.getRawMessage().getString();
            if (raw.equals("Unknown command")) raw = "Unknown or incomplete command. See below for error";
            var message = Component.text().color(NamedTextColor.RED).append(Component.text(raw));
            String input = e.getInput();
            if (input != null && e.getCursor() >= 0) {
                int at = Math.min(input.length(), e.getCursor());
                var context = Component.text().color(NamedTextColor.GRAY)
                        .clickEvent(net.kyori.adventure.text.event.ClickEvent.suggestCommand("/" + input));
                if (at > 10) context.append(Component.text("..."));
                context.append(Component.text(input.substring(Math.max(0, at - 10), at)));
                if (at < input.length()) {
                    context.append(Component.text(input.substring(at), NamedTextColor.RED, net.kyori.adventure.text.format.TextDecoration.UNDERLINED));
                }
                context.append(Component.translatable("command.context.here", NamedTextColor.RED, net.kyori.adventure.text.format.TextDecoration.ITALIC));
                message.append(Component.newline(), context.build());
            }
            sender.sendMessage(message.build());
        }

        @Override
        public List<String> tabComplete(CommandSender sender, String alias, String[] args) {
            String line = alias + " " + String.join(" ", args);
            ParseResults<CommandSourceStack> parsed = DISPATCHER.parse(line, new Source(sender, null, null));
            List<String> out = new ArrayList<>();
            for (Suggestion s : DISPATCHER.getCompletionSuggestions(parsed).join().getList()) out.add(s.getText());
            return out;
        }

        @Override
        public boolean testPermissionSilent(CommandSender target) {
            CommandNode<CommandSourceStack> node = DISPATCHER.getRoot().getChild(getName());
            return node == null || node.canUse(new Source(target, null, null));
        }
    }

    // ---- the command source ----

    static final class Source implements CommandSourceStack {
        final CommandSender sender;
        final Location location;
        final Entity executor;

        Source(CommandSender sender, Location location, Entity executor) {
            this.sender = sender;
            this.location = location;
            this.executor = executor != null ? executor : sender instanceof Entity e ? e : null;
        }

        @Override
        public Location getLocation() {
            if (location != null) return location;
            if (executor != null) return executor.getLocation();
            return Bukkit.getWorlds().isEmpty() ? null : Bukkit.getWorlds().get(0).getSpawnLocation();
        }

        @Override public CommandSender getSender() { return sender; }
        @Override public Entity getExecutor() { return executor; }

        @Override
        public Player getPlayerOrThrow() throws CommandSyntaxException {
            if (executor instanceof Player p) return p;
            throw NOT_PLAYER.create();
        }

        @Override
        public Entity getEntityOrThrow() throws CommandSyntaxException {
            if (executor != null) return executor;
            throw NOT_ENTITY.create();
        }

        @Override public CommandSourceStack withLocation(Location l) { return new Source(sender, l, executor); }
        @Override public CommandSourceStack withExecutor(Entity e) { return new Source(sender, location, e); }
        @Override public void sendReply(ComponentLike message) { sender.sendMessage(message); }
        @Override public void sendSuccess(ComponentLike message, boolean broadcast) { sender.sendMessage(message); }
        @Override public void sendFailure(ComponentLike message) { sender.sendMessage(message.asComponent().colorIfAbsent(NamedTextColor.RED)); }
    }

    // ---- Paper's argument types (ArgumentTypes.player(), .world()...) ----

    static Object argument(String name, Object[] args) {
        return ArgumentTypes.of(name, args);
    }

    static <T> ArgumentType<T> type(Parser<T> parser, Suggest suggest) {
        return new ArgumentType<T>() {
            @Override public T parse(StringReader reader) throws CommandSyntaxException { return parser.parse(reader); }

            @Override
            public <S> CompletableFuture<Suggestions> listSuggestions(CommandContext<S> context, SuggestionsBuilder builder) {
                if (suggest == null) return Suggestions.empty();
                String remaining = builder.getRemaining().toLowerCase(Locale.ROOT);
                for (String s : suggest.all()) if (s.toLowerCase(Locale.ROOT).startsWith(remaining)) builder.suggest(s);
                return builder.buildFuture();
            }
        };
    }

    interface Parser<T> {
        T parse(StringReader reader) throws CommandSyntaxException;
    }

    interface Suggest {
        Collection<String> all();
    }

    static String token(StringReader reader) {
        int start = reader.getCursor();
        while (reader.canRead() && reader.peek() != ' ') reader.skip();
        return reader.getString().substring(start, reader.getCursor());
    }

    static List<String> playerNames() {
        List<String> out = new ArrayList<>();
        for (Player p : Bukkit.getOnlinePlayers()) out.add(p.getName());
        return out;
    }
}
