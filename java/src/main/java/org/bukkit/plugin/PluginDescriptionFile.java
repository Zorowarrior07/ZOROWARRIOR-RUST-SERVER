package org.bukkit.plugin;

import java.util.List;
import java.util.Map;

/**
 * Read-only view of a plugin's {@code plugin.yml}.
 *
 * <p>Field names and semantics follow Bukkit's descriptor so existing plugin
 * metadata works unchanged. Values are immutable.</p>
 */
public final class PluginDescriptionFile {

    private final String name;
    private final String version;
    private final String mainClass;
    private final String apiVersion;
    private final String description;
    private final List<String> authors;
    private final List<String> depend;
    private final List<String> softDepend;
    private final List<String> loadBefore;
    private final String website;
    private final Map<String, Map<String, Object>> commands;

    /**
     * Creates a descriptor.
     *
     * @param name plugin name
     * @param version plugin version
     * @param mainClass fully-qualified main class
     * @param apiVersion declared api-version, or {@code null}
     * @param description human description, or {@code null}
     * @param authors author list, never {@code null}
     * @param depend hard dependencies
     * @param softDepend soft dependencies
     * @param loadBefore plugins that must load after this one
     * @param website project website, or {@code null}
     * @param commands command definitions by name
     */
    public PluginDescriptionFile(String name, String version, String mainClass, String apiVersion,
            String description, List<String> authors, List<String> depend, List<String> softDepend,
            List<String> loadBefore, String website, Map<String, Map<String, Object>> commands) {
        this.name = name;
        this.version = version;
        this.mainClass = mainClass;
        this.apiVersion = apiVersion;
        this.description = description;
        this.authors = List.copyOf(authors);
        this.depend = List.copyOf(depend);
        this.softDepend = List.copyOf(softDepend);
        this.loadBefore = List.copyOf(loadBefore);
        this.website = website;
        this.commands = Map.copyOf(commands);
    }

    /** @return the plugin name */
    public String getName() {
        return name;
    }

    /** @return the plugin version */
    public String getVersion() {
        return version;
    }

    /** @return the fully-qualified main class */
    public String getMain() {
        return mainClass;
    }

    /** @return the declared api-version, or {@code null} when absent */
    public String getAPIVersion() {
        return apiVersion;
    }

    /** @return the description, or {@code null} */
    public String getDescription() {
        return description;
    }

    /** @return the authors */
    public List<String> getAuthors() {
        return authors;
    }

    /** @return hard dependencies */
    public List<String> getDepend() {
        return depend;
    }

    /** @return soft dependencies */
    public List<String> getSoftDepend() {
        return softDepend;
    }

    /** @return plugins that must load after this one */
    public List<String> getLoadBefore() {
        return loadBefore;
    }

    /** @return the website, or {@code null} */
    public String getWebsite() {
        return website;
    }

    /** @return command definitions keyed by command name */
    public Map<String, Map<String, Object>> getCommands() {
        return commands;
    }

    /** @return the descriptor as a readable one-line summary */
    public String getFullName() {
        return name + " v" + version;
    }

    @Override
    public String toString() {
        return "PluginDescriptionFile{" + name + " v" + version + " main=" + mainClass + "}";
    }
}
