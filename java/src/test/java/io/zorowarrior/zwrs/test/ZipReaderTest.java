package io.zorowarrior.zwrs.test;

import static io.zorowarrior.zwrs.test.TestSupport.contains;
import static io.zorowarrior.zwrs.test.TestSupport.equal;
import static io.zorowarrior.zwrs.test.TestSupport.expectThrows;

import java.io.IOException;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.Map;

import io.zorowarrior.zwrs.plugin.PluginJar;

/** The JAR reader must decompress real plugin descriptors and refuse junk. */
public final class ZipReaderTest {

    private ZipReaderTest() {
    }

    public static void testReadsDeflatedEntry() throws IOException {
        Path dir = TestSupport.tempDir("zip");
        Path jar = TestSupport.writeJar(dir, "a.jar",
                TestSupport.entries("plugin.yml", "name: X\nversion: 1.0\nmain: a.b.C\n"));
        equal("name: X\nversion: 1.0\nmain: a.b.C\n", PluginJar.readEntry(jar, "plugin.yml"),
                "decompressed plugin.yml");
        Files.deleteIfExists(jar);
    }

    public static void testRejectsNonZip() throws IOException {
        Path dir = TestSupport.tempDir("notzip");
        Path fake = dir.resolve("fake.jar");
        Files.writeString(fake, "this is not a zip file at all", StandardCharsets.UTF_8);
        expectThrows(IOException.class, () -> PluginJar.readEntry(fake, "plugin.yml"),
                "a text file is not a JAR");
        Files.deleteIfExists(fake);
    }

    public static void testMissingEntryIsReported() throws IOException {
        Path dir = TestSupport.tempDir("missing");
        Path jar = TestSupport.writeJar(dir, "b.jar", TestSupport.entries("other.txt", "x"));
        expectThrows(IOException.class, () -> PluginJar.readEntry(jar, "plugin.yml"),
                "missing entry must be an error");
        Files.deleteIfExists(jar);
    }

    public static void testListsEntriesWithSizes() throws IOException {
        Path dir = TestSupport.tempDir("list");
        Path jar = TestSupport.writeJar(dir, "c.jar",
                TestSupport.entries("plugin.yml", "abc", "com/x/Main.class", "classbytes"));
        Map<String, Long> sizes = PluginJar.entries(jar);
        equal(3L, sizes.get("plugin.yml"), "size of plugin.yml");
        equal(10L, sizes.get("com/x/Main.class"), "size of class entry");
        contains(sizes.keySet().toString(), "plugin.yml", "entry names");
        Files.deleteIfExists(jar);
    }

    public static void testTruncatedArchiveIsRejected() throws IOException {
        Path dir = TestSupport.tempDir("trunc");
        Path jar = TestSupport.writeJar(dir, "d.jar", TestSupport.entries("plugin.yml", "name: X\n" + "x".repeat(200)));
        byte[] bytes = Files.readAllBytes(jar);
        byte[] truncated = java.util.Arrays.copyOf(bytes, bytes.length / 2);
        expectThrows(IOException.class, () -> PluginJar.readEntry(truncated, "plugin.yml"),
                "a truncated archive must be rejected");
        Files.deleteIfExists(jar);
    }
}
