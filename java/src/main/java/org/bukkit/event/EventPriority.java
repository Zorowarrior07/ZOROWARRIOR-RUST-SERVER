package org.bukkit.event;

/**
 * Handler ordering, lowest value first.
 *
 * <p>The host sorts handlers by priority and, within a priority, by registration
 * order, so dispatch is deterministic and testable.</p>
 */
public enum EventPriority {
    /** Runs first, before any plugin. */
    LOWEST,
    /** Runs before normal handlers. */
    LOW,
    /** Default priority. */
    NORMAL,
    /** Runs after normal handlers. */
    HIGH,
    /** Runs after high handlers. */
    HIGHEST,
    /** Runs last, for observers that must see the final state. */
    MONITOR
}
