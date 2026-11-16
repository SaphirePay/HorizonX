package horizon.javalayer.logging;

import org.slf4j.ILoggerFactory;
import org.slf4j.IMarkerFactory;
import org.slf4j.Marker;
import org.slf4j.helpers.BasicMDCAdapter;
import org.slf4j.helpers.BasicMarkerFactory;
import org.slf4j.helpers.LegacyAbstractLogger;
import org.slf4j.helpers.MessageFormatter;
import org.slf4j.spi.MDCAdapter;
import org.slf4j.spi.SLF4JServiceProvider;

import java.util.Map;
import java.util.concurrent.ConcurrentHashMap;
import java.util.logging.Level;

/**
 * SLF4J's provider (Paper's is Log4j's): a plugin's getSLF4JLogger(), its ComponentLogger and any library logging
 * through SLF4J print on the server's console, as on Paper, instead of SLF4J's no-op logger.
 */
public final class Slf4jProvider implements SLF4JServiceProvider {
    private final Map<String, Line> loggers = new ConcurrentHashMap<>();
    private final ILoggerFactory factory = name -> loggers.computeIfAbsent(name, Line::new);
    private final IMarkerFactory markers = new BasicMarkerFactory();
    private final MDCAdapter mdc = new BasicMDCAdapter();

    @Override public ILoggerFactory getLoggerFactory() { return factory; }
    @Override public IMarkerFactory getMarkerFactory() { return markers; }
    @Override public MDCAdapter getMDCAdapter() { return mdc; }
    @Override public String getRequestedApiVersion() { return "2.0.99"; }
    @Override public void initialize() {}

    static final class Line extends LegacyAbstractLogger {
        Line(String name) { this.name = name; }

        private boolean on(Level level) { return ToConsole.shows(name, level); }

        @Override public boolean isTraceEnabled() { return on(Level.FINEST); }
        @Override public boolean isDebugEnabled() { return on(Level.FINE); }
        @Override public boolean isInfoEnabled() { return on(Level.INFO); }
        @Override public boolean isWarnEnabled() { return on(Level.WARNING); }
        @Override public boolean isErrorEnabled() { return on(Level.SEVERE); }

        @Override protected String getFullyQualifiedCallerName() { return null; }

        @Override
        protected void handleNormalizedLoggingCall(org.slf4j.event.Level level, Marker marker, String pattern, Object[] args, Throwable thrown) {
            Level jul = switch (level) {
                case ERROR -> Level.SEVERE;
                case WARN -> Level.WARNING;
                case INFO -> Level.INFO;
                case DEBUG -> Level.FINE;
                case TRACE -> Level.FINEST;
            };
            var formatted = MessageFormatter.basicArrayFormat(pattern, args);
            ToConsole.log(name, jul, formatted, thrown);
        }
    }
}
