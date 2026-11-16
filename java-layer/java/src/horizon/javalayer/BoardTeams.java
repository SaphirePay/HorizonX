package horizon.javalayer;

import net.kyori.adventure.text.Component;
import net.kyori.adventure.text.format.NamedTextColor;
import net.kyori.adventure.text.serializer.legacy.LegacyComponentSerializer;
import org.bukkit.ChatColor;
import org.bukkit.OfflinePlayer;
import org.bukkit.entity.Entity;
import org.bukkit.scoreboard.NameTagVisibility;
import org.bukkit.scoreboard.Scoreboard;
import org.bukkit.scoreboard.Team;

import java.lang.reflect.InvocationHandler;
import java.lang.reflect.Proxy;
import java.util.*;
import java.util.concurrent.ConcurrentHashMap;

/**
 * Teams of the server's own scoreboards (getNewScoreboard): the server names a team by its text and takes each change
 * on the board (addTeamEntry(team, entry), setTeamOption(team, option, status)...). This is Bukkit's Team over that;
 * what the board doesn't hand back (prefix, color, options) is kept here as it was set, CraftTeam's defaults otherwise.
 */
final class BoardTeams {
    private BoardTeams() {}

    /** What a team was set to, by board and name. */
    static final class State {
        Component display, prefix = Component.empty(), suffix = Component.empty();
        NamedTextColor color;
        boolean friendlyFire = true, seeInvisibles = true;
        final Map<Team.Option, Team.OptionStatus> options = new EnumMap<>(Team.Option.class);
    }

    private static final Map<String, State> STATES = new ConcurrentHashMap<>();
    private static final Map<String, Team> VIEWS = new ConcurrentHashMap<>();

    private static String id(H board, String name) {
        return board.id + "|" + name;
    }

    static void forget(H board, String name) {
        STATES.remove(id(board, name));
        VIEWS.remove(id(board, name));
    }

    static Team team(H board, String name) {
        return VIEWS.computeIfAbsent(id(board, name), k -> make(board, name));
    }

    private static Object call(H board, String method, Object... args) {
        return Native.tryCall(board, method, Conv.args(args));
    }

    private static String entry(Object o) {
        if (o instanceof OfflinePlayer p) return p.getName();
        if (o instanceof Entity e) return e instanceof org.bukkit.entity.Player p ? p.getName() : e.getUniqueId().toString();
        return String.valueOf(o);
    }

    @SuppressWarnings("unchecked")
    private static Set<String> entries(H board, String name) {
        Object raw = call(board, "getTeamEntries", name);
        Object list = raw == Native.MISSING ? null : Conv.fromNative(raw, List.class);
        Set<String> out = new LinkedHashSet<>();
        if (list instanceof Collection<?> c) for (Object x : c) out.add(String.valueOf(x));
        return out;
    }

