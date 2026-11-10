package io.zorowarrior.zwrs;

import java.io.Console;
import java.io.IOException;
import java.io.InputStream;
import java.io.InputStreamReader;
import java.io.Reader;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.time.Instant;
import java.time.ZoneOffset;
import java.time.format.DateTimeFormatter;
import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.concurrent.CountDownLatch;
import java.util.concurrent.TimeUnit;
import java.util.concurrent.atomic.AtomicBoolean;

import io.zorowarrior.zwrs.plugin.JavaPluginHost;
import io.zorowarrior.zwrs.plugin.PluginManager;

/**
 * Entry point of {@code java -jar ZOROWARRIOR-Rust-Server.jar}.
 *
 * <p>Responsibilities, in order:</p>
 * <ol>
 *   <li>parse launcher arguments without letting anything unsafe through</li>
 *   <li>prepare the server directory layout ({@code config/ data/ plugins/ logs/ native/})</li>
 *   <li>load the Rust engine through JNI and verify the bridge version</li>
 *   <li>optionally run self-tests ({@code --selftest}) and exit with a real status</li>
 *   <li>start the engine, install a shutdown hook, and forward console commands</li>
 *   <li>discover Java plugins with the Bukkit-compatible host</li>
 * </ol>
 *
 * <p>The launcher never becomes the game server: it loads, starts, observes and
 * stops the Rust engine.</p>
 */
public final class Launcher {

    /** Version of this launcher; must match the engine release. */
    public static final String VERSION = "0.1.0";

    private final LauncherOptions args;
    private final AtomicBoolean stopping = new AtomicBoolean(false);
    private CountDownLatch stopped = new CountDownLatch(1);

    private Launcher(LauncherOptions args) {
        this.args = args;
        instance = this;
    }

    public static void main(String[] argv) {
        Log.setDebug(System.getenv("ZWRS_DEBUG") != null);
        LauncherOptions args = LauncherOptions.parse(argv);
        if (args.help()) {
            printUsage();
            return;
        }
        if (args.diagnose()) {
            NativeLibraryLoader.diagnostics().forEach(System.out::println);
            return;
        }
        if (!args.selfTest() && !args.statusOnly()) {
            installSignalHandlers();
        }
        Launcher launcher = new Launcher(args);
        int status = launcher.run();
        if (status != 0) {
            System.exit(status);
        }
    }

    private int run() {
        printHeader();
        Path directory;
        try {
            directory = args.directory().toAbsolutePath().normalize();
            LauncherSupport.prepareLayout(directory);
            Log.info("Server directory: " + directory);
            Log.debug("options: " + args.describe());
        } catch (IOException error) {
            Log.error("Could not prepare the server directory: " + error.getMessage());
            return 1;
        }

        try {
            NativeBridge.load(args.nativePath());
        } catch (IllegalStateException | UnsatisfiedLinkError error) {
            Log.error(error.getMessage());
            Log.error("The Rust engine is required to run ZOROWARRIOR Rust Server. "
                    + "Run `java -jar ZOROWARRIOR-Rust-Server.jar --diagnose` for details.");
            return 2;
        }

        try {
            Map<String, Object> build = NativeBridge.buildInfo();
            Log.info("engine: " + Json.string(build, "product", "ZWRS") + " "
                    + Json.string(build, "version", "?") + " ("
                    + Json.string(build, "platform", "?") + ", "
                    + Json.string(build, "profile", "?") + " build, JNI v"
                    + Json.number(build, "jni_version", 0) + ")");
            Log.info("Minecraft protocol support: " + Json.number(build, "min_protocol", 0)
                    + "-" + Json.number(build, "max_protocol", 0));
        } catch (RuntimeException error) {
            Log.warn("Could not read engine build info: " + error.getMessage());
        }

        if (args.selfTest()) {
            return runSelfTest(directory);
        }

        return runServer(directory);
    }

    private int runServer(Path directory) {
        Map<String, Object> startInfo;
        try {
            startInfo = NativeBridge.start(directory.toString(), args.portSet() ? args.port() : 0, args.onlineMode());
        } catch (RuntimeException error) {
            Log.error("The engine failed to start: " + error.getMessage());
            return 3;
        }
        Log.info("Rust engine is running and owns the Minecraft server on "
                + Json.string(startInfo, "address", "?"));
        reportCapabilities(Json.object(startInfo, "capabilities"));

        Path logsDir = directory.resolve("logs");
        tailEngineLog(logsDir);

        try {
            PluginManager manager = new PluginManager(directory.resolve("plugins"), new JavaPluginHost());
            manager.discover().forEach(Log::info);
            if (!manager.problems().isEmpty()) {
                manager.problems().forEach(problem -> Log.warn("plugin: " + problem));
            }
        } catch (RuntimeException error) {
            Log.warn("Plugin discovery failed: " + error.getMessage());
        }

        Runtime.getRuntime().addShutdownHook(new Thread(this::shutdown, "zwrs-shutdown"));
        Log.info("Type a command (/help) or press Ctrl+C to stop.");

        if (args.statusOnly()) {
            Map<String, Object> state = NativeBridge.state();
            Log.info("status: " + state);
            shutdown();
            return 0;
        }

        consoleLoop();
        shutdown();
        return 0;
    }

