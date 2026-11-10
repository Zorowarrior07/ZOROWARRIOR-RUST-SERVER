package io.zorowarrior.zwrs.test;

import static io.zorowarrior.zwrs.test.TestSupport.contains;
import static io.zorowarrior.zwrs.test.TestSupport.equal;
import static io.zorowarrior.zwrs.test.TestSupport.expectThrows;

import java.io.IOException;
import java.nio.file.Path;
import java.util.List;

import io.zorowarrior.zwrs.plugin.PluginManifest;

/** Descriptor parsing must accept real plugin.yml files and reject broken ones. */
public final class PluginYamlTest {

    private PluginYamlTest() {
    }

    public static void testParsesFullDescriptor() throws IOException {
        Path dir = TestSupport.tempDir("yml");
        Path jar = TestSupport.writeJar(dir, "full.jar", TestSupport.entries("plugin.yml",
                TestSupport.pluginYml("FullPlugin", "1.2.3", "com.example.FullPlugin", "1.20")));
        PluginManifest manifest = PluginManifest.read(jar);
        TestSupport.equal("FullPlugin", manifest.name(), "name");
        TestSupport.equal("1.2.3", manifest.version(), "version");
        TestSupport.equal("com.example.FullPlugin", manifest.mainClass(), "main");
        TestSupport.equal("1.20", manifest.apiVersion(), "api-version");
        TestSupport.equal("a test plugin", manifest.description(), "description");
        equal(List.of("TestAuthor"), manifest.authors(), "authors list");
        TestSupport.check(manifest.commands().containsKey("hello"),
                "command parsed: " + manifest.commands());
        TestSupport.check(manifest.permissions().contains("test.hello"),
                "permission parsed: " + manifest.permissions());
    }

    public static void testMissingRequiredFieldsAreRejected() throws IOException {
        Path dir = TestSupport.tempDir("ymlbad");
        Path noName = TestSupport.writeJar(dir, "noname.jar",
                TestSupport.entries("plugin.yml", "version: 1.0\nmain: a.b.C\n"));
        expectThrows(IOException.class, () -> PluginManifest.read(noName), "name is required");

        Path noMain = TestSupport.writeJar(dir, "nomain.jar",
                TestSupport.entries("plugin.yml", "name: X\nversion: 1.0\n"));
        expectThrows(IOException.class, () -> PluginManifest.read(noMain), "main is required");

        Path noVersion = TestSupport.writeJar(dir, "nover.jar",
                TestSupport.entries("plugin.yml", "name: X\nmain: a.b.C\n"));
        expectThrows(IOException.class, () -> PluginManifest.read(noVersion), "version is required");
    }

    public static void testRejectsBadMainClassAndName() throws IOException {
        Path dir = TestSupport.tempDir("ymlbad2");
        Path badMain = TestSupport.writeJar(dir, "badmain.jar",
                TestSupport.entries("plugin.yml", "name: X\nversion: 1.0\nmain: not a class\n"));
        expectThrows(IOException.class, () -> PluginManifest.read(badMain), "main must be qualified");

        Path badName = TestSupport.writeJar(dir, "badname.jar",
                TestSupport.entries("plugin.yml", "name: bad name!\nversion: 1.0\nmain: a.b.C\n"));
        expectThrows(IOException.class, () -> PluginManifest.read(badName), "name charset");
    }

    public static void testMissingDescriptorIsReported() throws IOException {
        Path dir = TestSupport.tempDir("ymlmissing");
        Path jar = TestSupport.writeJar(dir, "nomanifest.jar", TestSupport.entries("readme.txt", "hi"));
        try {
            PluginManifest.read(jar);
            throw new AssertionError("expected an IOException for a JAR without plugin.yml");
        } catch (IOException error) {
            contains(error.getMessage(), "plugin.yml", "message names the missing file");
        }
    }

    public static void testQuotedValuesAndComments() throws IOException {
        Path dir = TestSupport.tempDir("ymlquote");
        String yml = """
                # leading comment
                name: "Quoted Plugin"
                version: '2.0.0'
                main: com.example.Quoted
                description: "has a # inside"
                """;
        Path jar = TestSupport.writeJar(dir, "quoted.jar", TestSupport.entries("plugin.yml", yml));
        // Quoted names with spaces are legal YAML but not legal Bukkit plugin names.
        expectThrows(IOException.class, () -> PluginManifest.read(jar), "name with space rejected");
    }
}
