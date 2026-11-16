package horizon.javalayer;

import com.google.common.collect.Range;
import com.mojang.brigadier.StringReader;
import com.mojang.brigadier.arguments.ArgumentType;
import com.mojang.brigadier.exceptions.CommandSyntaxException;
import com.mojang.brigadier.exceptions.DynamicCommandExceptionType;
import io.papermc.paper.command.brigadier.CommandSourceStack;
import io.papermc.paper.math.Position;
import net.kyori.adventure.key.Key;
import net.kyori.adventure.text.format.NamedTextColor;
import net.kyori.adventure.text.format.TextColor;
import net.kyori.adventure.text.serializer.gson.GsonComponentSerializer;
import org.bukkit.Bukkit;
import org.bukkit.GameMode;
import org.bukkit.Location;
import org.bukkit.Material;
import org.bukkit.NamespacedKey;
import org.bukkit.World;
import org.bukkit.entity.Entity;
import org.bukkit.entity.Player;

import java.lang.reflect.Proxy;
import java.util.*;
import java.util.function.Function;

/** Paper's vanilla argument types (`ArgumentTypes.player()`, `.world()`...), parsed here from the command's text. */
final class ArgumentTypes {
    private ArgumentTypes() {}

    private static final DynamicCommandExceptionType BAD = new DynamicCommandExceptionType(what -> () -> "Invalid " + what);
    private static final DynamicCommandExceptionType NO_PLAYER = new DynamicCommandExceptionType(name -> () -> "No player was found: " + name);
    private static final DynamicCommandExceptionType NO_ENTITY = new DynamicCommandExceptionType(name -> () -> "No entity was found: " + name);

    /** A resolver (`PlayerSelectorArgumentResolver`...): its one method, `resolve(source)`. */
    @SuppressWarnings("unchecked")
    static <R> R resolver(Class<R> type, Function<CommandSourceStack, Object> body) {
        return (R) Proxy.newProxyInstance(ArgumentTypes.class.getClassLoader(), new Class<?>[]{type}, (proxy, m, args) -> switch (m.getName()) {
            case "resolve" -> body.apply((CommandSourceStack) args[0]);
            case "toString" -> type.getSimpleName();
            case "hashCode" -> System.identityHashCode(proxy);
            case "equals" -> proxy == args[0];
            default -> null;
        });
    }

    private static Class<?> cls(String name) {
        try {
            return Class.forName(name, false, ArgumentTypes.class.getClassLoader());
        } catch (ClassNotFoundException e) {
            throw new IllegalStateException(e);
        }
    }

    /** Selector or name: `@p @a @r @s @e`, a player's name, or a UUID. */
    static List<Entity> select(String text, CommandSourceStack source, boolean playersOnly) {
        List<Entity> out = new ArrayList<>();
        String base = text.contains("[") ? text.substring(0, text.indexOf('[')) : text;
        Location at = source.getLocation();
        switch (base) {
            case "@a" -> out.addAll(Bukkit.getOnlinePlayers());
            case "@s" -> {
                if (source.getExecutor() != null) out.add(source.getExecutor());
            }
            case "@p", "@n" -> {
                Player best = null;
                double distance = Double.MAX_VALUE;
                for (Player p : Bukkit.getOnlinePlayers()) {
                    double d = at == null || p.getWorld() != at.getWorld() ? Double.MAX_VALUE - 1 : p.getLocation().distanceSquared(at);
                    if (d < distance) {
                        distance = d;
                        best = p;
                    }
                }
                if (best != null) out.add(best);
            }
            case "@r" -> {
                List<Player> all = new ArrayList<>(Bukkit.getOnlinePlayers());
                if (!all.isEmpty()) out.add(all.get(new Random().nextInt(all.size())));
            }
            case "@e" -> {
                if (playersOnly) out.addAll(Bukkit.getOnlinePlayers());
                else for (World w : Bukkit.getWorlds()) out.addAll(w.getEntities());
            }
            default -> {
                Player p = Bukkit.getPlayerExact(text);
                if (p != null) out.add(p);
                else {
                    try {
                        Entity e = Bukkit.getEntity(UUID.fromString(text));
                        if (e != null && (!playersOnly || e instanceof Player)) out.add(e);
                    } catch (IllegalArgumentException ignored) {}
                }
            }
        }
        return out;
    }