    private int runSelfTest(Path directory) {
        List<String> failures = new ArrayList<>();
        int checks = 0;
        try {
            Map<String, Object> info = NativeBridge.buildInfo();
            checks++;
            if (!Json.string(info, "product", "").toLowerCase().contains("zorowarrior")) {
                failures.add("build info does not identify the product");
            }
            if (Json.number(info, "min_protocol", 0) <= 0) {
                failures.add("engine reports no supported protocol versions");
            }

            // A real Java -> Rust -> Java round trip with a checksum the Java side
            // recomputes independently.
            String username = "SelfTestPlayer";
            Map<String, Object> selfTest = NativeBridge.selfTest(username);
            checks++;
            if (!username.equals(Json.string(selfTest, "echo", null))) {
                failures.add("engine did not echo the self-test string");
            }
            String expected = OfflineUuid.forUsername(username).toString();
            String actual = Json.string(selfTest, "offline_uuid", "");
            checks++;
            if (!expected.equals(actual)) {
                failures.add("offline UUID mismatch: Java computed " + expected
                        + " but the Rust engine returned " + actual);
            }

            Map<String, Object> start = NativeBridge.start(directory.toString(), 0, false);
            checks++;
            String address = Json.string(start, "address", "");
            if (!address.contains(":")) {
                failures.add("engine did not report a bound address");
            }
            Log.info("self-test: engine bound to " + address);

            NativeBridge.CommandResult status = NativeBridge.command("/status");
            checks++;
            if (status.lines().isEmpty() || !status.recognised()) {
                failures.add("/status returned nothing through the bridge");
            }
            status.lines().forEach(line -> Log.info("self-test status: " + line));

            Map<String, Object> state = NativeBridge.state();
            checks++;
            if (!Json.bool(state, "running", false)) {
                failures.add("engine reports it is not running");
            }
            Map<String, Object> stop = NativeBridge.stop();
            checks++;
            if (!Json.bool(stop, "stopped", false)) {
                failures.add("engine did not stop cleanly");
            }
            // Stopping twice must be safe and idempotent.
            Map<String, Object> stopAgain = NativeBridge.stop();
            checks++;
            if (Json.bool(stopAgain, "stopped", true)) {
                failures.add("second stop reported a stop that did not happen");
            }
            Path players = directory.resolve("data").resolve("players").resolve("players.json");
            checks++;
            if (!Files.exists(players)) {
                failures.add("player database was not created at " + players);
            }
        } catch (RuntimeException error) {
            failures.add("exception during self-test: " + error);
        }

        if (failures.isEmpty()) {
            Log.info("SELF-TEST PASSED: " + checks + " checks, Java -> Rust -> Java bridge verified");
            return 0;
        }
        for (String failure : failures) {
            Log.error("self-test failure: " + failure);
        }
        Log.error("SELF-TEST FAILED: " + failures.size() + " of " + checks + " checks failed");
        return 4;
    }

    private void reportCapabilities(Map<String, Object> capabilities) {
        for (Map.Entry<String, Object> entry : capabilities.entrySet()) {
            Log.info("capability " + entry.getKey() + ": " + entry.getValue());
        }
    }

    private void tailEngineLog(Path logsDir) {
        // The engine writes to this process's stdout through its own logger; the
        // launcher keeps a file copy for operators who only look at logs/.
        try {
            Path latest = logsDir.resolve("latest.log");
            if (!Files.exists(latest)) {
                Files.createDirectories(logsDir);
                Files.writeString(latest, "");
            }
            Log.setSink((level, message) -> {
                String stamp = DateTimeFormatter.ofPattern("yyyy-MM-dd HH:mm:ss")
                        .withZone(ZoneOffset.UTC).format(Instant.now());
                try {
                    Files.writeString(latest, stamp + " [" + level + "] " + message + System.lineSeparator(),
                            StandardCharsets.UTF_8,
                            java.nio.file.StandardOpenOption.APPEND);
                } catch (IOException ignored) {
                    // Logging must never break the server.
                }
            });
        } catch (IOException error) {
            Log.debug("could not open logs/latest.log: " + error.getMessage());
        }
    }

