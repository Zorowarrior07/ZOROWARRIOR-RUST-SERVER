package org.bukkit.event;

/**
 * Marks a method in a {@link Listener} as an event handler.
 *
 * <p>Package and name match Bukkit, so an existing plugin's imports keep working.
 * A handler must take exactly one parameter assignable from the event class; the
 * ZWRS host validates that at registration and reports a clear error instead of
 * failing deep inside dispatch.</p>
 */
@java.lang.annotation.Target(java.lang.annotation.ElementType.METHOD)
@java.lang.annotation.Retention(java.lang.annotation.RetentionPolicy.RUNTIME)
public @interface EventHandler {

    /** @return priority order for this handler */
    EventPriority priority() default EventPriority.NORMAL;

    /** @return whether this handler should still run if the event was cancelled */
    boolean ignoreCancelled() default false;
}
