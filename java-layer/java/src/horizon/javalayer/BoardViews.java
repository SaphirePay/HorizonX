package horizon.javalayer;

import org.bukkit.OfflinePlayer;
import org.bukkit.entity.Entity;
import org.bukkit.scoreboard.Objective;
import org.bukkit.scoreboard.Score;
import org.bukkit.scoreboard.Scoreboard;

import java.lang.reflect.InvocationHandler;
import java.lang.reflect.Method;
import java.lang.reflect.Proxy;
import java.util.*;

/**
 * Objectives and scores of the server's own scoreboards (getNewScoreboard): the server names an objective by its
 * text and answers its methods on the board, objective first (setDisplaySlot(objective, slot), getScore(objective,
 * entry)...). These are Bukkit's Objective and Score over that.
 */
public final class BoardViews {
    private BoardViews() {}

    /** A Bukkit view over a board's objective: its board handle and name. */
    public interface ObjectiveView {
        H horizonBoard();
        String horizonObjective();
    }

    static Object[] cat(Object[] head, Object[] rest) {
        Object[] out = Arrays.copyOf(head, head.length + (rest == null ? 0 : rest.length));
        if (rest != null) System.arraycopy(rest, 0, out, head.length, rest.length);
        return out;
    }

    static Object plain(Object o) {
        if (o instanceof Enum<?> e) return e.name();
        if (o instanceof OfflinePlayer p) return p.getName();
        if (o instanceof Entity e) return e instanceof org.bukkit.entity.Player p ? p.getName() : e.getUniqueId().toString();
        return o;
    }

    static Objective objective(H board, String name) {
        InvocationHandler handler = (proxy, m, args) -> {
            String method = m.getName();
            int n = args == null ? 0 : args.length;
            switch (method) {
                case "horizonBoard": return board;
                case "horizonObjective": return name;
                case "getName": return name;
                case "getScoreboard": return Conv.fromNative(board, Scoreboard.class);
                case "isModifiable": return true;
                case "getScore":
                case "getScoreFor":
                    if (n == 1) return score(board, name, String.valueOf(plain(args[0])));
                    break;
                case "toString": return "org.bukkit.craftbukkit.scoreboard.CraftObjective" + "@" + Integer.toHexString(System.identityHashCode(proxy));
                case "hashCode": return (board.key + "|" + name).hashCode();
                case "equals": return args[0] instanceof ObjectiveView v && v.horizonObjective().equals(name) && v.horizonBoard().id == board.id;
                default: break;
            }
            Object[] call = cat(new Object[]{name}, Conv.args(Arrays.stream(args == null ? new Object[0] : args).map(BoardViews::plain).toArray()));
            Object raw = Native.tryCall(board, method, call);
            if (raw == Native.MISSING && method.equals("unregister")) raw = Native.tryCall(board, "unregisterObjective", call);
            if (raw == Native.MISSING) {
                if (m.isDefault()) return InvocationHandler.invokeDefault(proxy, m, args);
                throw Report.cant("Objective." + method + "/" + n);
            }
            return fromBoard(board, raw, m);
        };
        return (Objective) Proxy.newProxyInstance(BoardViews.class.getClassLoader(), new Class<?>[]{Objective.class, ObjectiveView.class}, handler);
    }

