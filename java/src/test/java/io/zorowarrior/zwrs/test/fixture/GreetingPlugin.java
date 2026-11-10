package io.zorowarrior.zwrs.test.fixture;

import org.bukkit.command.Command;
import org.bukkit.command.CommandSender;
import org.bukkit.event.EventHandler;
import org.bukkit.event.Listener;
import org.bukkit.event.player.PlayerJoinEvent;
import org.bukkit.event.player.PlayerQuitEvent;
import org.bukkit.plugin.java.JavaPlugin;

/**
 * A real plugin used by the tests: registers listeners and a command, and records
 * what happened in {@link Recorder} so the test can observe it across class
 * loaders.
 *
 * <p>It only uses API the host documents as implemented; if the host changes
 * incompatibly, this fixture stops compiling, which is the point.</p>
 */
public final class GreetingPlugin extends JavaPlugin implements Listener {

    @Override
    public void onEnable() {
        Recorder.enabled = true;
        Recorder.EVENTS.add("enable:" + getName());
        getServer().getPluginManager().registerEvents(this, this);

        Command command = new Command("greet") {
            @Override
            public boolean execute(CommandSender sender, String label, String[] args) {
                String who = args.length > 0 ? args[0] : "world";
                Recorder.EVENTS.add("command:" + label + ":" + who);
                sender.sendMessage("Hello, " + who + "!");
                return true;
            }
        };
        command.setDescription("greets someone");
        if (!getServer().getPluginManager().registerCommand(command, this)) {
            throw new IllegalStateException("/greet is already taken");
        }
    }

    @EventHandler
    public void onJoin(PlayerJoinEvent event) {
        Recorder.EVENTS.add("join:" + event.getPlayer().getName()
                + ":" + event.getPlayer().getUniqueId()
                + ":" + event.getPlayer().getProtocolVersion()
                + ":" + event.getPlayer().isOnline());
        event.setJoinMessage("welcome " + event.getPlayer().getName());
    }

    @EventHandler
    public void onQuit(PlayerQuitEvent event) {
        Recorder.EVENTS.add("quit:" + event.getPlayer().getName()
                + ":" + event.getPlayer().isOnline());
    }

    @Override
    public void onDisable() {
        Recorder.disabled = true;
        Recorder.EVENTS.add("disable:" + getName());
    }
}
