package io.zorowarrior.zwrs.plugin;

import java.util.ArrayList;
import java.util.Comparator;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Locale;
import java.util.Map;
import java.util.UUID;
import java.util.concurrent.ConcurrentHashMap;

import org.bukkit.Player;
import org.bukkit.Server;
import org.bukkit.command.CommandSender;
import org.bukkit.event.Event;
import org.bukkit.plugin.PluginManager;

import io.zorowarrior.zwrs.Log;

/**
 * The {@link Server} plugins see.
 *
 * <p>Online players, the console command table and the MOTD come from the Rust
 * engine through the launcher. When the engine cannot answer (it is stopping, or
 * the launcher is shutting down) the cached data is returned rather than a fake
 * empty list.</p>
 */
public final class ServerImpl implements Server {

    /** Supplies live data from the launcher/engine. */
    public interface EngineAccess {
        /**
         * @param line console command line
         * @return the lines the engine produced
         */
        List<String> runCommand(String line);
    }

    private final PluginRuntime runtime;
    private final EngineAccess engine;
    private final String motd;
    private final int maxPlayers;
    private final boolean offlineMode;
    private final String version;
    private final String bukkitVersion;
    private final long startedAtMillis = System.currentTimeMillis();
    private final Map<UUID, PlayerImpl> online = new ConcurrentHashMap<>();
    private final CommandSender console = new ConsoleSender();

    /**
     * @param runtime plugin runtime used for event dispatch
     * @param engine live command access
     * @param motd configured MOTD
     * @param maxPlayers configured player limit
     * @param offlineMode whether authentication is disabled
     * @param version engine version string
     * @param bukkitVersion advertised Minecraft version
     */
    public ServerImpl(PluginRuntime runtime, EngineAccess engine, String motd, int maxPlayers,
            boolean offlineMode, String version, String bukkitVersion) {
        this.runtime = runtime;
        this.engine = engine;
        this.motd = motd;
        this.maxPlayers = maxPlayers;
        this.offlineMode = offlineMode;
        this.version = version;
        this.bukkitVersion = bukkitVersion;
    }

    @Override
    public String getName() {
        return "ZWRS";
    }

    @Override
    public String getVersion() {
        return version;
    }

    @Override
    public String getBukkitVersion() {
        return bukkitVersion;
    }

    @Override
    public PluginManager getPluginManager() {
        return runtime;
    }

    @Override
    public java.util.logging.Logger getLogger() {
        return java.util.logging.Logger.getLogger("ZWRS");
    }

    @Override
    public String getMotd() {
        return motd;
    }

    @Override
    public int getMaxPlayers() {
        return maxPlayers;
    }

    @Override
    public List<String> getOnlinePlayers() {
        return online.values().stream().map(PlayerImpl::getName).sorted().toList();
    }

    @Override
    public int getOnlinePlayerCount() {
        return online.size();
    }

    @Override
    public Player getPlayer(String name) {
        if (name == null) {
            return null;
        }
        return online.values().stream()
                .filter(player -> player.getName().equalsIgnoreCase(name))
                .findFirst()
                .orElse(null);
    }

    @Override
    public Player getPlayer(UUID uuid) {
        return uuid == null ? null : online.get(uuid);
    }

    @Override
    public List<Player> getOnlinePlayerObjects() {
        List<Player> players = new ArrayList<>(online.values());
        players.sort(Comparator.comparing(Player::getName));
        return players;
    }

    @Override
    public List<String> dispatchCommand(String line) {
        return engine == null ? List.of() : engine.runCommand(line);
    }

    @Override
    public void callEvent(Event event) {
        runtime.callEvent(event);
    }

    @Override
    public boolean isOfflineMode() {
        return offlineMode;
    }

    @Override
    public long getUptimeSeconds() {
        return (System.currentTimeMillis() - startedAtMillis) / 1000L;
    }

    /**
     * Records a join.
     *
     * @param event engine event document
     * @return the player, or {@code null} when the event is unusable
     */
    PlayerImpl track(Map<String, Object> event) {
        PlayerImpl player = PlayerImpl.fromEvent(event, true);
        if (player != null) {
            online.put(player.getUniqueId(), player);
        }
        return player;
    }

    /**
     * Records a quit.
     *
     * @param event engine event document
     * @return the player that left, or {@code null}
     */
    PlayerImpl untrack(Map<String, Object> event) {
        PlayerImpl player = PlayerImpl.fromEvent(event, false);
        if (player != null) {
            online.remove(player.getUniqueId());
        }
        return player;
    }

    /** @return the console command sender handed to plugins */
    public CommandSender consoleSender() {
        return console;
    }

    /** @return a snapshot of tracked players, for diagnostics */
    public Map<String, Object> snapshot() {
        Map<String, Object> map = new LinkedHashMap<>();
        map.put("online", getOnlinePlayerCount());
        map.put("players", getOnlinePlayers());
        return map;
    }

    /** Console sender: prints through the launcher log. */
    private final class ConsoleSender implements CommandSender {
        @Override
        public void sendMessage(String message) {
            if (message == null) {
                return;
            }
            for (String line : message.split("\n", -1)) {
                Log.info(line);
            }
        }

        @Override
        public String getName() {
            return "CONSOLE";
        }

        @Override
        public boolean isConsole() {
            return true;
        }

        @Override
        public boolean dispatchCommand(String line) {
            List<String> lines = ServerImpl.this.dispatchCommand(line);
            return !lines.isEmpty();
        }
    }

    @Override
    public String toString() {
        return "ZWRS " + version + " (" + bukkitVersion + ", "
                + getOnlinePlayerCount() + "/" + maxPlayers + " online"
                + (offlineMode ? ", offline mode" : "") + ")";
    }

    /** @return whether the given name looks like a valid Minecraft username */
    static boolean isValidUsername(String name) {
        return name != null && name.matches("[A-Za-z0-9_]{3,16}");
    }

    /** @return the name lower-cased with the ROOT locale (for maps) */
    static String key(String name) {
        return name.toLowerCase(Locale.ROOT);
    }
}
