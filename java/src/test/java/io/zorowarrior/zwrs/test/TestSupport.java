package io.zorowarrior.zwrs.test;

import java.io.IOException;
import java.io.OutputStream;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.zip.ZipEntry;
import java.util.zip.ZipOutputStream;

/**
 * Tiny assertion helpers and fixtures.
 *
 * <p>The project has no third-party test dependency, so tests are plain
 * {@code main} methods run by {@link TestRunner}. This keeps the whole build
 * reproducible on a phone with nothing but a JDK.</p>
 */
public final class TestSupport {

    private TestSupport() {
    }

    public static void check(boolean condition, String message) {
        if (!condition) {
            throw new AssertionError(message);
        }
    }

    public static void equal(Object expected, Object actual, String message) {
        if (expected == null ? actual != null : !expected.equals(actual)) {
            throw new AssertionError(message + " (expected " + expected + ", got " + actual + ")");
        }
    }

    public static void contains(String haystack, String needle, String message) {
        if (haystack == null || !haystack.contains(needle)) {
            throw new AssertionError(message + " (expected to contain \"" + needle + "\", got \""
                    + haystack + "\")");
        }
    }

    /** A {@link Runnable} that may throw a checked exception. */
    public interface ThrowingRunnable {
        void run() throws Exception;
    }

    public static Throwable expectThrows(Class<? extends Throwable> type, ThrowingRunnable action, String message) {
        try {
            action.run();
        } catch (Throwable error) {
            if (type.isInstance(error)) {
                return error;
            }
            throw new AssertionError(message + " (expected " + type.getSimpleName() + ", got "
                    + error.getClass().getSimpleName() + ": " + error.getMessage() + ")");
        }
        throw new AssertionError(message + " (expected " + type.getSimpleName() + " but nothing was thrown)");
    }

    /** Creates a temporary directory that is removed on JVM exit. */
    public static Path tempDir(String tag) throws IOException {
        Path dir = Files.createTempDirectory("zwrs-java-" + tag + "-");
        dir.toFile().deleteOnExit();
        return dir;
    }

    /** Writes a JAR containing the given text entries. */
    public static Path writeJar(Path dir, String name, Map<String, byte[]> entries) throws IOException {
        Path jar = dir.resolve(name);
        try (OutputStream out = Files.newOutputStream(jar);
                ZipOutputStream zip = new ZipOutputStream(out, StandardCharsets.UTF_8)) {
            for (Map.Entry<String, byte[]> entry : entries.entrySet()) {
                zip.putNextEntry(new ZipEntry(entry.getKey()));
                zip.write(entry.getValue());
                zip.closeEntry();
            }
        }
        return jar;
    }

    public static Map<String, byte[]> entries(Object... pairs) {
        Map<String, byte[]> map = new LinkedHashMap<>();
        for (int i = 0; i < pairs.length; i += 2) {
            map.put((String) pairs[i], ((String) pairs[i + 1]).getBytes(StandardCharsets.UTF_8));
        }
        return map;
    }

    /** A minimal, valid Java class file with the requested major version. */
    public static byte[] classFile(int majorVersion) {
        List<Byte> bytes = new ArrayList<>();
        int[] header = {0xCA, 0xFE, 0xBA, 0xBE, 0x00, 0x00, (majorVersion >> 8) & 0xff, majorVersion & 0xff};
        for (int b : header) {
            bytes.add((byte) b);
        }
        // constant pool count = 3: one Class entry ("java/lang/Object") and one UTF-8 name.
        bytes.add((byte) 0x00);
        bytes.add((byte) 0x03);
        bytes.add((byte) 0x01); // Utf8
        bytes.add((byte) 0x00);
        bytes.add((byte) 0x10);
        for (byte b : "java/lang/Object".getBytes(StandardCharsets.UTF_8)) {
            bytes.add(b);
        }
        bytes.add((byte) 0x07); // Class
        bytes.add((byte) 0x00);
        bytes.add((byte) 0x01);
        // access flags, this, super, interfaces, fields, methods, attributes
        for (int b : new int[] {0x00, 0x21, 0x00, 0x02, 0x00, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00}) {
            bytes.add((byte) b);
        }
        byte[] out = new byte[bytes.size()];
        for (int i = 0; i < bytes.size(); i++) {
            out[i] = bytes.get(i);
        }
        return out;
    }

    /** A plugin.yml for a plugin whose main class exists in the JAR. */
    public static String pluginYml(String name, String version, String mainClass, String apiVersion) {
        StringBuilder yml = new StringBuilder();
        yml.append("name: ").append(name).append('\n');
        yml.append("version: ").append(version).append('\n');
        yml.append("main: ").append(mainClass).append('\n');
        if (apiVersion != null) {
            yml.append("api-version: ").append(apiVersion).append('\n');
        }
        yml.append("description: a test plugin\n");
        yml.append("authors:\n");
        yml.append("  - TestAuthor\n");
        yml.append("commands:\n");
        yml.append("  hello:\n");
        yml.append("    description: greets the player\n");
        yml.append("    usage: /hello\n");
        yml.append("permissions:\n");
        yml.append("  test.hello:\n");
        yml.append("    description: allows /hello\n");
        yml.append("    default: true\n");
        return yml.toString();
    }
}
