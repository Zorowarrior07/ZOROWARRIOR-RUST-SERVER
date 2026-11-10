package io.zorowarrior.zwrs.test;

import java.nio.file.Files;
import java.nio.file.Path;
import java.util.List;
import java.util.Map;

import io.zorowarrior.zwrs.plugin.PluginRuntime;
import io.zorowarrior.zwrs.plugin.ServerImpl;
import io.zorowarrior.zwrs.test.fixture.BrokenPlugin;
import io.zorowarrior.zwrs.test.fixture.GreetingPlugin;

/**
 * Tests that Java plugins are really loaded, enabled, dispatched to and disabled.
 *
 * <p>The plugin under test is compiled from {@code fixture/} by the test harness
 * and packaged into a real JAR, so this exercises the same code path an operator
 * uses: ZIP reading, {@code plugin.yml} parsing, class loading from the JAR,
 * instantiation, {@code onEnable()}, reflection-based event dispatch and
 * {@code onDisable()}. A class-path plugin would not prove the class loader works.</p>
 */
public final class PluginRuntimeTest {

    private PluginRuntimeTest() {
    }

    /** Builds a plugin JAR containing a real class, loaded from this class path. */
    static Path writePluginJar(Path dir, String jarName, String pluginName, String version,
            String mainClass, String apiVersion, Map<String, String> extraEntries)
            throws Exception {
        Files.createDirectories(dir);
        Map<String, byte[]> entries = new java.util.LinkedHashMap<>();
        entries.put("plugin.yml", TestSupport.pluginYml(pluginName, version, mainClass, apiVersion)
                .getBytes(java.nio.charset.StandardCharsets.UTF_8));
        // Real plugin JARs ship every class they contain, including anonymous and
        // named inner classes. The fixture must do the same or it is not a
        // realistic plugin (and the loader correctly refuses a half-shipped class).
        byte[] classBytes = readClassBytes(mainClass);
        entries.put(mainClass.replace('.', '/') + ".class", classBytes);
        addInnerClasses(entries, mainClass);
        if (extraEntries != null) {
            for (Map.Entry<String, String> entry : extraEntries.entrySet()) {
                entries.put(entry.getKey(),
                        entry.getValue().getBytes(java.nio.charset.StandardCharsets.UTF_8));
            }
        }
        return TestSupport.writeJar(dir, jarName, entries);
    }

    /** Adds every {@code Outer$Inner.class} resource next to the main class. */
    static void addInnerClasses(Map<String, byte[]> entries, String mainClass) throws Exception {
        java.net.URL location = PluginRuntimeTest.class.getClassLoader()
                .getResource(mainClass.replace('.', '/') + ".class");
        if (location == null || !"file".equals(location.getProtocol())) {
            return;
        }
        Path folder = Path.of(location.toURI()).getParent();
        String simpleName = mainClass.substring(mainClass.lastIndexOf('.') + 1);
        try (var stream = Files.list(folder)) {
            for (Path file : stream.toList()) {
                String fileName = file.getFileName().toString();
                if (fileName.startsWith(simpleName + "$") && fileName.endsWith(".class")) {
                    String binary = mainClass.substring(0, mainClass.lastIndexOf('.') + 1)
                            + fileName.substring(0, fileName.length() - ".class".length());
                    entries.put(binary.replace('.', '/') + ".class", Files.readAllBytes(file));
                }
            }
        }
    }

    static byte[] readClassBytes(String className) throws Exception {
        String resource = className.replace('.', '/') + ".class";
        try (java.io.InputStream in = PluginRuntimeTest.class.getClassLoader()
                .getResourceAsStream(resource)) {
            if (in == null) {
                throw new IllegalStateException("test class not compiled: " + resource);
            }
            return in.readAllBytes();
        }
    }

    static ServerImpl server(PluginRuntime runtime) {
        return new ServerImpl(runtime, line -> List.of("ran: " + line),
                "Test MOTD", 20, true, "0.1.0-test", "1.21.8");
    }

