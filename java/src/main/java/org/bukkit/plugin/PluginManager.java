package org.bukkit.plugin;

import org.bukkit.event.Event;
import org.bukkit.event.EventPriority;
import org.bukkit.event.Listener;

/**
 * Plugin registration and event dispatch.
 *
 * <p>Listeners are registered with a {@link Listener} instance; methods annotated
 * with {@code @EventHandler} are discovered once, at registration, and cached.
 * Dispatch is synchronous on the thread that fired the event.</p>
 */
public interface PluginManager {

    /**
     * Registers every {@code @EventHandler} method of {@code listener}.
     *
     * @param plugin owning plugin
     * @param listener listener instance
     */
    void registerEvents(Listener listener, Plugin plugin);

    /**
     * Registers a single listener for one event class.
     *
     * @param eventClass event type
     * @param listener listener instance
     * @param priority priority
     * @param plugin owning plugin
     * @param <T> event type
     */
    <T extends Event> void registerEvent(Class<T> eventClass, Listener listener,
            EventPriority priority, Plugin plugin);

    /**
     * Delivers an event to every registered listener in priority order.
     *
     * @param event the event
     */
    void callEvent(Event event);

    /** @return whether an event has a listener other than the default one */
    boolean isEventRegistered(Class<? extends Event> eventClass);

    /**
     * Registers a console command.
     *
     * @param name command name without a slash
     * @param plugin owning plugin
     * @return whether the name was free
     */
    boolean registerCommand(String name, Plugin plugin);

    /**
     * Registers a real command object, including its aliases.
     *
     * <p>Bukkit exposes this through the server's command map; ZWRS puts it on the
     * plugin manager so a plugin needs no extra service lookup.</p>
     *
     * @param command the command
     * @param plugin owning plugin
     * @return whether the name (and every alias) was free
     */
    boolean registerCommand(org.bukkit.command.Command command, Plugin plugin);

    /** @return registered command names, sorted */
    java.util.List<String> getRegisteredCommands();

    /** @return the plugin providing {@code command}, or {@code null} */
    Plugin getCommandOwner(String command);
}
