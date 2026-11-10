package org.bukkit.plugin;

import java.io.File;
import java.util.logging.Logger;

/**
 * The minimal Bukkit plugin contract ZWRS implements.
 *
 * <p>This is a compatibility subset, not the whole Bukkit API. Every method here
 * is implemented and tested by the ZWRS plugin host; anything else a plugin
 * calls will fail with {@link NoClassDefFoundError} or a missing-method error,
 * which is reported by name rather than hidden.</p>
 */
public interface Plugin {

    /** Called after the plugin is instantiated, before {@link #onEnable()}. */
    default void onLoad() {
    }

    /** Called when the plugin becomes active. */
    default void onEnable() {
    }

    /** Called when the plugin is deactivated. */
    default void onDisable() {
    }

    /** The descriptor read from {@code plugin.yml}. */
    PluginDescriptionFile getDescription();

    /** @return the plugin name from the descriptor */
    String getName();

    /** Data folder ({@code plugins/<name>}), created before {@link #onEnable()}. */
    File getDataFolder();

    /** The config file ({@code plugins/<name>/config.yml}) if one exists. */
    File getConfigFile();

    /** Whether the plugin is currently enabled. */
    boolean isEnabled();

    /** Plugin logger: {@code [name] message} on the server console. */
    Logger getLogger();

    /** The server the plugin is registered with. */
    org.bukkit.plugin.java.JavaPluginLoader getLoader();
}
