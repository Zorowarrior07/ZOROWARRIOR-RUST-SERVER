package io.zorowarrior.zwrs.plugin;

import java.net.InetSocketAddress;
import java.util.Objects;
import java.util.UUID;

import org.bukkit.Player;
import io.zorowarrior.zwrs.Log;

/**
 * A connected player, built from a real engine event.
 *
 * <p>Only data the engine actually reported is exposed. Play-state actions
 * (teleport, inventory, chat delivery) are deliberately absent from the
 * {@link Player} interface in this build: a plugin that needs them fails to
 * compile instead of failing at runtime.</p>
 */
final class PlayerImpl implements Player {

    private final String name;
    private final UUID uuid;
    private final InetSocketAddress address;
    private final int protocol;
    private final String sessionStartedAt;
    private final long sessionSeconds;
    private volatile boolean online = true;

    private PlayerImpl(String name, UUID uuid, InetSocketAddress address, int protocol,
            String sessionStartedAt, long sessionSeconds) {
        this.name = name;
        this.uuid = uuid;
        this.address = address;
        this.protocol = protocol;
        this.sessionStartedAt = sessionStartedAt;
        this.sessionSeconds = sessionSeconds;
    }

    /**
     * Builds a player from an engine event.
     *
     * @param event parsed event document
     * @param joining whether this is a join (as opposed to a quit)
     * @return the player, or {@code null} when the event is unusable
     */
    static PlayerImpl fromEvent(java.util.Map<String, Object> event, boolean joining) {
        Object rawUuid = event.get("uuid");
        Object rawName = event.get("username");
        if (!(rawUuid instanceof String uuidText) || !(rawName instanceof String name)) {
            return null;
        }
        UUID uuid;
        try {
            uuid = UUID.fromString(uuidText);
        } catch (IllegalArgumentException e) {
            return null;
        }
        InetSocketAddress address = null;
        if (event.get("address") instanceof String text && !text.isBlank()) {
            int colon = text.lastIndexOf(':');
            if (colon > 0) {
                try {
                    address = new InetSocketAddress(text.substring(0, colon),
                            Integer.parseInt(text.substring(colon + 1)));
                } catch (RuntimeException ignored) {
                    address = null;
                }
            }
        }
        int protocol = event.get("protocol") instanceof Number number ? number.intValue() : -1;
        String started = event.get("timestamp") instanceof String value ? value : "";
        long seconds = event.get("session_seconds") instanceof Number number
                ? number.longValue() : 0L;
        PlayerImpl player = new PlayerImpl(name, uuid, address, protocol, started, seconds);
        player.online = joining;
        return player;
    }

    @Override
    public String getName() {
        return name;
    }

    @Override
    public UUID getUniqueId() {
        return uuid;
    }

    @Override
    public InetSocketAddress getAddress() {
        return address;
    }

    @Override
    public int getProtocolVersion() {
        return protocol;
    }

    @Override
    public String getSessionStartedAt() {
        return sessionStartedAt;
    }

    @Override
    public long getSessionSeconds() {
        return sessionSeconds;
    }

    @Override
    public boolean isOnline() {
        return online;
    }

    @Override
    public void sendMessage(String message) {
        // This build has no play-state packets, so there is nowhere to deliver a
        // chat message. Saying so beats a silent no-op that looks like it worked.
        Log.info("[chat to " + name + "] " + message
                + " (not delivered in-game: play-state packets are not implemented)");
    }

    @Override
    public boolean equals(Object other) {
        return other instanceof PlayerImpl player && player.uuid.equals(uuid);
    }

    @Override
    public int hashCode() {
        return Objects.hash(uuid);
    }

    @Override
    public String toString() {
        return name + " (" + uuid + ")";
    }
}