    static List<String> selectorSuggestions(boolean entities) {
        List<String> out = new ArrayList<>(List.of("@p", "@a", "@r", "@s"));
        if (entities) out.add("@e");
        out.addAll(Brigadier.playerNames());
        return out;
    }

    private static double coordinate(String text, double base) throws CommandSyntaxException {
        try {
            if (text.startsWith("~")) return base + (text.length() > 1 ? Double.parseDouble(text.substring(1)) : 0);
            return Double.parseDouble(text);
        } catch (NumberFormatException e) {
            throw BAD.create("coordinate " + text);
        }
    }

    /** Vanilla's Vec3/Vec2 argument: each coordinate as WorldCoordinate.parseDouble reads it, the game's messages. */
    private static String[] coordinates(StringReader reader, int count) throws CommandSyntaxException {
        String[] out = new String[count];
        for (int i = 0; i < count; i++) {
            if (i > 0) {
                if (!reader.canRead() || reader.peek() != ' ') {
                    throw new com.mojang.brigadier.exceptions.SimpleCommandExceptionType(new com.mojang.brigadier.LiteralMessage("Incomplete (expected " + count + " coordinates)")).createWithContext(reader);
                }
                reader.skip();
            }
            if (!reader.canRead()) throw CommandSyntaxException.BUILT_IN_EXCEPTIONS.readerExpectedDouble().createWithContext(reader);
            int start = reader.getCursor();
            out[i] = Brigadier.token(reader);
            String number = out[i].startsWith("~") || out[i].startsWith("^") ? out[i].substring(1) : out[i];
            if (!number.isEmpty()) {
                try {
                    Double.parseDouble(number);
                } catch (NumberFormatException e) {
                    reader.setCursor(start);
                    // StringReader.readDouble: no number characters at all is "Expected double".
                    if (number.chars().noneMatch(c -> c >= '0' && c <= '9' || c == '.' || c == '-')) {
                        throw CommandSyntaxException.BUILT_IN_EXCEPTIONS.readerExpectedDouble().createWithContext(reader);
                    }
                    throw CommandSyntaxException.BUILT_IN_EXCEPTIONS.readerInvalidDouble().createWithContext(reader, number);
                }
            } else if (!out[i].startsWith("~") && !out[i].startsWith("^")) {
                throw CommandSyntaxException.BUILT_IN_EXCEPTIONS.readerExpectedDouble().createWithContext(reader);
            }
        }
        return out;
    }

    private static int ticks(String text) throws CommandSyntaxException {
        try {
            if (text.endsWith("d")) return (int) (Double.parseDouble(text.substring(0, text.length() - 1)) * 24000);
            if (text.endsWith("s")) return (int) (Double.parseDouble(text.substring(0, text.length() - 1)) * 20);
            if (text.endsWith("t")) return (int) Double.parseDouble(text.substring(0, text.length() - 1));
            return (int) Double.parseDouble(text);
        } catch (NumberFormatException e) {
            throw BAD.create("time " + text);
        }
    }

