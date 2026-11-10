package io.zorowarrior.zwrs.plugin;

import java.io.IOException;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.List;
import java.util.Locale;

/**
 * Scans the plugin directory for both ecosystems and delegates to the right host.
 *
 * <p>Java plugins: {@link JavaPluginHost}. Rust/WASM plugins are validated by the
 * Rust engine ({@code zwrs_engine::plugins}); the launcher only reports that the
 * directory exists so the two reports never contradict each other.</p>
 */
public final class PluginManager {

    private final Path pluginsDir;
    private final JavaPluginHost javaHost;
    private final List<String> problems = new ArrayList<>();

    public PluginManager(Path pluginsDir, JavaPluginHost javaHost) {
        this.pluginsDir = pluginsDir;
        this.javaHost = javaHost;
    }

    /** Discovers plugins in both ecosystems and returns report lines. */
    public List<String> discover() {
        return discover(pluginsDir);
    }

    /** Discovers plugins in {@code directory}, which defaults to the configured one. */
    public List<String> discover(Path directory) {
        Path pluginsDir = directory;
        List<String> lines = new ArrayList<>();
        problems.clear();
        try {
            lines.addAll(javaHost.discover(pluginsDir));
        } catch (RuntimeException error) {
            problems.add("Java plugin discovery failed: " + error);
        }

        if (Files.isDirectory(pluginsDir)) {
            try (var stream = Files.list(pluginsDir)) {
                List<Path> wasmDirs = stream
                        .filter(Files::isDirectory)
                        .filter(dir -> Files.isRegularFile(dir.resolve("manifest.json")))
                        .sorted()
                        .toList();
                for (Path dir : wasmDirs) {
                    lines.add("Rust plugin candidate " + dir.getFileName()
                            + ": manifest present; the Rust engine validates and reports it (no WASM runtime is linked in this build)");
                }
            } catch (IOException error) {
                problems.add("could not inspect " + pluginsDir + ": " + error.getMessage());
            }
        }

        long jars;
        try (var stream = Files.list(pluginsDir)) {
            jars = stream
                    .filter(Files::isRegularFile)
                    .filter(path -> path.getFileName().toString().toLowerCase(Locale.ROOT).endsWith(".jar"))
                    .count();
        } catch (IOException error) {
            jars = 0;
        }
        lines.add("plugin scan complete: " + jars + " Java JAR(s)");
        return lines;
    }

    /** Problems collected during the last scan. */
    public List<String> problems() {
        return List.copyOf(problems);
    }

    /** Delegate for tests and the console. */
    public JavaPluginHost javaHost() {
        return javaHost;
    }
}
