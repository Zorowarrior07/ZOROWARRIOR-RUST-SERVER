package io.zorowarrior.zwrs;

import java.nio.file.Path;

/**
 * Parsed launcher command-line options.
 *
 * <p>Parsing is strict: an unknown flag is an error, not a silent default, and
 * every option that needs a value validates it immediately so the user sees the
 * problem before the server directory is touched.</p>
 *
 * @param directory    server directory
 * @param port         port override (0 means "ask the engine/config")
 * @param portSet      whether {@code --port} was given
 * @param onlineMode   authentication mode to force
 * @param onlineModeSet whether a mode flag was given
 * @param nativePath   explicit native library path
 * @param selfTest     run the bridge self-test and exit
 * @param statusOnly   print status, stop the engine and exit
 * @param diagnose     print platform diagnostics and exit
 * @param help         print usage and exit
 */
public record LauncherOptions(
        Path directory,
        int port,
        boolean portSet,
        boolean onlineMode,
        boolean onlineModeSet,
        String nativePath,
        boolean selfTest,
        boolean statusOnly,
        boolean diagnose,
        boolean help) {

    /** Parses {@code argv}. */
    public static LauncherOptions parse(String[] argv) {
        Path directory = Path.of(".");
        int port = 0;
        boolean portSet = false;
        boolean onlineMode = false;
        boolean onlineModeSet = false;
        String nativePath = null;
        boolean selfTest = false;
        boolean statusOnly = false;
        boolean diagnose = false;
        boolean help = false;

        for (String raw : argv) {
            String arg = raw == null ? "" : raw.trim();
            if (arg.isEmpty()) {
                continue;
            }
            String key = arg;
            String value = null;
            int eq = arg.indexOf('=');
            if (eq > 0) {
                key = arg.substring(0, eq);
                value = arg.substring(eq + 1);
            }
            switch (key) {
                case "--help", "-h", "-?" -> help = true;
                case "--dir", "-d" -> directory = Path.of(require(value, key));
                case "--port" -> {
                    String text = require(value, key);
                    int parsed;
                    try {
                        parsed = Integer.parseInt(text);
                    } catch (NumberFormatException error) {
                        throw new IllegalArgumentException("--port must be a number, got `" + text + "`");
                    }
                    if (parsed < 0 || parsed > 65535) {
                        throw new IllegalArgumentException("--port must be 0-65535, got " + parsed);
                    }
                    port = parsed;
                    portSet = true;
                }
                case "--online-mode" -> {
                    onlineMode = true;
                    onlineModeSet = true;
                }
                case "--offline-mode" -> {
                    onlineMode = false;
                    onlineModeSet = true;
                }
                case "--native" -> nativePath = require(value, key);
                case "--selftest" -> selfTest = true;
                case "--status" -> statusOnly = true;
                case "--diagnose" -> diagnose = true;
                case "--debug" -> Log.setDebug(true);
                default -> throw new IllegalArgumentException(
                        "unknown option: " + arg + " (try --help)");
            }
        }
        return new LauncherOptions(directory, port, portSet, onlineMode, onlineModeSet,
                nativePath, selfTest, statusOnly, diagnose, help);
    }

    private static String require(String value, String key) {
        if (value == null || value.isBlank()) {
            throw new IllegalArgumentException(key + " requires a value, e.g. " + key + "=...");
        }
        return value;
    }

    /** One-line summary for the startup log. */
    public String describe() {
        return "dir=" + directory + ", port=" + (portSet ? Integer.toString(port) : "from config")
                + ", mode=" + (onlineModeSet ? (onlineMode ? "online" : "offline") : "from config");
    }
}
