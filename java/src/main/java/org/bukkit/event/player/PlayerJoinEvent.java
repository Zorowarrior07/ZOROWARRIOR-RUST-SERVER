package org.bukkit.event.player;

import org.bukkit.Player;

/**
 * Fired after a login is admitted and the profile and session are persisted.
 *
 * <p>Naming follows Bukkit so existing plugins compile: the join message is the
 * text other players would see, and a plugin may replace it.</p>
 */
public class PlayerJoinEvent extends PlayerEvent {

    private String joinMessage;

    /**
     * @param player the player who joined
     * @param joinMessage default announcement, or {@code null}
     */
    public PlayerJoinEvent(Player player, String joinMessage) {
        super(player);
        this.joinMessage = joinMessage;
    }

    /** @return the announcement text, or {@code null} */
    public String getJoinMessage() {
        return joinMessage;
    }

    /**
     * Replaces the announcement text.
     *
     * @param joinMessage new text, or {@code null} to stay quiet
     */
    public void setJoinMessage(String joinMessage) {
        this.joinMessage = joinMessage;
    }
}
