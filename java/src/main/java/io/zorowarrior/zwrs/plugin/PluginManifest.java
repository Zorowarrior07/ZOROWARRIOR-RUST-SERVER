package io.zorowarrior.zwrs.plugin;

import java.io.IOException;
import java.nio.file.Path;
import java.util.List;
import java.util.Map;

/**
 * Public, read-only view of a {@code plugin.yml} descriptor.
 *
 * <p>Used by the plugin host, by {@code --diagnose} style tooling and by the
 * project's tests, which validate real JARs without exposing the parser.</p>
 */
public final class PluginManifest {

    private final PluginDescriptor descriptor;

    private PluginManifest(PluginDescriptor descriptor) {
        this.descriptor = descriptor;
    }

    /** Reads and validates {@code plugin.yml} from {@code jar}. */
    public static PluginManifest read(Path jar) throws IOException {
        return new PluginManifest(PluginYaml.read(jar).descriptor());
    }

    public String name() {
        return descriptor.name();
    }

    public String version() {
        return descriptor.version();
    }

    public String mainClass() {
        return descriptor.mainClass();
    }

    public String apiVersion() {
        return descriptor.apiVersion();
    }

    public String description() {
        return descriptor.description();
    }

    public List<String> authors() {
        return descriptor.authors();
    }

    public List<String> depend() {
        return descriptor.depend();
    }

    public List<String> softDepend() {
        return descriptor.softDepend();
    }

    public Map<String, Object> commands() {
        return descriptor.commands();
    }

    public List<String> permissions() {
        return descriptor.permissions();
    }

    public String website() {
        return descriptor.website();
    }

    @Override
    public String toString() {
        return descriptor.summary();
    }
}
