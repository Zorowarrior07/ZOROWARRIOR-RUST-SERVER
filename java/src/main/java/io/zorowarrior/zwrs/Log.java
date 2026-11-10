package io.zorowarrior.zwrs;

import java.io.PrintStream;
import java.time.LocalDateTime;
import java.time.format.DateTimeFormatter;

/**
 * Launcher logging.
 *
 * <p>Keeps the console format close to a normal Minecraft server so existing
 * tooling and operators recognise it, and forwards lines to a {@link LogSink}
 * when one is installed (used by the test harness and by the plugin host).</p>
 */
public final class Log {

    /** Receives structured log lines. */
    public interface LogSink {
        void line(String level, String message);
    }

    private static final DateTimeFormatter TIME = DateTimeFormatter.ofPattern("HH:mm:ss");
    private static volatile LogSink sink;
    private static volatile boolean debugEnabled;
    private static volatile PrintStream out = System.out;

    private Log() {
    }

    public static void setSink(LogSink newSink) {
        sink = newSink;
    }

    public static void setDebug(boolean enabled) {
        debugEnabled = enabled;
    }

    public static boolean isDebug() {
        return debugEnabled;
    }

    /** Redirects output (used by tests to keep stdout clean). */
    public static void setStream(PrintStream stream) {
        out = stream;
    }

    public static void info(String message) {
        emit("INFO", message);
    }

    public static void warn(String message) {
        emit("WARN", message);
    }

    public static void error(String message) {
        emit("ERROR", message);
    }

    public static void debug(String message) {
        if (debugEnabled) {
            emit("DEBUG", message);
        }
    }

    private static void emit(String level, String message) {
        String stamp = LocalDateTime.now().format(TIME);
        PrintStream stream = out;
        if (stream != null) {
            stream.println("[" + stamp + "] [" + level + "] " + message);
            stream.flush();
        }
        LogSink current = sink;
        if (current != null) {
            current.line(level, message);
        }
    }
}
