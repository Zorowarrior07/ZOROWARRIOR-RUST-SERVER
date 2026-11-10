package io.zorowarrior.zwrs;

import java.io.IOException;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.List;

/**
 * Small helpers the launcher shares with its tests.
 *
 * <p>Kept separate from {@link Launcher} so tests can prepare a server directory
 * or print diagnostics without starting a server or installing signal handlers.</p>
 */
public final class LauncherSupport {

    private LauncherSupport() {
    }

    /**
     * Creates the documented server layout and the plugin README.
     *
     * <p>Existing directories and files are never removed or overwritten, so
     * pointing the launcher at an existing server directory is safe.</p>
     */
    public static void prepareLayout(Path directory) throws IOException {
        for (String name : new String[] {"config", "data", "plugins", "logs", "native"}) {
            Files.createDirectories(directory.resolve(name));
        }
        Path readme = directory.resolve("plugins").resolve("README.txt");
        if (!Files.exists(readme)) {
            Files.writeString(readme, String.join(System.lineSeparator(),
                    "Place Java plugins (.jar) and Rust plugins (directories containing",
                    "manifest.json + .wasm) in this folder.",
                    "",
                    "Java plugins are inspected by the launcher's plugin host (Bukkit-compatible",
                    "subset; see docs/JAVA-PLUGIN-COMPATIBILITY.md). Loading plugin code is not",
                    "implemented yet, so every plugin is reported as metadata-compatible or",
                    "incompatible and nothing is executed.",
                    "",
                    "Rust/WASM plugins are validated by the engine (manifest, API version, \\0asm",
                    "header). This build links no WASM runtime, so nothing is executed; see",
                    "docs/RUST-PLUGIN-DEVELOPMENT.md.",
                    ""), StandardCharsets.UTF_8);
        }
    }

    /** Platform and library diagnostics for {@code --diagnose}. */
    public static List<String> diagnostics() {
        return NativeLibraryLoader.diagnostics();
    }
}
