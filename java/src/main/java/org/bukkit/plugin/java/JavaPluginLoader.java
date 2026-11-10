package org.bukkit.plugin.java;

import org.bukkit.command.Command;
import org.bukkit.plugin.Plugin;

/**
 * The Bukkit loader type plugins cast to.
 *
 * <p>It exists so plugin source that calls {@code (JavaPluginLoader) getLoader()}
 * or {@code getLoader().getPlugin(...)} behaves predictably instead of throwing
 * {@code ClassCastException}. ZWRS implements the parts that make sense here; the
 * real work lives in {@code io.zorowarrior.zwrs.plugin.PluginRuntime}.</p>
 */
public abstract class JavaPluginLoader implements PluginLoader {

    /**
     * @param name plugin name
     * @return the plugin with that name, or {@code null} if it is not loaded
     */
    public abstract Plugin getPlugin(String name);

    /**
     * @param name plugin name
     * @param command command name
     * @return the registered command
     * @throws IllegalArgumentException if the command is not registered
     */
    public abstract Command commandFor(String name, String command);
}
