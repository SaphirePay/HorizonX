package horizon.javalayer;

import java.io.ByteArrayOutputStream;
import java.io.OutputStream;
import java.io.PrintStream;
import java.io.PrintWriter;
import java.io.StringWriter;
import java.nio.charset.StandardCharsets;
import java.util.logging.*;

/** Java's logging and System.out/err, into the server's log. */
public final class Logs {
    private Logs() {}

    static final class ToServer extends Handler {
        @Override
        public void publish(LogRecord record) {
            if (record == null || !isLoggable(record)) return;
            String text = record.getMessage();
            try {
                if (record.getParameters() != null && record.getParameters().length > 0) text = java.text.MessageFormat.format(text, record.getParameters());
            } catch (IllegalArgumentException ignored) {}
            String name = record.getLoggerName();
            String prefix = name == null || name.isEmpty() || name.equals("Minecraft") || name.startsWith("horizon.") ? "" : "[" + name + "] ";
            StringBuilder line = new StringBuilder(prefix).append(text == null ? "" : text);
            if (record.getThrown() != null) {
                line.append('\n').append(trace(record.getThrown()));
                Report.error(prefix + text, record.getThrown());
            }
            int level = record.getLevel().intValue();
            Report.log(name, level >= Level.SEVERE.intValue() ? 2 : level >= Level.WARNING.intValue() ? 1 : 0, (text == null ? "" : text));
            Native.log(level >= Level.SEVERE.intValue() ? 2 : level >= Level.WARNING.intValue() ? 1 : 0, line.toString());
        }

        @Override public void flush() {}
        @Override public void close() {}
    }

    static String trace(Throwable t) {
        StringWriter out = new StringWriter();
        t.printStackTrace(new PrintWriter(out));
        return out.toString().stripTrailing();
    }

    /** A stream that hands whole lines to the log. */
    static final class Lines extends OutputStream {
        private final int level;
        private final ByteArrayOutputStream line = new ByteArrayOutputStream();

        Lines(int level) { this.level = level; }

        @Override
        public synchronized void write(int b) {
            if (b == '\n') flushLine();
            else if (b != '\r') line.write(b);
        }

        private void flushLine() {
            String text = line.toString(StandardCharsets.UTF_8);
            line.reset();
            Report.log(Report.ownerNow(), level, text);
            Native.log(level, text);
        }

        @Override
        public synchronized void flush() {
            if (line.size() > 0) flushLine();
        }
    }

    static void install() {
        // Log4j (log4j-core, as on Paper) configured for this console before anything logs through it: not the
        // server jar's log4j2.xml, whose terminal appender isn't here.
        if (System.getProperty("log4j2.configurationFactory") == null) System.setProperty("log4j2.configurationFactory", "horizon.javalayer.logging.LayerLog4j$Factory");
        Logger root = Logger.getLogger("");
        for (Handler h : root.getHandlers()) root.removeHandler(h);
        root.addHandler(new ToServer());
        root.setLevel(Level.INFO);
        System.setOut(new PrintStream(new Lines(0), true, StandardCharsets.UTF_8));
        System.setErr(new PrintStream(new Lines(1), true, StandardCharsets.UTF_8));
    }

    static Logger plugin(String name) {
        Logger logger = Logger.getLogger(name);
        logger.setParent(Logger.getLogger(""));
        return logger;
    }

    static void info(String line) { Native.log(0, "[JavaLayer] " + line); }
    static void warning(String line) { Native.log(1, "[JavaLayer] " + line); }
    static void severe(String line, Throwable t) {
        Native.log(2, line + (t == null ? "" : "\n" + trace(t)));
        Report.error(line, t);
    }
}
