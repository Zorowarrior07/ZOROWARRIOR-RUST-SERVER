package io.zorowarrior.zwrs.plugin;

import java.nio.file.Path;
import java.util.List;
import java.util.Map;

/**
 * Metadata read from a plugin JAR's {@code plugin.yml}.
 *
 * <p>Only the fields the host actually uses are modelled. Unknown keys are kept
 * in {@link #extra()} so they can be reported instead of silently dropped.</p>
 */
record PluginDescriptor(
        String name,
        String version,
        String mainClass,
        String apiVersion,
        String description,
        List<String> authors,
        List<String> depend,
        List<String> softDepend,
        List<String> loadBefore,
        List<String> load,
        String website,
        Map<String, Object> commands,
        List<String> permissions,
        Map<String, Object> extra,
        Path jar) {

    /** Whether the descriptor names a Bukkit/Spigot/Paper API version. */
    boolean declaresApiVersion() {
        return apiVersion != null;
    }

    /** Short one-line summary for logs. */
    String summary() {
        return name + " v" + version + " (main=" + mainClass + ", api=" + apiVersion + ")";
    }
}
