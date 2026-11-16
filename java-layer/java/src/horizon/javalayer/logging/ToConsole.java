package horizon.javalayer.logging;

import java.util.logging.Level;
import java.util.logging.Logger;

/**
 * Where SLF4J and Log4j log lines go: the layer's console (java.util.logging, whose root handler hands them to the
 * server), named as Paper's console names them. Paper prints a logger's name in brackets ("[Reflections] ...",
 * "[com.foo.Bar] ...") except the game's own (net.minecraft., com.mojang., ...), which print bare. The game's own
 * loggers here are the layer's hollow game (registries started for a plugin), not the server: only their warnings show.
 */
final class ToConsole {
    private ToConsole() {}

    private static final String[] GAME = {"net.minecraft.", "com.mojang.", "com.sk89q.", "ru.tehkode.", "Minecraft"};

    static boolean game(String name) {
        if (name == null || name.isEmpty()) return true;
        for (String g : GAME) if (name.startsWith(g)) return true;
        return false;
    }

    static Logger jul(String name) {
        Logger logger = Logger.getLogger(game(name) ? "Minecraft" : name);
        if (logger.getParent() != Logger.getLogger("")) logger.setParent(Logger.getLogger(""));
        return logger;
    }

    /** Whether a line at this level shows (INFO and up, as Paper's console; the game's own from WARN). */
    static boolean shows(String name, Level level) {
        return level.intValue() >= (game(name) ? Level.WARNING : Level.INFO).intValue();
    }

    static void log(String name, Level level, String text, Throwable thrown) {
        if (!shows(name, level)) return;
        Logger logger = jul(name);
        if (thrown != null) logger.log(level, text, thrown);
        else logger.log(level, text);
    }
}
