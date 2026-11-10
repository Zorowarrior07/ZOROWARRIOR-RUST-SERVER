package org.bukkit;

/**
 * Access point for the server singleton, as in Bukkit.
 *
 * <p>{@link #getServer()} returns the ZWRS implementation. It is installed by the
 * launcher before any plugin is enabled; calling it from a plugin constructor
 * (before the host sets it) throws with an explanation rather than returning
 * {@code null} and causing an obscure {@code NullPointerException} later.</p>
 */
public final class Bukkit {

    private static volatile Server instance;

    private Bukkit() {
    }

    /**
     * @return the running server
     * @throws IllegalStateException if the server is not running
     */
    public static Server getServer() {
        Server server = instance;
        if (server == null) {
            throw new IllegalStateException(
                    "no server is running: call Bukkit.getServer() from onEnable(), not from a constructor");
        }
        return server;
    }

    /**
     * Installs the singleton. Called by the host.
     *
     * @param server the server
     */
    public static void setServer(Server server) {
        instance = server;
    }

    /** @return the logger of the running server, or a detached logger before startup */
    public static java.util.logging.Logger getLogger() {
        Server server = instance;
        return server == null
                ? java.util.logging.Logger.getLogger("Minecraft")
                : server.getLogger();
    }

    /** @return the global event dispatcher */
    public static org.bukkit.plugin.PluginManager getPluginManager() {
        return getServer().getPluginManager();
    }
}
