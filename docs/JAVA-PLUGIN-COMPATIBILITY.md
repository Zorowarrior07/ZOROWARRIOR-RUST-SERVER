# Java plugin compatibility

## Summary

| Capability | State |
|---|---|
| Discover `.jar` files in `plugins/` | **working** |
| Validate the JAR container | **working** (ZIP structure, entry bounds, deflate limits) |
| Parse and validate `plugin.yml` | **working** |
| Check `api-version` and class-file version | **working** |
| Dependency ordering and cycles | **working** |
| SHA-256 per artifact | **working** |
| **Load the plugin class in an isolated class loader** | **working** |
| **Instantiate it and call `onLoad()` / `onEnable()`** | **working** |
| **`PluginManager.registerEvents` + event dispatch in priority order** | **working** |
| **`PlayerJoinEvent` / `PlayerQuitEvent` from the Rust engine** | **working** |
| **Command registration and console dispatch (`Command`, `CommandSender`)** | **working** |
| **`Bukkit.getServer()` / `getPluginManager()` / logger / data folder** | **working** |
| **Failure isolation (a throwing plugin is disabled, not fatal)** | **working** |
| Worlds, blocks, entities, inventories, chat delivery | **not implemented** |
| Scheduler (`runTask*`), services, `FileConfiguration` tree | **not implemented** |
| Permissions at runtime, `PlayerMoveEvent` and the rest of the event catalogue | **not implemented** |

This is a **compatibility subset**, not "Bukkit support". A plugin that only uses
the rows marked working will run. A plugin that needs worlds, inventories,
schedulers or the full event catalogue will fail at class load or method
resolution, and the failure is reported with the class or method name.

## The API surface that exists

```text
org.bukkit.Bukkit                    getServer, getPluginManager, getLogger
org.bukkit.Server                    name, version, motd, limits, online players,
                                     dispatchCommand, callEvent, offline mode, uptime
org.bukkit.Player                    name, uuid, address, protocol, session, isOnline,
                                     sendMessage
org.bukkit.plugin.Plugin             onLoad/onEnable/onDisable, description, data folder,
                                     logger, loader, name
org.bukkit.plugin.PluginDescriptionFile   name, version, main, api-version, authors,
                                     depend, softdepend, loadbefore, commands
org.bukkit.plugin.PluginManager      registerEvents, registerEvent, callEvent,
                                     isEventRegistered, registerCommand, getRegisteredCommands,
                                     getCommandOwner
org.bukkit.plugin.java.JavaPlugin    getServer, getName, getDataFolder, getLogger,
                                     getConfigFile/readConfigText, saveResource, getCommand
org.bukkit.plugin.java.PluginClassLoader, JavaPluginLoader
org.bukkit.event.*                   Event, Cancellable, Listener, EventPriority, EventHandler
org.bukkit.event.player.*            PlayerEvent, PlayerJoinEvent, PlayerQuitEvent
org.bukkit.command.*                 Command, CommandSender
```

`@EventHandler` lives in `org.bukkit.event` exactly as in Bukkit, so an existing
plugin's imports work unchanged.

## Lifecycle and guarantees

```text
plugin.yml -> validate -> class load -> new() -> onLoad() -> onEnable()
                                     -> [events, commands] -> onDisable()
```

* A plugin is reported `ENABLED` **only** after `onEnable()` returned normally.
* An exception in the constructor, `onLoad()` or `onEnable()` marks the plugin
  `FAILED`, records the real exception message, disables it and continues with
  the other plugins. The server keeps running.
* An exception inside an event handler disables **that plugin** and logs the
  stack trace; other plugins keep receiving events.
* `onDisable()` runs before the engine closes its sockets, so a plugin can still
  talk to the server while shutting down.
* Events arrive **after** the durable state is written: a join handler never sees
  a profile that was not persisted, and playtime is credited before `quit`.
* Each plugin gets `plugins/<Name>/` as its data folder, created before
  `onEnable()`.

## Class loading

* One `PluginClassLoader` per plugin. Plugin classes come from the plugin's JAR
  first, so two plugins may ship different versions of the same library.
* `org.bukkit.*` (the API above) and the JDK always come from the server, so a
  plugin cannot shadow the API or replace `java.lang` classes.
* A class that is neither in the plugin, the API nor the JDK produces:
  `plugin '<name>' could not load <class>: it is not in the plugin JAR, the server
  API or the JDK. Bundle the library inside the plugin JAR.`

## Writing a plugin for ZWRS

```java
package com.example;

import org.bukkit.event.EventHandler;
import org.bukkit.event.Listener;
import org.bukkit.event.player.PlayerJoinEvent;
import org.bukkit.plugin.java.JavaPlugin;

public final class HelloPlugin extends JavaPlugin implements Listener {
    @Override
    public void onEnable() {
        getServer().getPluginManager().registerEvents(this, this);
        getLogger().info("Hello from " + getName());
    }

    @EventHandler
    public void onJoin(PlayerJoinEvent event) {
        getLogger().info(event.getPlayer().getName() + " joined with "
                + event.getPlayer().getUniqueId());
    }
}
```

```yaml
name: Hello
version: 1.0.0
main: com.example.HelloPlugin
api-version: 1.20
```

```bash
javac -cp ZOROWARRIOR-Rust-Server.jar -d classes src/com/example/HelloPlugin.java
jar cf plugins/Hello.jar -C classes .
```

Drop the JAR in `plugins/` and start the server. `scripts/acceptance_plugins.py`
is a complete, working example of exactly this flow (it compiles the plugin, runs
the server, logs in and asserts on what the plugin observed).

## What a plugin still cannot do

* Move, teleport or inspect players: there are no play-state packets, so
  `Player` exposes identity and session data only.
* Deliver chat to clients: `Player.sendMessage` writes to the console and says so.
* Use the scheduler, services or `FileConfiguration`: read `config.yml` with
  `readConfigText()` and your own parser.
* Listen for events other than join and quit: those are the only two the engine
  can raise today.