    public static void testLoadsEnablesAndDisablesARealPlugin() throws Exception {
        io.zorowarrior.zwrs.test.fixture.Recorder.reset();
        Path dir = Files.createTempDirectory("zwrs-plugins");
        try {
            writePluginJar(dir, "Greeting.jar", "Greeting", "1.2.3",
                    GreetingPlugin.class.getName(), "1.20", null);
            PluginRuntime runtime = new PluginRuntime();
            ServerImpl server = server(runtime);
            org.bukkit.Bukkit.setServer(server);
            runtime.loadAll(dir, server);

            TestSupport.equal(1, runtime.plugins().size(), "one plugin discovered");
            PluginRuntime.LoadedPlugin plugin = runtime.plugins().get(0);
            TestSupport.equal(PluginRuntime.State.ENABLED, plugin.state(),
                    "plugin reached ENABLED: " + plugin.describe());
            TestSupport.check(io.zorowarrior.zwrs.test.fixture.Recorder.enabled, "onEnable() really ran");
            TestSupport.check(io.zorowarrior.zwrs.test.fixture.Recorder.EVENTS.contains("enable:Greeting"),
                    "the plugin saw its own name: " + io.zorowarrior.zwrs.test.fixture.Recorder.EVENTS);
            TestSupport.check(runtime.getRegisteredCommands().contains("greet"),
                    "the plugin's command is registered: " + runtime.getRegisteredCommands());
            TestSupport.equal("Greeting", runtime.getCommandOwner("greet").getName(),
                    "the command is owned by the plugin");
            // The instance is loaded from the JAR by the plugin class loader, so its
            // Class is a different copy from the test's: compare names and loaders.
            TestSupport.equal(GreetingPlugin.class.getName(), plugin.instance().getClass().getName(),
                    "the instance is the plugin class named in plugin.yml");
            TestSupport.check(plugin.instance().getClass() != GreetingPlugin.class,
                    "the plugin class really came from the JAR, not the test class path");
            TestSupport.check(plugin.instance().getClass().getClassLoader()
                            .getClass().getName().contains("PluginClassLoader"),
                    "the plugin was loaded by the isolated plugin class loader");
            TestSupport.check(plugin.instance().getDataFolder().isDirectory(),
                    "the plugin data folder was created");
            TestSupport.check(plugin.instance().getDescription().getVersion().equals("1.2.3"),
                    "the descriptor reached the plugin");

            runtime.disableAll();
            TestSupport.check(io.zorowarrior.zwrs.test.fixture.Recorder.disabled, "onDisable() ran");
            TestSupport.equal(PluginRuntime.State.DISABLED, plugin.state(), "state is DISABLED");
            TestSupport.equal(0, runtime.enabledCount(), "no plugin is enabled after shutdown");
        } finally {
            TestSupport.deleteTree(dir);
        }
    }

    public static void testDeliversPlayerEventsToPlugins() throws Exception {
        io.zorowarrior.zwrs.test.fixture.Recorder.reset();
        Path dir = Files.createTempDirectory("zwrs-plugins-events");
        try {
            writePluginJar(dir, "Greeting.jar", "Greeting", "1.0.0",
                    GreetingPlugin.class.getName(), "1.20", null);
            PluginRuntime runtime = new PluginRuntime();
            ServerImpl server = server(runtime);
            org.bukkit.Bukkit.setServer(server);
            runtime.loadAll(dir, server);
            TestSupport.check(runtime.plugins().get(0).isEnabled(), "plugin enabled");

            // These are the documents the Rust engine really sends (see
            // crates/zwrs-engine/src/events.rs), fed through the same hook method
            // the JVM calls from the native thread.
            String join = "{\"uuid\":\"b50ad385-829d-3141-a216-7e7d7539ba7f\","
                    + "\"username\":\"Notch\",\"protocol\":772,\"address\":\"127.0.0.1:50000\","
                    + "\"timestamp\":\"2026-11-10T10:00:00Z\",\"online_count\":1,"
                    + "\"session_seconds\":0}";
            io.zorowarrior.zwrs.plugin.EngineHooks.install(runtime, server);
            io.zorowarrior.zwrs.plugin.EngineHooks.onPlayerJoin(join);

            TestSupport.check(io.zorowarrior.zwrs.test.fixture.Recorder.EVENTS.stream()
                            .anyMatch(e -> e.equals("join:Notch:b50ad385-829d-3141-a216-7e7d7539ba7f:772:true")),
                    "the plugin received the join with real identity data: "
                            + io.zorowarrior.zwrs.test.fixture.Recorder.EVENTS);
            TestSupport.equal(1, server.getOnlinePlayerCount(), "the player is tracked as online");
            TestSupport.check(server.getPlayer("Notch") != null, "lookup by name works");

            String quit = join.replace("\"session_seconds\":0", "\"session_seconds\":65");
            io.zorowarrior.zwrs.plugin.EngineHooks.onPlayerQuit(quit);
            TestSupport.check(io.zorowarrior.zwrs.test.fixture.Recorder.EVENTS.contains("quit:Notch:false"),
                    "the quit carried isOnline=false: " + io.zorowarrior.zwrs.test.fixture.Recorder.EVENTS);
            TestSupport.equal(0, server.getOnlinePlayerCount(), "the player is no longer online");

            // A malformed event must be logged and ignored, not thrown into JNI.
            io.zorowarrior.zwrs.plugin.EngineHooks.onPlayerJoin("not json");
            TestSupport.check(io.zorowarrior.zwrs.plugin.EngineHooks.lastError() != null,
                    "the malformed payload was recorded");
        } finally {
            io.zorowarrior.zwrs.plugin.EngineHooks.install(null, null);
            TestSupport.deleteTree(dir);
        }
    }

