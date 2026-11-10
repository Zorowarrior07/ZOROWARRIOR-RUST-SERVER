package io.zorowarrior.zwrs.test;

import static io.zorowarrior.zwrs.test.TestSupport.contains;
import static io.zorowarrior.zwrs.test.TestSupport.equal;
import static io.zorowarrior.zwrs.test.TestSupport.expectThrows;

import java.nio.file.Path;
import java.util.List;

import io.zorowarrior.zwrs.LauncherOptions;

/** Launcher argument parsing and directory preparation. */
public final class LauncherArgsTest {

    private LauncherArgsTest() {
    }

    public static void testParsesAllSupportedOptions() {
        LauncherOptions options = LauncherOptions.parse(new String[] {
                "--dir=/tmp/zwrs", "--port=25566", "--offline-mode", "--native=/tmp/libzwrs_jni.so",
                "--selftest", "--debug"
        });
        equal(Path.of("/tmp/zwrs"), options.directory(), "directory");
        equal(25566, options.port(), "port");
        TestSupport.check(options.portSet(), "port marked set");
        equal(Boolean.FALSE, options.onlineMode(), "offline mode");
        equal("/tmp/libzwrs_jni.so", options.nativePath(), "native path");
        TestSupport.check(options.selfTest(), "self-test flag");
    }

    public static void testDetectsConflictingModes() {
        // Later flags win, which is the least surprising behaviour for a CLI.
        LauncherOptions options = LauncherOptions.parse(new String[] {"--online-mode", "--offline-mode"});
        equal(Boolean.FALSE, options.onlineMode(), "last flag wins");
        TestSupport.check(options.onlineModeSet(), "mode recorded");
    }

    public static void testRejectsUnknownAndIncompleteOptions() {
        expectThrows(IllegalArgumentException.class,
                () -> LauncherOptions.parse(new String[] {"--frobnicate"}), "unknown option");
        expectThrows(IllegalArgumentException.class,
                () -> LauncherOptions.parse(new String[] {"--port"}), "missing value");
        expectThrows(IllegalArgumentException.class,
                () -> LauncherOptions.parse(new String[] {"--port=70000"}), "port range");
        expectThrows(IllegalArgumentException.class,
                () -> LauncherOptions.parse(new String[] {"--port=abc"}), "port not a number");
    }

    public static void testPrepareLayoutCreatesDirectoriesAndPluginReadme() throws Exception {
        Path dir = TestSupport.tempDir("layout");
        io.zorowarrior.zwrs.LauncherSupport.prepareLayout(dir);
        for (String name : List.of("config", "data", "plugins", "logs", "native")) {
            TestSupport.check(java.nio.file.Files.isDirectory(dir.resolve(name)),
                    "directory " + name + " must exist");
        }
        String readme = java.nio.file.Files.readString(dir.resolve("plugins").resolve("README.txt"));
        contains(readme, "manifest.json", "plugin README explains both ecosystems");
        contains(readme, "docs/JAVA-PLUGIN-COMPATIBILITY.md", "README points at the compatibility doc");
    }

    public static void testDiagnosticsListPlatformFacts() {
        List<String> lines = io.zorowarrior.zwrs.LauncherSupport.diagnostics();
        String text = String.join("\n", lines);
        contains(text, "platform tag", "platform tag reported");
        contains(text, "library file", "library filename reported");
        contains(text, "java.version", "java version reported");
    }
}