    @SuppressWarnings({"unchecked", "rawtypes"})
    static Object of(String name, Object[] args) {
        switch (name) {
            case "player":
            case "players": {
                boolean one = name.equals("player");
                Class<?> type = cls("io.papermc.paper.command.brigadier.argument.resolvers.selector.PlayerSelectorArgumentResolver");
                return Brigadier.type(reader -> {
                    String text = Brigadier.token(reader);
                    return resolver(type, source -> {
                        List<Player> out = new ArrayList<>();
                        for (Entity e : select(text, source, true)) if (e instanceof Player p) out.add(p);
                        if (out.isEmpty()) throw sneaky(NO_PLAYER.create(text));
                        return one ? out.subList(0, 1) : out;
                    });
                }, () -> selectorSuggestions(false));
            }
            case "entity":
            case "entities": {
                boolean one = name.equals("entity");
                Class<?> type = cls("io.papermc.paper.command.brigadier.argument.resolvers.selector.EntitySelectorArgumentResolver");
                return Brigadier.type(reader -> {
                    String text = Brigadier.token(reader);
                    return resolver(type, source -> {
                        List<Entity> out = select(text, source, false);
                        if (out.isEmpty()) throw sneaky(NO_ENTITY.create(text));
                        return one ? out.subList(0, 1) : out;
                    });
                }, () -> selectorSuggestions(true));
            }
            case "playerProfiles": {
                Class<?> type = cls("io.papermc.paper.command.brigadier.argument.resolvers.PlayerProfileListResolver");
                return Brigadier.type(reader -> {
                    String text = Brigadier.token(reader);
                    return resolver(type, source -> {
                        List<Object> out = new ArrayList<>();
                        List<Entity> found = select(text, source, true);
                        if (found.isEmpty()) out.add(Bukkit.createProfile(text));
                        for (Entity e : found) out.add(((Player) e).getPlayerProfile());
                        return out;
                    });
                }, Brigadier::playerNames);
            }
            case "blockPosition": {
                Class<?> type = cls("io.papermc.paper.command.brigadier.argument.resolvers.BlockPositionResolver");
                return Brigadier.type(reader -> {
                    String[] c = coordinates(reader, 3);
                    return resolver(type, source -> {
                        Location at = source.getLocation();
                        try {
                            return Position.block((int) Math.floor(coordinate(c[0], at.getBlockX())), (int) Math.floor(coordinate(c[1], at.getBlockY())), (int) Math.floor(coordinate(c[2], at.getBlockZ())));
                        } catch (CommandSyntaxException e) {
                            throw sneaky(e);
                        }
                    });
                }, () -> List.of("~ ~ ~"));
            }
            case "finePosition": {
                Class<?> type = cls("io.papermc.paper.command.brigadier.argument.resolvers.FinePositionResolver");
                return Brigadier.type(reader -> {
                    String[] c = coordinates(reader, 3);
                    return resolver(type, source -> {
                        Location at = source.getLocation();
                        try {
                            return Position.fine(coordinate(c[0], at.getX()), coordinate(c[1], at.getY()), coordinate(c[2], at.getZ()));
                        } catch (CommandSyntaxException e) {
                            throw sneaky(e);
                        }
                    });
                }, () -> List.of("~ ~ ~"));
            }
            case "world":
                return Brigadier.type(reader -> {
                    String text = Brigadier.token(reader);
                    World w = Bukkit.getWorld(text);
                    if (w == null) {
                        NamespacedKey key = NamespacedKey.fromString(text);
                        if (key != null) w = Bukkit.getWorld(key);
                    }
                    if (w == null) throw BAD.create("world " + text);
                    return w;
                }, () -> {
                    List<String> out = new ArrayList<>();
                    for (World w : Bukkit.getWorlds()) out.add(w.getName());
                    return out;
                });
            case "gameMode":
                return Brigadier.type(reader -> {
                    String text = Brigadier.token(reader);
                    try {
                        return GameMode.valueOf(text.toUpperCase(Locale.ROOT));
                    } catch (IllegalArgumentException e) {
                        throw BAD.create("game mode " + text);
                    }
                }, () -> List.of("survival", "creative", "adventure", "spectator"));
            case "uuid":
                return Brigadier.type(reader -> {
                    String text = Brigadier.token(reader);
                    try {
                        return UUID.fromString(text);
                    } catch (IllegalArgumentException e) {
                        throw BAD.create("UUID " + text);
                    }
                }, null);
            case "namespacedKey":
                return Brigadier.type(reader -> {
                    NamespacedKey key = NamespacedKey.fromString(Brigadier.token(reader).toLowerCase(Locale.ROOT));
                    if (key == null) throw BAD.create("key");
                    return key;
                }, null);
            case "key":
                return Brigadier.type(reader -> Key.key(Brigadier.token(reader).toLowerCase(Locale.ROOT)), null);
            case "component":
                return Brigadier.type(reader -> {
                    String rest = reader.getRemaining();
                    reader.setCursor(reader.getTotalLength());
                    return GsonComponentSerializer.gson().deserialize(rest);
                }, null);
            case "namedColor":
                return Brigadier.type(reader -> {
                    String text = Brigadier.token(reader);
                    NamedTextColor c = NamedTextColor.NAMES.value(text.toLowerCase(Locale.ROOT));
                    if (c == null) throw BAD.create("color " + text);
                    return c;
                }, () -> NamedTextColor.NAMES.keys());
            case "hexColor":
                return Brigadier.type(reader -> {
                    String text = Brigadier.token(reader);
                    TextColor c = text.startsWith("#") ? TextColor.fromHexString(text) : NamedTextColor.NAMES.value(text.toLowerCase(Locale.ROOT));
                    if (c == null) throw BAD.create("color " + text);
                    return c;
                }, null);
            case "time": {
                int min = args != null && args.length > 0 ? (Integer) args[0] : 0;
                return Brigadier.type(reader -> {
                    int t = ticks(Brigadier.token(reader));
                    if (t < min) throw BAD.create("time (at least " + min + " ticks)");
                    return t;
                }, () -> List.of("1d", "1s", "20t"));
            }
            case "integerRange": {
                Class<?> type = cls("io.papermc.paper.command.brigadier.argument.range.IntegerRangeProvider");
                return Brigadier.type(reader -> {
                    Range<Integer> range = range(Brigadier.token(reader), Integer::parseInt);
                    return Proxy.newProxyInstance(ArgumentTypes.class.getClassLoader(), new Class<?>[]{type}, (p, m, a) -> m.getName().equals("range") ? range : null);
                }, null);
            }
            case "doubleRange": {
                Class<?> type = cls("io.papermc.paper.command.brigadier.argument.range.DoubleRangeProvider");
                return Brigadier.type(reader -> {
                    Range<Double> range = range(Brigadier.token(reader), Double::parseDouble);
                    return Proxy.newProxyInstance(ArgumentTypes.class.getClassLoader(), new Class<?>[]{type}, (p, m, a) -> m.getName().equals("range") ? range : null);
                }, null);
            }
            case "itemStack":
                return Brigadier.type(reader -> {
                    String text = Brigadier.token(reader);
                    String id = text.contains("[") ? text.substring(0, text.indexOf('[')) : text;
                    Material m = Material.matchMaterial(id);
                    if (m == null || !m.isItem()) throw BAD.create("item " + text);
                    return Items.create(m, 1);
                }, null);
            case "heightMap":
                return enumType(org.bukkit.HeightMap.class);
            case "scoreboardDisplaySlot":
                return enumType(org.bukkit.scoreboard.DisplaySlot.class);
            case "templateMirror":
                return enumType(org.bukkit.block.structure.Mirror.class);
            case "templateRotation":
                return enumType(org.bukkit.block.structure.StructureRotation.class);
            case "entityAnchor":
                return enumType(io.papermc.paper.entity.LookAnchor.class);
            case "resource": {
                Object key = args[0];
                return Brigadier.type(reader -> {
                    NamespacedKey k = NamespacedKey.fromString(Brigadier.token(reader).toLowerCase(Locale.ROOT));
                    Object value = k == null ? null : new Registries().getRegistry((io.papermc.paper.registry.RegistryKey) key).get(k);
                    if (value == null) throw BAD.create("resource");
                    return value;
                }, null);
            }
            case "resourceKey": {
                Object key = args[0];
                return Brigadier.type(reader -> io.papermc.paper.registry.TypedKey.create((io.papermc.paper.registry.RegistryKey) key, Key.key(Brigadier.token(reader).toLowerCase(Locale.ROOT))), null);
            }
            default:
                break;
        }
        NativeHandler.missing("ArgumentTypes." + name);
        // Read as one word so the command still parses; a plugin asking for the typed value fails clearly.
        return com.mojang.brigadier.arguments.StringArgumentType.word();
    }