    public static void testRunsPluginCommandsFromTheConsole() throws Exception {
        io.zorowarrior.zwrs.test.fixture.Recorder.reset();
        Path dir = Files.createTempDirectory("zwrs-plugins-commands");
        try {
            writePluginJar(dir, "Greeting.jar", "Greeting", "1.0.0",
                    GreetingPlugin.class.getName(), "1.20", null);
            PluginRuntime runtime = new PluginRuntime();
            ServerImpl server = server(runtime);
            org.bukkit.Bukkit.setServer(server);
            runtime.loadAll(dir, server);

            // The engine asks through the same JSON hook the JNI layer uses.
            io.zorowarrior.zwrs.plugin.EngineHooks.install(runtime, server);
            String reply = io.zorowarrior.zwrs.plugin.EngineHooks.onCommand(
                    "{\"command\":\"greet\",\"args\":[\"Arena\"]}");
            TestSupport.check(reply != null, "the plugin claimed /greet");
            TestSupport.contains(reply, "Hello, Arena!", "the command output is returned");
            TestSupport.check(io.zorowarrior.zwrs.test.fixture.Recorder.EVENTS.contains("command:greet:Arena"),
                    "the plugin's execute() ran: " + io.zorowarrior.zwrs.test.fixture.Recorder.EVENTS);

            TestSupport.check(io.zorowarrior.zwrs.plugin.EngineHooks.onCommand(
                            "{\"command\":\"notmine\",\"args\":[]}") == null,
                    "an unowned command returns null so the engine reports it as unknown");
        } finally {
            io.zorowarrior.zwrs.plugin.EngineHooks.install(null, null);
            TestSupport.deleteTree(dir);
        }
    }

    public static void testContainsAFailedPluginWithoutStoppingTheServer() throws Exception {
        io.zorowarrior.zwrs.test.fixture.Recorder.reset();
        Path dir = Files.createTempDirectory("zwrs-plugins-failure");
        try {
            writePluginJar(dir, "a-broken.jar", "Broken", "1.0.0",
                    BrokenPlugin.class.getName(), "1.20", null);
            writePluginJar(dir, "b-good.jar", "Greeting", "1.0.0",
                    GreetingPlugin.class.getName(), "1.20", null);
            PluginRuntime runtime = new PluginRuntime();
            ServerImpl server = server(runtime);
            org.bukkit.Bukkit.setServer(server);
            runtime.loadAll(dir, server);

            TestSupport.equal(2, runtime.plugins().size(), "both plugins were discovered");
            PluginRuntime.LoadedPlugin broken = runtime.plugins().stream()
                    .filter(p -> p.name().equals("Broken")).findFirst().orElseThrow();
            PluginRuntime.LoadedPlugin good = runtime.plugins().stream()
                    .filter(p -> p.name().equals("Greeting")).findFirst().orElseThrow();
            TestSupport.equal(PluginRuntime.State.FAILED, broken.state(),
                    "the broken plugin is FAILED, never ENABLED");
            TestSupport.contains(broken.error(), "intentionally broken",
                    "the real exception message is reported: " + broken.error());
            TestSupport.equal(PluginRuntime.State.ENABLED, good.state(),
                    "the healthy plugin still loaded");
            TestSupport.equal(1, runtime.enabledCount(), "exactly one plugin is enabled");
            TestSupport.check(runtime.problems().stream()
                            .anyMatch(p -> p.contains("Broken")),
                    "the failure is listed in problems");
        } finally {
            TestSupport.deleteTree(dir);
        }
    }

    public static void testRejectsAPluginWithAMissingMainClass() throws Exception {
        Path dir = Files.createTempDirectory("zwrs-plugins-missing");
        try {
            TestSupport.writeJar(dir, "missing.jar", Map.of(
                    "plugin.yml", TestSupport.pluginYml("Missing", "1.0.0",
                            "com.example.NotThere", "1.20")
                            .getBytes(java.nio.charset.StandardCharsets.UTF_8)));
            PluginRuntime runtime = new PluginRuntime();
            runtime.loadAll(dir, server(runtime));
            PluginRuntime.LoadedPlugin plugin = runtime.plugins().get(0);
            TestSupport.equal(PluginRuntime.State.FAILED, plugin.state(),
                    "INCOMPATIBLE/FAILED, never enabled: " + plugin.describe());
            TestSupport.contains(plugin.error(), "not in the JAR",
                    "the error names the missing class: " + plugin.error());
        } finally {
            TestSupport.deleteTree(dir);
        }
    }
}
