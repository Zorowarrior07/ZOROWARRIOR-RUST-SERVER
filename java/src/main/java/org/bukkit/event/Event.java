package org.bukkit.event;

/**
 * Base class for everything the ZWRS host can deliver to a plugin.
 *
 * <p>Cancellable events extend {@link Cancellable}; handlers see the cancellation
 * state through {@link #isCancelled()} exactly like Bukkit.</p>
 */
public abstract class Event {

    private final boolean async;
    private boolean cancelled;

    /** Creates a synchronous event. */
    protected Event() {
        this(false);
    }

    /**
     * Creates an event.
     *
     * @param isAsync whether the event came from another thread
     */
    protected Event(boolean isAsync) {
        this.async = isAsync;
    }

    /**
     * @return the event's type, used for registration and dispatch
     * @deprecated kept for source compatibility with Bukkit plugins; the host
     *             uses {@code getClass()}
     */
    @Deprecated
    public String getEventName() {
        return getClass().getSimpleName();
    }

    /** @return whether the event was fired off the main thread */
    public final boolean isAsynchronous() {
        return async;
    }

    /** @return whether the event can be cancelled */
    public final boolean isCancellable() {
        return this instanceof Cancellable;
    }

    /** @return whether a handler cancelled the event */
    public final boolean isCancelled() {
        return this instanceof Cancellable && cancelled;
    }

    /**
     * Cancels the event.
     *
     * @param cancel desired state
     * @throws IllegalStateException if the event is not cancellable
     */
    public final void setCancelled(boolean cancel) {
        if (!(this instanceof Cancellable)) {
            throw new IllegalStateException(
                    getClass().getName() + " is not cancellable; check isCancellable() first");
        }
        this.cancelled = cancel;
    }

    /** Convenience shorthand for {@code setCancelled(true)}. */
    public final void cancel() {
        setCancelled(true);
    }
}
