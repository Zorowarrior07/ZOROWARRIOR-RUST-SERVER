package org.bukkit;

import java.util.List;
import java.util.UUID;

import org.bukkit.event.Event;
import org.bukkit.plugin.PluginManager;

/**
 * The part of the Bukkit server singleton that ZWRS implements.
 *
 * <p>Every method here is backed by live data from the Rust engine: the online
 * list comes from the session registry, {@code dispatchCommand} reaches the same
 * console command table, and {@link #getVersion()} says which implementation is
 * running instead of pretending to be CraftBukkit.</p>
 */
public interface Server {

    /** @return the server implementation name, e.g. {@code ZWRS} */
    String getName();

    /** @return the implementation and engine version */
    String getVersion();

    /** @return the advertised Minecraft version */
    String getBukkitVersion();

    /** @return the plugin manager (registration and dispatch) */
    PluginManager getPluginManager();

    /** @return the logger for server messages */
    java.util.logging.Logger getLogger();

    /** @return the MOTD shown in the server list */
    String getMotd();

    /** @return the configured maximum player count */
    int getMaxPlayers();

    /** @return usernames of players online right now, sorted */
    List<String> getOnlinePlayers();

    /** @return the number of players online right now */
    int getOnlinePlayerCount();

    /**
     * Looks up an online player.
     *
     * @param name username, case-insensitive
     * @return the player, or {@code null} if not online
     */
    Player getPlayer(String name);

    /**
     * Looks up an online player.
     *
     * @param uuid player UUID
     * @return the player, or {@code null} if not online
     */
    Player getPlayer(UUID uuid);

    /** @return every player currently online */
    List<Player> getOnlinePlayerObjects();

    /**
     * Runs a command exactly as if it had been typed in the console.
     *
     * @param line command line, with or without a leading slash
     * @return the lines the command produced
     */
    List<String> dispatchCommand(String line);

    /**
     * Delivers an event to registered listeners.
     *
     * @param event the event
     */
    void callEvent(Event event);

    /** @return whether the server is in offline mode (no Mojang authentication) */
    boolean isOfflineMode();

    /** @return seconds since the engine started */
    long getUptimeSeconds();
}
