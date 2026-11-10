package io.zorowarrior.zwrs.test.fixture;

import org.bukkit.event.EventHandler;
import org.bukkit.event.player.PlayerJoinEvent;
import org.bukkit.plugin.java.JavaPlugin;

/** A plugin that throws in onEnable, to prove failures are contained. */
public final class BrokenPlugin extends JavaPlugin {

    @Override
    public void onEnable() {
        throw new IllegalStateException("this plugin is intentionally broken");
    }

    /** Never reached: the plugin is disabled before any event. */
    @EventHandler
    public void onJoin(PlayerJoinEvent event) {
        throw new AssertionError("a disabled plugin must not receive events");
    }
}
