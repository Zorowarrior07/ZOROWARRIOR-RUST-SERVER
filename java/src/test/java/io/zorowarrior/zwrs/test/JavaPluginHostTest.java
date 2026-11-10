package io.zorowarrior.zwrs.test;

import static io.zorowarrior.zwrs.test.TestSupport.contains;
import static io.zorowarrior.zwrs.test.TestSupport.equal;

import java.io.ByteArrayOutputStream;
import java.io.IOException;
import java.io.PrintStream;
import java.nio.file.Path;
import java.util.List;

import io.zorowarrior.zwrs.plugin.JavaPluginHost;

/** Discovery, validation and honesty checks for the Java plugin host. */
public final class JavaPluginHostTest {

    private JavaPluginHostTest() {
    }

    private static JavaPluginHost host(Path dir) {
        JavaPluginHost host = new JavaPluginHost();
        host.setOutput(new PrintStream(new ByteArrayOutputStream()));
        return host;
    }

    public static void testDiscoversAndValidatesACompatiblePlugin() throws IOException {
        Path dir = TestSupport.tempDir("host");
        TestSupport.writeJar(dir, "Good.jar", TestSupport.entries(
                "plugin.yml", TestSupport.pluginYml("GoodPlugin", "1.0.0", "com.example.GoodPlugin", "1.20"),
                "com/example/GoodPlugin.class", "") ) ;
        // Replace the empty class entry with a real class file.
        TestSupport.writeJar(dir, "Good.jar", java.util.Map.of(
                "plugin.yml", TestSupport.pluginYml("GoodPlugin", "1.0.0", "com.example.GoodPlugin", "1.20")
                        .getBytes(java.nio.charset.StandardCharsets.UTF_8),
                "com/example/GoodPlugin.class", TestSupport.classFile(61)));

        JavaPluginHost host = host(dir);
        List<String> lines = host.discover(dir);
        equal(1, host.statuses().size(), "one plugin discovered");
        JavaPluginHost.PluginStatus status = host.statuses().get(0);
        equal("GoodPlugin", status.name(), "name");
        equal(JavaPluginHost.State.METADATA_COMPATIBLE, status.state(), "state");
        equal(64, status.sha256().length(), "sha256 length");
        TestSupport.check(status.notes().stream().anyMatch(n -> n.contains("not executed")),
                "notes must state that the plugin is not executed: " + status.notes());
        TestSupport.check(status.notes().stream().anyMatch(n -> n.contains("Java 17")),
                "class file version must be reported: " + status.notes());
        contains(String.join("\n", lines), "no plugin code has been executed", "host status line");
    }

    public static void testPluginForNewerJavaIsIncompatible() throws IOException {
        Path dir = TestSupport.tempDir("hostfuture");
        TestSupport.writeJar(dir, "Future.jar", java.util.Map.of(
                "plugin.yml", TestSupport.pluginYml("FuturePlugin", "1.0", "com.example.Future", "1.20")
                        .getBytes(java.nio.charset.StandardCharsets.UTF_8),
                "com/example/Future.class", TestSupport.classFile(69)));
        JavaPluginHost host = host(dir);
        host.discover(dir);
        equal(JavaPluginHost.State.INCOMPATIBLE, host.statuses().get(0).state(), "state");
        TestSupport.check(host.statuses().get(0).notes().stream().anyMatch(n -> n.contains("newer Java")),
                "explains the runtime mismatch");
    }

    public static void testMissingMainClassIsIncompatible() throws IOException {
        Path dir = TestSupport.tempDir("hostnomain");
        TestSupport.writeJar(dir, "NoClass.jar", TestSupport.entries(
                "plugin.yml", TestSupport.pluginYml("NoClass", "1.0", "com.example.Missing", "1.20")));
        JavaPluginHost host = host(dir);
        host.discover(dir);
        equal(JavaPluginHost.State.INCOMPATIBLE, host.statuses().get(0).state(), "state");
    }

    public static void testBrokenJarIsReportedWithoutStoppingTheScan() throws IOException {
        Path dir = TestSupport.tempDir("hostbroken");
        TestSupport.writeJar(dir, "Broken.jar", TestSupport.entries("plugin.yml", "name: [unclosed\n"));
        TestSupport.writeJar(dir, "Valid.jar", java.util.Map.of(
                "plugin.yml", TestSupport.pluginYml("Valid", "1.0", "com.example.Valid", "1.20")
                        .getBytes(java.nio.charset.StandardCharsets.UTF_8),
                "com/example/Valid.class", TestSupport.classFile(61)));
        JavaPluginHost host = host(dir);
        host.discover(dir);
        equal(1, host.statuses().size(), "the valid plugin is still reported");
        equal(1, host.problems().size(), "the broken plugin is reported as a problem");
    }

    public static void testUnsupportedApisAreStated() {
        TestSupport.check(JavaPluginHost.unsupportedApis().stream().anyMatch(a -> a.contains("org.bukkit")),
                "the missing Bukkit surface must be listed");
        TestSupport.check(!JavaPluginHost.supportedApis().isEmpty(), "supported surface is documented");
        TestSupport.check(JavaPluginHost.supportedApis().stream().noneMatch(a -> a.contains("onEnable")),
                "onEnable must not be claimed as supported");
    }

    public static void testEmptyDirectoryIsNotAnError() throws IOException {
        Path dir = TestSupport.tempDir("hostempty");
        JavaPluginHost host = host(dir);
        List<String> lines = host.discover(dir);
        equal(0, host.statuses().size(), "no plugins");
        contains(String.join("\n", lines), "empty", "reports an empty directory");
    }
}