    private void consoleLoop() {
        Console console = System.console();
        try {
            Reader reader = console != null
                    ? console.reader()
                    : new InputStreamReader(openStdin(), StandardCharsets.UTF_8);
            java.io.BufferedReader buffered = new java.io.BufferedReader(reader);
            String line;
            while (!stopping.get() && (line = buffered.readLine()) != null) {
                String trimmed = line.trim();
                if (trimmed.isEmpty()) {
                    continue;
                }
                try {
                    NativeBridge.CommandResult result = NativeBridge.command(trimmed);
                    result.lines().forEach(System.out::println);
                    if (!result.recognised()) {
                        System.out.println("(unknown command: try /help)");
                    }
                    if (!result.keepRunning()) {
                        break;
                    }
                } catch (RuntimeException error) {
                    Log.error("command failed: " + error.getMessage());
                }
            }
        } catch (IOException error) {
            Log.warn("Console input closed: " + error.getMessage());
        }
    }

    private static InputStream openStdin() {
        return System.in;
    }

    /**
     * Installs handlers for SIGINT/SIGTERM so a container stop or Ctrl+C closes
     * sessions and saves player data before the JVM exits. Falls back to the JVM
     * shutdown hook where {@code sun.misc.Signal} is unavailable (Android).
     */
    private static void installSignalHandlers() {
        try {
            Class<?> signalClass = Class.forName("sun.misc.Signal");
            Class<?> handlerClass = Class.forName("sun.misc.SignalHandler");
            Object handler = java.lang.reflect.Proxy.newProxyInstance(
                    Launcher.class.getClassLoader(),
                    new Class<?>[] {handlerClass},
                    (proxy, method, methodArgs) -> {
                        if ("handle".equals(method.getName())) {
                            currentInstance().shutdown();
                        }
                        return null;
                    });
            for (String name : new String[] {"INT", "TERM"}) {
                Object signal = signalClass.getConstructor(String.class).newInstance(name);
                signalClass.getMethod("handle", signalClass, handlerClass).invoke(null, signal, handler);
            }
            Log.debug("installed SIGINT/SIGTERM handlers");
        } catch (ReflectiveOperationException | RuntimeException error) {
            Log.debug("signal handlers unavailable (" + error.getClass().getSimpleName()
                    + "); relying on the JVM shutdown hook");
        } catch (LinkageError error) {
            Log.debug("sun.misc.Signal is not available on this platform; relying on the JVM shutdown hook");
        }
    }

    private static volatile Launcher instance;

    private static Launcher currentInstance() {
        Launcher current = instance;
        if (current == null) {
            throw new IllegalStateException("launcher not initialised");
        }
        return current;
    }

    private void shutdown() {
        if (!stopping.compareAndSet(false, true)) {
            return;
        }
        Log.info("Stopping the Rust engine...");
        try {
            Map<String, Object> result = NativeBridge.stop();
            if (Json.bool(result, "stopped", false)) {
                Log.info("Engine stopped; closed " + Json.number(result, "online_players_closed", 0)
                        + " open session(s).");
            } else {
                Log.info("Engine was not running.");
            }
        } catch (RuntimeException error) {
            Log.error("Error while stopping the engine: " + error.getMessage());
        } finally {
            stopped.countDown();
        }
    }

    /** Waits for shutdown to complete (used by tests). */
    public boolean awaitStop(long seconds) throws InterruptedException {
        return stopped.await(seconds, TimeUnit.SECONDS);
    }

    private void printHeader() {
        System.out.println();
        System.out.println("  ZOROWARRIOR Rust Server " + VERSION);
        System.out.println("  Rust engine + Java launcher");
        System.out.println();
    }

    private static void printUsage() {
        System.out.println("""
                ZOROWARRIOR Rust Server launcher

                Usage: java -jar ZOROWARRIOR-Rust-Server.jar [options]

                Options:
                  --dir=<path>       server directory (default: current directory)
                  --port=<1-65535>   override the configured port
                  --online-mode      require Mojang/Microsoft authentication
                  --offline-mode     derive UUIDs from usernames (unauthenticated)
                  --native=<path>    load a specific native engine library
                  --selftest         run the Java <-> Rust bridge self-test and exit
                  --status           print engine status, then stop it and exit
                  --diagnose         print platform/JNI diagnostics and exit
                  --debug            verbose launcher logging
                  --help             this message

                The Rust engine runs the Minecraft server. The JAR only loads,
                starts, observes and stops it.
                """);
    }

}