    private static <T extends Comparable<T>> Range<T> range(String text, Function<String, T> parse) throws CommandSyntaxException {
        try {
            int dots = text.indexOf("..");
            if (dots < 0) return Range.singleton(parse.apply(text));
            String low = text.substring(0, dots), high = text.substring(dots + 2);
            if (low.isEmpty()) return Range.atMost(parse.apply(high));
            if (high.isEmpty()) return Range.atLeast(parse.apply(low));
            return Range.closed(parse.apply(low), parse.apply(high));
        } catch (RuntimeException e) {
            throw BAD.create("range " + text);
        }
    }

    private static <E extends Enum<E>> ArgumentType<E> enumType(Class<E> type) {
        return Brigadier.type(reader -> {
            String text = Brigadier.token(reader);
            Object v = Conv.enumOf(type, text);
            if (v == null) throw BAD.create(type.getSimpleName() + " " + text);
            return type.cast(v);
        }, () -> {
            List<String> out = new ArrayList<>();
            for (E e : type.getEnumConstants()) out.add(e.name().toLowerCase(Locale.ROOT));
            return out;
        });
    }

    /** A checked Brigadier error out of a resolver lambda (Paper's resolvers declare it). */
    @SuppressWarnings("unchecked")
    static <T extends Throwable> RuntimeException sneaky(Throwable t) throws T {
        throw (T) t;
    }
}
