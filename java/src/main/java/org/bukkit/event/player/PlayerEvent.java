package org.bukkit.event.player;

import org.bukkit.Player;
import org.bukkit.event.Event;

/** Base class for events about one player. */
public abstract class PlayerEvent extends Event {

    private final Player player;

    /**
     * @param player the player this event concerns
     */
    protected PlayerEvent(Player player) {
        this.player = player;
    }

    /** @return the player */
    public final Player getPlayer() {
        return player;
    }

    /** @return the player's username */
    public final String getPlayerName() {
        return player.getName();
    }
}
