package org.bukkit;

import java.net.InetSocketAddress;
import java.util.UUID;

/**
 * A player who is currently connected.
 *
 * <p>Identity, address and session data come from the Rust engine. Play-state
 * actions (teleport, inventory, chat) are not implemented in this build: those
 * methods are absent rather than present-and-broken, so a plugin fails to compile
 * against this API instead of failing mysteriously at runtime.</p>
 */
public interface Player {

    /** @return the player's username as observed at login */
    String getName();

    /** @return the player's canonical UUID */
    UUID getUniqueId();

    /** @return the remote address, or {@code null} when not recorded */
    InetSocketAddress getAddress();

    /** @return the connection protocol version */
    int getProtocolVersion();

    /** @return the session start timestamp (ISO-8601 UTC) */
    String getSessionStartedAt();

    /** @return seconds the session has been open */
    long getSessionSeconds();

    /** @return whether the player is still connected */
    boolean isOnline();

    /**
     * Sends a chat-style message. Delivered to the player's connection where the
     * protocol allows it; in this build it is also written to the console so the
     * message is never silently dropped.
     *
     * @param message message text
     */
    void sendMessage(String message);
}
