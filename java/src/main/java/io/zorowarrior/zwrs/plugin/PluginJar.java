package io.zorowarrior.zwrs.plugin;

import java.io.IOException;
import java.nio.file.Path;
import java.util.Map;

/**
 * Test- and tool-facing access to a plugin JAR's contents.
 *
 * <p>Keeps the defensive ZIP reader package-private while still letting the
 * project's own tests (and future admin tools) inspect archives.</p>
 */
public final class PluginJar {

    private PluginJar() {
    }

    /** Decompressed text of `entry`. */
    public static String readEntry(Path jar, String entry) throws IOException {
        try (ZipReader reader = ZipReader.open(jar)) {
            return reader.readText(entry);
        }
    }

    /** Decompressed text of `entry` from an in-memory archive. */
    public static String readEntry(byte[] archive, String entry) throws IOException {
        try (ZipReader reader = ZipReader.open(archive)) {
            return reader.readText(entry);
        }
    }

    /** Entry names and uncompressed sizes. */
    public static Map<String, Long> entries(Path jar) throws IOException {
        try (ZipReader reader = ZipReader.open(jar)) {
            return reader.entrySizes();
        }
    }
}