    static Score score(H board, String objective, String entry) {
        InvocationHandler handler = (proxy, m, args) -> {
            String method = m.getName();
            int n = args == null ? 0 : args.length;
            switch (method) {
                case "getEntry": return entry;
                case "getPlayer": return org.bukkit.Bukkit.getOfflinePlayer(entry);
                case "getObjective": return objective(board, objective);
                case "getScoreboard": return Conv.fromNative(board, Scoreboard.class);
                case "getScore": {
                    Object raw = Native.tryCall(board, "getScore", new Object[]{objective, entry});
                    Object v = Conv.fromNative(raw, Integer.class);
                    return v instanceof Number num ? num.intValue() : 0;
                }
                case "setScore":
                    Native.tryCall(board, "setScore", new Object[]{objective, entry, args[0]});
                    return null;
                case "isScoreSet": {
                    Object raw = Native.tryCall(board, "isScoreSet", new Object[]{objective, entry});
                    if (raw instanceof Boolean b) return b;
                    Object v = Native.tryCall(board, "getScore", new Object[]{objective, entry});
                    return v != null && v != Native.MISSING;
                }
                case "resetScore":
                    Native.tryCall(board, "resetScore", new Object[]{objective, entry});
                    return null;
                case "toString": return "org.bukkit.craftbukkit.scoreboard.CraftScore" + "@" + Integer.toHexString(System.identityHashCode(proxy));
                case "hashCode": return (objective + "|" + entry).hashCode();
                case "equals": return proxy == args[0];
                default: break;
            }
            Object raw = Native.tryCall(board, method, cat(new Object[]{objective, entry}, Conv.args(args)));
            if (raw == Native.MISSING) {
                if (m.isDefault()) return InvocationHandler.invokeDefault(proxy, m, args);
                throw Report.cant("Score." + method + "/" + n);
            }
            return fromBoard(board, raw, m);
        };
        return (Score) Proxy.newProxyInstance(BoardViews.class.getClassLoader(), new Class<?>[]{Score.class}, handler);
    }

    /** A board's answer as Java: objective names as objectives (one or a collection). */
    static Object fromBoard(H board, Object raw, Method m) {
        Class<?> r = m.getReturnType();
        if (r == Objective.class) {
            Object v = Conv.fromNative(raw, String.class);
            return v instanceof String s ? objective(board, s) : Conv.fromNative(raw, m.getGenericReturnType());
        }
        if (Collection.class.isAssignableFrom(r) && m.getGenericReturnType() instanceof java.lang.reflect.ParameterizedType p && p.getActualTypeArguments()[0] == Objective.class) {
            Object list = Conv.fromNative(raw, List.class);
            Collection<Object> out = Set.class.isAssignableFrom(r) ? new LinkedHashSet<>() : new ArrayList<>();
            if (list instanceof Collection<?> c) for (Object x : c) out.add(x instanceof String s ? objective(board, s) : x);
            return out;
        }
        return Conv.fromNative(raw, m.getGenericReturnType());
    }

    /** Scoreboard methods that hand out objectives, on a board that names them by text. NOT_MINE otherwise. */
    static Object board(Object proxy, H h, Method m, String name, Object[] args, Object raw) {
        int n = args == null ? 0 : args.length;
        if (name.equals("registerNewObjective") && raw instanceof Boolean ok && n >= 1) return ok ? objective(h, String.valueOf(args[0])) : null;
        // Teams, named by text on such a board: Bukkit's Team over them (BoardTeams). CraftScoreboard.registerNewTeam
        // refuses a name in use.
        if (name.equals("registerNewTeam") && raw instanceof Boolean ok && n >= 1) {
            if (!ok) throw new IllegalArgumentException("Team name '" + args[0] + "' is already in use");
            return BoardTeams.team(h, String.valueOf(args[0]));
        }
        Class<?> rt = m.getReturnType();
        if (rt == org.bukkit.scoreboard.Team.class) {
            Object v = Conv.fromNative(raw, Object.class);
            if (v instanceof String s) return BoardTeams.team(h, s);
            if (v == null) return null;
        }
        if (Collection.class.isAssignableFrom(rt) && m.getGenericReturnType() instanceof java.lang.reflect.ParameterizedType tp && tp.getActualTypeArguments()[0] == org.bukkit.scoreboard.Team.class) {
            Object list = Conv.fromNative(raw, List.class);
            if (list instanceof Collection<?> c && (c.isEmpty() || c.iterator().next() instanceof String)) {
                Set<Object> out = new LinkedHashSet<>();
                for (Object x : c) out.add(BoardTeams.team(h, String.valueOf(x)));
                return out;
            }
        }
        Class<?> r = m.getReturnType();
        boolean objectives = r == Objective.class || (Collection.class.isAssignableFrom(r) && m.getGenericReturnType() instanceof java.lang.reflect.ParameterizedType p && p.getActualTypeArguments()[0] == Objective.class);
        if (!objectives) return NativeHandler.NOT_MINE;
        Object probe = Conv.fromNative(raw, Object.class);
        if (probe instanceof String || probe instanceof Collection<?> c && (c.isEmpty() || c.iterator().next() instanceof String)) return fromBoard(h, raw, m);
        return NativeHandler.NOT_MINE;
    }
}
