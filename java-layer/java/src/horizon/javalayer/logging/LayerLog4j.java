package horizon.javalayer.logging;

import org.apache.logging.log4j.Level;
import org.apache.logging.log4j.core.Appender;
import org.apache.logging.log4j.core.LogEvent;
import org.apache.logging.log4j.core.LoggerContext;
import org.apache.logging.log4j.core.appender.AbstractAppender;
import org.apache.logging.log4j.core.config.AbstractConfiguration;
import org.apache.logging.log4j.core.config.Configuration;
import org.apache.logging.log4j.core.config.ConfigurationFactory;
import org.apache.logging.log4j.core.config.ConfigurationSource;
import org.apache.logging.log4j.core.config.LoggerConfig;
import org.apache.logging.log4j.core.config.Property;

import java.net.URI;

/**
 * Log4j as on Paper (log4j-core, so a plugin that adds a filter to the root logger or reads the LoggerContext finds
 * core's own classes: EliteMobs filters its console lines so), configured for the Java layer instead of Paper's
 * terminal: every line to the server's console as Paper's console names it (ToConsole), the game's own loggers from
 * WARN. Set as the configuration factory before anything logs (Logs.install).
 */
public final class LayerLog4j {
    private LayerLog4j() {}

    /** The `log4j2.configurationFactory` system property's class: every LoggerContext gets the layer's configuration. */
    public static final class Factory extends ConfigurationFactory {
        @Override protected String[] getSupportedTypes() { return new String[]{"*"}; }
        @Override public Configuration getConfiguration(LoggerContext context, ConfigurationSource source) { return new Layer(context); }
        @Override public Configuration getConfiguration(LoggerContext context, String name, URI location) { return new Layer(context); }
    }

    static final class Layer extends AbstractConfiguration {
        Layer(LoggerContext context) {
            super(context, ConfigurationSource.NULL_SOURCE);
            setName("HorizonJavaLayer");
        }

        @Override
        protected void doConfigure() {
            Appender console = new ToServer();
            console.start();
            addAppender(console);
            LoggerConfig root = getRootLogger();
            root.getAppenders().keySet().forEach(root::removeAppender);
            root.addAppender(console, null, null);
            root.setLevel(Level.INFO);
            for (String game : new String[]{"net.minecraft", "com.mojang", "Minecraft"}) {
                LoggerConfig quiet = new LoggerConfig(game, Level.WARN, true);
                addLogger(game, quiet);
            }
        }
    }

    /** Each line to the layer's console (java.util.logging's root, which hands it to the server). */
    static final class ToServer extends AbstractAppender {
        ToServer() {
            super("HorizonServer", null, null, true, Property.EMPTY_ARRAY);
        }

        @Override
        public void append(LogEvent event) {
            Level level = event.getLevel();
            java.util.logging.Level jul = level.isMoreSpecificThan(Level.ERROR) ? java.util.logging.Level.SEVERE
                    : level.isMoreSpecificThan(Level.WARN) ? java.util.logging.Level.WARNING
                    : level.isMoreSpecificThan(Level.INFO) ? java.util.logging.Level.INFO
                    : java.util.logging.Level.FINE;
            String text = event.getMessage() == null ? "" : event.getMessage().getFormattedMessage();
            ToConsole.log(event.getLoggerName(), jul, text, event.getThrown());
        }
    }
}
