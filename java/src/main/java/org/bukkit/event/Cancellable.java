package org.bukkit.event;

/** Marker for events whose outcome a plugin may veto. */
public interface Cancellable {

    /** @return whether a handler cancelled the event */
    boolean isCancelled();

    /**
     * Sets the cancellation state.
     *
     * @param cancel desired state
     */
    void setCancelled(boolean cancel);
}