    private static Team make(H board, String name) {
        State st = STATES.computeIfAbsent(id(board, name), k -> {
            State s = new State();
            s.display = Component.text(name);
            return s;
        });
        InvocationHandler handler = (proxy, m, args) -> {
            String method = m.getName();
            int n = args == null ? 0 : args.length;
            switch (method) {
                case "getName": return name;
                case "getScoreboard": return Conv.fromNative(board, Scoreboard.class);
                case "toString": return "org.bukkit.craftbukkit.scoreboard.CraftTeam@" + Integer.toHexString(System.identityHashCode(proxy));
                case "hashCode": return id(board, name).hashCode();
                case "equals": return proxy == args[0];
                case "displayName":
                    if (n == 0) return st.display;
                    st.display = (Component) args[0];
                    call(board, "setTeamDisplayName", name, st.display);
                    return null;
                case "prefix":
                    if (n == 0) return st.prefix;
                    st.prefix = (Component) args[0];
                    call(board, "setTeamPrefix", name, st.prefix);
                    return null;
                case "suffix":
                    if (n == 0) return st.suffix;
                    st.suffix = (Component) args[0];
                    call(board, "setTeamSuffix", name, st.suffix);
                    return null;
                case "getDisplayName": return LegacyComponentSerializer.legacySection().serialize(st.display);
                case "setDisplayName":
                    st.display = LegacyComponentSerializer.legacySection().deserialize(String.valueOf(args[0]));
                    call(board, "setTeamDisplayName", name, st.display);
                    return null;
                case "getPrefix": return LegacyComponentSerializer.legacySection().serialize(st.prefix);
                case "setPrefix":
                    st.prefix = LegacyComponentSerializer.legacySection().deserialize(String.valueOf(args[0]));
                    call(board, "setTeamPrefix", name, st.prefix);
                    return null;
                case "getSuffix": return LegacyComponentSerializer.legacySection().serialize(st.suffix);
                case "setSuffix":
                    st.suffix = LegacyComponentSerializer.legacySection().deserialize(String.valueOf(args[0]));
                    call(board, "setTeamSuffix", name, st.suffix);
                    return null;
                case "hasColor": return st.color != null;
                case "color":
                    if (n == 0) {
                        if (st.color == null) throw new IllegalStateException("Team colors must have hex values");
                        return st.color;
                    }
                    st.color = (NamedTextColor) args[0];
                    call(board, "setTeamColor", name, st.color == null ? "RESET" : st.color.toString().toUpperCase(Locale.ROOT));
                    return null;
                case "getColor": {
                    if (st.color == null) return ChatColor.RESET;
                    return ChatColor.valueOf(st.color.toString().toUpperCase(Locale.ROOT));
                }
                case "setColor": {
                    ChatColor c = (ChatColor) args[0];
                    st.color = c == null || !c.isColor() ? null : NamedTextColor.NAMES.value(c.name().toLowerCase(Locale.ROOT));
                    call(board, "setTeamColor", name, c == null ? "RESET" : c.name());
                    return null;
                }
                case "allowFriendlyFire": return st.friendlyFire;
                case "setAllowFriendlyFire":
                    st.friendlyFire = (Boolean) args[0];
                    call(board, "setTeamFriendlyFire", name, st.friendlyFire);
                    return null;
                case "canSeeFriendlyInvisibles": return st.seeInvisibles;
                case "setCanSeeFriendlyInvisibles":
                    st.seeInvisibles = (Boolean) args[0];
                    call(board, "setTeamSeeInvisibles", name, st.seeInvisibles);
                    return null;
                case "getOption": return st.options.getOrDefault((Team.Option) args[0], Team.OptionStatus.ALWAYS);
                case "setOption":
                    st.options.put((Team.Option) args[0], (Team.OptionStatus) args[1]);
                    call(board, "setTeamOption", name, ((Team.Option) args[0]).name(), ((Team.OptionStatus) args[1]).name());
                    return null;
                case "getNameTagVisibility": {
                    Team.OptionStatus s = st.options.getOrDefault(Team.Option.NAME_TAG_VISIBILITY, Team.OptionStatus.ALWAYS);
                    return switch (s) {
                        case NEVER -> NameTagVisibility.NEVER;
                        case FOR_OTHER_TEAMS -> NameTagVisibility.HIDE_FOR_OWN_TEAM;
                        case FOR_OWN_TEAM -> NameTagVisibility.HIDE_FOR_OTHER_TEAMS;
                        default -> NameTagVisibility.ALWAYS;
                    };
                }
                case "setNameTagVisibility": {
                    Team.OptionStatus s = switch ((NameTagVisibility) args[0]) {
                        case NEVER -> Team.OptionStatus.NEVER;
                        case HIDE_FOR_OWN_TEAM -> Team.OptionStatus.FOR_OTHER_TEAMS;
                        case HIDE_FOR_OTHER_TEAMS -> Team.OptionStatus.FOR_OWN_TEAM;
                        default -> Team.OptionStatus.ALWAYS;
                    };
                    st.options.put(Team.Option.NAME_TAG_VISIBILITY, s);
                    call(board, "setTeamOption", name, Team.Option.NAME_TAG_VISIBILITY.name(), s.name());
                    return null;
                }
                case "getEntries": return entries(board, name);
                case "getSize": return entries(board, name).size();
                case "getPlayers": {
                    Set<OfflinePlayer> out = new LinkedHashSet<>();
                    for (String e : entries(board, name)) out.add(org.bukkit.Bukkit.getOfflinePlayer(e));
                    return out;
                }
                case "hasEntry": return entries(board, name).contains(String.valueOf(args[0]));
                case "hasPlayer": case "hasEntity": return entries(board, name).contains(entry(args[0]));
                case "addEntry": case "addPlayer": case "addEntity":
                    call(board, "addTeamEntry", name, entry(args[0]));
                    return null;
                case "addEntries": case "addEntities": {
                    Collection<?> all = args[0] instanceof Collection<?> c ? c : Arrays.asList((Object[]) args[0]);
                    for (Object o : all) call(board, "addTeamEntry", name, entry(o));
                    return null;
                }
                case "removeEntry": case "removePlayer": case "removeEntity": {
                    Object r = call(board, "removeTeamEntry", name, entry(args[0]));
                    return r instanceof Boolean b ? b : Boolean.TRUE;
                }
                case "removeEntries": case "removeEntities": {
                    Collection<?> all = args[0] instanceof Collection<?> c ? c : Arrays.asList((Object[]) args[0]);
                    boolean any = false;
                    for (Object o : all) {
                        Object r = call(board, "removeTeamEntry", name, entry(o));
                        any |= r instanceof Boolean b && b;
                    }
                    return any;
                }
                case "unregister":
                    call(board, "unregisterTeam", name);
                    forget(board, name);
                    return null;
                case "audiences": {
                    List<net.kyori.adventure.audience.Audience> out = new ArrayList<>();
                    for (String e : entries(board, name)) {
                        org.bukkit.entity.Player p = org.bukkit.Bukkit.getPlayerExact(e);
                        if (p != null) out.add(p);
                    }
                    return out;
                }
                default: break;
            }
            if (m.isDefault()) return InvocationHandler.invokeDefault(proxy, m, args);
            throw Report.cant("Team." + method + "/" + n);
        };
        return (Team) Proxy.newProxyInstance(BoardTeams.class.getClassLoader(), new Class<?>[]{Team.class}, handler);
    }
}
