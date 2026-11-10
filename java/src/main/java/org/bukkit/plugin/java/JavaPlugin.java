package org.bukkit.plugin.java;

import java.io.File;
import java.io.IOException;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.util.logging.Logger;

import org.bukkit.Server;
import org.bukkit.plugin.Plugin;
import org.bukkit.plugin.PluginDescriptionFile;

/**
 * Base class plugins extend, as in Bukkit.
 *
 * <p>The host creates the instance reflectively (public no-argument constructor),
 * then calls {@link #initialize} before any other method so {@link #getServer()},
 * {@link #getDataFolder()} and friends are valid inside {@code onLoad()}.</p>
 */
public abstract class JavaPlugin implements Plugin {

    private PluginDescriptionFile description;
    private PluginClassLoader classLoader;
    private PluginLoader loader;
    private File dataFolder;
    private File file;
    private Server server;
    private Logger logger;
    private boolean enabled;
    private boolean initialised;

    /**
     * Called by the host. Plugins must not call this.
     *
     * @param server the server
     * @param description descriptor from plugin.yml
     * @param loader the loader that owns this plugin
     * @param classLoader the plugin's class loader
     * @param dataFolder writable plugin directory
     * @param file the plugin JAR
     * @param logger plugin logger
     */
    public final void initialize(Server server, PluginDescriptionFile description,
            PluginLoader loader, PluginClassLoader classLoader, File dataFolder, File file,
            Logger logger) {
        this.server = server;
        this.description = description;
        this.loader = loader;
        this.classLoader = classLoader;
        this.dataFolder = dataFolder;
        this.file = file;
        this.logger = logger;
        this.initialised = true;
        if (dataFolder != null && !dataFolder.exists() && !dataFolder.mkdirs()) {
            logger.warning("could not create the data folder " + dataFolder);
        }
    }

    /**
     * Marks the plugin enabled. Called by the host after {@link #onEnable()}.
     *
     * @param value new state
     */
    public final void setEnabled(boolean value) {
        this.enabled = value;
    }

    private void requireInitialised() {
        if (!initialised) {
            throw new IllegalStateException(
                    "plugin was not initialised by the server; do not call this from a constructor");
        }
    }

    /** @return the server */
    public final Server getServer() {
        return server;
    }

    @Override
    public final PluginDescriptionFile getDescription() {
        return description;
    }

    @Override
    public final File getDataFolder() {
        requireInitialised();
        return dataFolder;
    }

    @Override
    public File getConfigFile() {
        File folder = getDataFolder();
        if (folder == null) {
            return null;
        }
        File config = new File(folder, "config.yml");
        return config.isFile() ? config : null;
    }

    /**
     * Reads the plugin's {@code config.yml} as text.
     *
     * <p>ZWRS does not implement Bukkit's YAML tree model; this returns the raw
     * file so a plugin can parse it with its own library. Returning the text
     * beats returning a half-working {@code FileConfiguration}.</p>
     *
     * @return file contents, or {@code null} when there is no config file
     * @throws IOException if the file exists but cannot be read
     */
    public final String readConfigText() throws IOException {
        File config = getConfigFile();
        if (config == null) {
            return null;
        }
        return Files.readString(config.toPath(), StandardCharsets.UTF_8);
    }

    @Override
    public final boolean isEnabled() {
        return enabled;
    }

    @Override
    public final Logger getLogger() {
        requireInitialised();
        return logger;
    }

    @Override
    public final JavaPluginLoader getLoader() {
        return (JavaPluginLoader) loader;
    }

    /** @return the plugin JAR */
    public final File getFile() {
        return file;
    }

    /** @return this plugin's class loader */
    public final PluginClassLoader getPluginClassLoader() {
        return classLoader;
    }

    @Override
    public final String getName() {
        return description == null ? getClass().getSimpleName() : description.getName();
    }

    /**
     * Registers a command from inside the plugin.
     *
     * @param name command name, or {@code null} to use {@link #getName()}
     * @return the command instance
     * @throws IllegalArgumentException if the name is already taken
     */
    public final org.bukkit.command.Command getCommand(String name) {
        String resolved = name == null ? getName() : name;
        return ((JavaPluginLoader) loader).commandFor(getName(), resolved);
    }

    /**
     * Saves a bundled resource from the JAR to the data folder.
     *
     * @param resource path inside the JAR
     * @throws IOException if the resource is missing or cannot be written
     */
    public final void saveResource(String resource) throws IOException {
        try (java.io.InputStream in = classLoader.getResourceAsStream(resource)) {
            if (in == null) {
                throw new IOException("the plugin JAR does not contain " + resource);
            }
            File target = new File(dataFolder, resource);
            File parent = target.getParentFile();
            if (parent != null && !parent.exists() && !parent.mkdirs()) {
                throw new IOException("could not create " + parent);
            }
            Files.copy(in, target.toPath());
        }
    }

    @Override
    public String toString() {
        return "JavaPlugin{" + getName() + "}";
    }
}
