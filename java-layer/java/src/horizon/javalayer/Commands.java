package horizon.javalayer;

import org.bukkit.Location;
import org.bukkit.command.*;
import org.bukkit.plugin.Plugin;
import org.bukkit.plugin.java.JavaPlugin;

import java.lang.reflect.Constructor;
import java.util.*;
import java.util.concurrent.ConcurrentHashMap;

/**
 * Plugins' commands: plugin.yml's (PluginCommand) and any `Command` put in the command map. It is a real
 * SimpleCommandMap (plugins cast to it and read its `knownCommands` field by reflection), made without
 * SimpleCommandMap's constructor (which registers Bukkit's own commands): `knownCommands` is this map's `known`.
 */
public final class Commands extends SimpleCommandMap {
    public static final Commands MAP = make();

    /** By name, alias and `plugin:name`, lower case (also SimpleCommandMap's knownCommands). */
    private Map<String, Command> known;
    /** What the server routes to each registered name: (owner, name) -> command. */
    private Map<String, Command> routed;

    private Commands() {
        super(null, null);
    }

    private static Commands make() {
        Commands map = Craft.allocate(Commands.class);
        Map<String, Command> known = new ConcurrentHashMap<>();
        Craft.set(map, SimpleCommandMap.class, "knownCommands", known);
        map.known = known;
        map.routed = new ConcurrentHashMap<>();
        return map;
    }

    private static PluginCommand newPluginCommand(String name, Plugin plugin) {
        try {
            Constructor<PluginCommand> k = PluginCommand.class.getDeclaredConstructor(String.class, Plugin.class);
            k.setAccessible(true);
            return k.newInstance(name, plugin);
        } catch (ReflectiveOperationException e) {
            throw new IllegalStateException(e);
        }
    }

    /** plugin.yml's `commands:`. */
    @SuppressWarnings("unchecked")
    void fromDescription(JavaPlugin plugin) {
        Map<String, Map<String, Object>> commands = plugin.getDescription().getCommands();
        if (commands == null) return;
        for (Map.Entry<String, Map<String, Object>> e : commands.entrySet()) {
            String name = e.getKey();
            Map<String, Object> data = e.getValue() == null ? Map.of() : e.getValue();
            PluginCommand command = newPluginCommand(name, plugin);
            Object description = data.get("description");
            if (description != null) command.setDescription(description.toString());
            Object usage = data.get("usage");
            if (usage != null) command.setUsage(usage.toString());
            Object aliases = data.get("aliases");
            List<String> list = new ArrayList<>();
            if (aliases instanceof List<?> l) for (Object a : l) list.add(String.valueOf(a));
            else if (aliases != null) list.add(aliases.toString());
            command.setAliases(list);
            Object permission = data.get("permission");
            if (permission != null) command.setPermission(permission.toString());
            Object message = data.get("permission-message");
            if (message != null) command.setPermissionMessage(message.toString());
            register(plugin.getName().toLowerCase(Locale.ROOT), command);
        }
    }

    PluginCommand pluginCommand(String name) {
        Command c = known.get(name.toLowerCase(Locale.ROOT));
        return c instanceof PluginCommand p ? p : null;
    }

    void removeAll(Plugin plugin) {
        known.values().removeIf(c -> c instanceof PluginIdentifiableCommand p && p.getPlugin() == plugin);
        routed.values().removeIf(c -> c instanceof PluginIdentifiableCommand p && p.getPlugin() == plugin);
    }

    // ---- the server's calls ----

    boolean execute(String owner, String name, Object rawSender, String label, String[] args) {
        Command command = routed.get(owner + ":" + name);
        if (command == null) command = known.get(name.toLowerCase(Locale.ROOT));
        if (command == null) return false;
        CommandSender sender = (CommandSender) Conv.fromNative(rawSender, CommandSender.class);
        try {
            command.execute(sender, label, args);
        } catch (Throwable e) {
            Throwable cause = e instanceof CommandException && e.getCause() != null ? e.getCause() : e;
            Logs.severe("Unhandled exception executing command '" + label + "' in " + describe(command), cause);
            Synth.exception(new com.destroystokyo.paper.exception.ServerCommandException("Unhandled exception executing command '" + label + "' in " + describe(command), cause, command, sender, args));
            // Paper 26.3 runs a player's command through Brigadier: vanilla's command.failed, red, the error on hover.
            if (sender instanceof org.bukkit.entity.Player) {
                String what = "Unhandled exception executing command '" + label + "' in " + describe(command);
                sender.sendMessage(net.kyori.adventure.text.Component.translatable("command.failed", net.kyori.adventure.text.format.NamedTextColor.RED)
                        .hoverEvent(net.kyori.adventure.text.event.HoverEvent.showText(net.kyori.adventure.text.Component.text(what))));
            } else if (sender != null) {
                sender.sendMessage("§cAn internal error occurred while attempting to perform this command");
            }
        }
        return true;
    }

