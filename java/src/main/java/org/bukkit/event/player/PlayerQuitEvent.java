package org.bukkit.event.player;

import org.bukkit.Player;

/** Fired after the session closed and playtime was credited. */
public class PlayerQuitEvent extends PlayerEvent {

    private String quitMessage;

    /**
     * @param player the player who left
     * @param quitMessage default announcement, or {@code null}
     */
    public PlayerQuitEvent(Player player, String quitMessage) {
        super(player);
        this.quitMessage = quitMessage;
    }

    /** @return the announcement text, or {@code null} */
    public String getQuitMessage() {
        return quitMessage;
    }

    /**
     * Replaces the announcement text.
     *
     * @param quitMessage new text, or {@code null}
     */
    public void setQuitMessage(String quitMessage) {
        this.quitMessage = quitMessage;
    }
}
