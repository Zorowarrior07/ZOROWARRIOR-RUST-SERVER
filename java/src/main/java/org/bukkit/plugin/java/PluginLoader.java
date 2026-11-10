package org.bukkit.plugin.java;

import org.bukkit.plugin.Plugin;

/** Creates and disposes plugin instances. Implemented by the ZWRS host. */
public interface PluginLoader {

    /**
     * Logs a line on behalf of the plugin.
     *
     * @param plugin the plugin
     * @param level one of {@code INFO}, {@code WARN}, {@code ERROR}
     * @param message text
     */
    void log(Plugin plugin, String level, String message);

    /**
     * Instantiates a plugin from its main class.
     *
     * @param pluginName plugin name
     * @param mainClass fully-qualified class name
     * @return the plugin instance
     * @throws Exception if the class cannot be loaded or constructed
     */
    Plugin instantiate(String pluginName, String mainClass) throws Exception;

    /**
     * @param pluginName plugin name
     * @return the class loader used for that plugin
     */
    PluginClassLoader getClassLoader(String pluginName);
}