    List<String> tab(String owner, String name, Object rawSender, String alias, String[] args) {
        Command command = routed.get(owner + ":" + name);
        if (command == null) command = known.get(name.toLowerCase(Locale.ROOT));
        if (command == null) return List.of();
        CommandSender sender = (CommandSender) Conv.fromNative(rawSender, CommandSender.class);
        // SimpleCommandMap.tabComplete: nothing for a sender the command's permission refuses.
        if (!command.testPermissionSilent(sender)) return List.of();
        try {
            List<String> out = command.tabComplete(sender, alias, args);
            return out == null ? List.of() : out;
        } catch (Throwable e) {
            Logs.severe("Unhandled exception tab-completing '" + alias + "' in " + describe(command), e);
            return List.of();
        }
    }

    private static String describe(Command command) {
        return command instanceof PluginIdentifiableCommand p ? p.getPlugin().getDescription().getFullName() : command.getName();
    }

    // ---- CommandMap ----

    @Override
    public void registerAll(String fallback, List<Command> commands) {
        if (commands != null) for (Command c : commands) register(fallback, c);
    }

    @Override
    public boolean register(String label, String fallback, Command command) {
        String name = label.toLowerCase(Locale.ROOT).trim();
        String prefix = fallback.toLowerCase(Locale.ROOT).trim();
        boolean free = !known.containsKey(name);
        // SimpleCommandMap.register: a label another command has as its own name stays that command's (the first to
        // register, SimpleEconomy's and xwojteek-core's /baltop: Paper keeps the one enabled first); this one is
        // reached by `prefix:name` only. Told to the server it took the label's executor over.
        Command holder = known.get(name);
        if (holder != null && holder != command && holder.getName().equalsIgnoreCase(name)) {
            known.put(prefix + ":" + name, command);
            routed.put(prefix + ":" + name, command);
            command.setLabel(prefix + ":" + name);
            command.register(this);
            return false;
        }
        known.putIfAbsent(name, command);
        known.put(prefix + ":" + name, command);
        for (String alias : command.getAliases()) {
            known.putIfAbsent(alias.toLowerCase(Locale.ROOT), command);
            known.put(prefix + ":" + alias.toLowerCase(Locale.ROOT), command);
        }
        routed.put(prefix + ":" + name, command);
        String permission = command.getPermission();
        boolean server = Native.registerCommand(prefix, name, command.getAliases().toArray(new String[0]), command.getDescription(), command.getUsage(), permission == null ? "" : permission);
        command.setLabel(name);
        command.register(this);
        return free && server;
    }

    @Override
    public boolean register(String fallback, Command command) {
        return register(command.getName(), fallback, command);
    }

    @Override
    public boolean dispatch(CommandSender sender, String line) throws CommandException {
        Object done = Native.tryStatic("Bukkit", "dispatchCommand", Conv.args(new Object[]{sender, line}));
        return done instanceof Boolean b && b;
    }

    @Override public void clearCommands() { known.clear(); }

    @Override public Command getCommand(String name) { return known.get(name.toLowerCase(Locale.ROOT)); }

    @Override
    public List<String> tabComplete(CommandSender sender, String line) {
        String[] parts = line.split(" ", -1);
        Command c = getCommand(parts[0].startsWith("/") ? parts[0].substring(1) : parts[0]);
        if (c == null) return null;
        return c.tabComplete(sender, parts[0], Arrays.copyOfRange(parts, 1, parts.length));
    }

    @Override public List<String> tabComplete(CommandSender sender, String line, Location location) { return tabComplete(sender, line); }

    @Override public Map<String, Command> getKnownCommands() { return known; }
}
