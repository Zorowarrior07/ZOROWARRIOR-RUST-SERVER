package io.zorowarrior.zwrs.plugin;

import java.io.File;

import io.zorowarrior.zwrs.Log;
import java.lang.reflect.Constructor;
import java.lang.reflect.InvocationTargetException;
import java.lang.reflect.Method;
import java.lang.reflect.Modifier;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.Comparator;
import java.util.HashMap;
import java.util.LinkedHashMap;
import java.util.LinkedHashSet;
import java.util.List;
import java.util.Locale;
import java.util.Map;
import java.util.Objects;
import java.util.Set;
import java.util.logging.Level;
import java.util.logging.Logger;

import org.bukkit.Player;
import org.bukkit.Server;
import org.bukkit.command.Command;
import org.bukkit.command.CommandSender;
import org.bukkit.event.Event;
import org.bukkit.event.EventPriority;
import org.bukkit.event.Listener;
import org.bukkit.event.player.PlayerJoinEvent;
import org.bukkit.event.player.PlayerQuitEvent;
import org.bukkit.plugin.Plugin;
import org.bukkit.plugin.PluginDescriptionFile;
import org.bukkit.plugin.PluginManager;
import org.bukkit.event.EventHandler;
import org.bukkit.plugin.java.JavaPlugin;
import org.bukkit.plugin.java.JavaPluginLoader;
import org.bukkit.plugin.java.PluginClassLoader;

/**
 * Loads, enables, drives and disables Java plugins.
 *
 * <h2>Lifecycle</h2>
 *
 * <pre>
 * discover -> validate -> load (instantiate) -> enable -> [events/commands] -> disable
 * </pre>
 *
 * Each stage is reported with its real outcome: a plugin that fails validation is
 * {@code INCOMPATIBLE}, one that throws in its constructor is {@code FAILED}, and
 * only a plugin whose {@code onEnable()} returned normally is {@code ENABLED}.
 * Nothing is ever reported as loaded without an instance, and nothing is reported
 * as enabled without a successful {@code onEnable()}.
 *
 * <h2>Failure isolation</h2>
 *
 * A plugin that throws is disabled and its error recorded; the remaining plugins
 * load and the server keeps running. Plugin code never unwinds into the launcher.
 */
public final class PluginRuntime implements PluginManager {

    /** Lifecycle state of one plugin artifact. */
    public enum State {
        /** discovered, descriptor not parsed */
        DISCOVERED,
        /** descriptor parsed, checks passed, not instantiated */
        VALIDATED,
        /** instance created, {@code onLoad} and {@code onEnable} returned normally */
        ENABLED,
        /** present but unusable (bad descriptor, missing class, unsupported Java) */
        INCOMPATIBLE,
        /** failed at load or enable time; see the recorded error */
        FAILED,
        /** was enabled, shut down cleanly */
        DISABLED
    }

    /** One plugin as the runtime sees it. */
    public static final class LoadedPlugin {
        private final PluginDescriptor descriptor;
        private final Path jar;
        private State state;
        private String error;
        private JavaPlugin instance;
        private PluginClassLoader classLoader;
        private Logger logger;
        private final List<String> notes = new ArrayList<>();
        private final Map<String, Command> commands = new LinkedHashMap<>();

        LoadedPlugin(PluginDescriptor descriptor, Path jar) {
            this.descriptor = descriptor;
            this.jar = jar;
            this.state = State.DISCOVERED;
        }

        /** @return the parsed plugin.yml descriptor */
        public PluginDescriptor descriptor() {
            return descriptor;
        }

        /** @return the plugin name from plugin.yml */
        public String name() {
            return descriptor.name();
        }

        /** @return the plugin version from plugin.yml */
        public String version() {
            return descriptor.version();
        }

        /** @return the main class from plugin.yml */
        public String mainClass() {
            return descriptor.main();
        }

        /** @return the JAR */
        public Path jar() {
            return jar;
        }

        /** @return the current lifecycle state */
        public State state() {
            return state;
        }

        /** @return the failure message, or {@code null} */
        public String error() {
            return error;
        }

        /** @return the live plugin instance, or {@code null} */
        public JavaPlugin instance() {
            return instance;
        }

        /** @return informational notes (unsupported API areas, for example) */
        public List<String> notes() {
            return List.copyOf(notes);
        }

        /** @return commands this plugin registered */
        public Map<String, Command> commands() {
            return Map.copyOf(commands);
        }

        /** @return whether the plugin is running */
        public boolean isEnabled() {
            return state == State.ENABLED;
        }

        /** @return a one-line status for the console */
        public String describe() {
            StringBuilder out = new StringBuilder();
            out.append(descriptor.name()).append(" v").append(descriptor.version())
                    .append(" - ").append(state);
            if (error != null) {
                out.append(" (").append(error).append(')');
            }
            return out.toString();
        }
    }

    /** A registered event handler. */
    private static final class Handler {
        private final Listener listener;
        private final Method method;
        private final EventPriority priority;
        private final Plugin plugin;
        private final long order;

        Handler(Listener listener, Method method, EventPriority priority, Plugin plugin, long order) {
            this.listener = listener;
            this.method = method;
            this.priority = priority;
            this.plugin = plugin;
            this.order = order;
        }
    }

    private final Map<String, LoadedPlugin> plugins = new LinkedHashMap<>();
    private final Map<Class<?>, List<Handler>> handlers = new HashMap<>();
    private final Map<String, Command> commands = new LinkedHashMap<>();
    private final Map<String, Plugin> commandOwners = new HashMap<>();
    private final List<String> problems = new ArrayList<>();
    private final Logger serverLogger = Logger.getLogger("ZWRS");
    private final JavaPluginLoader loader = new HostLoader();
    private long handlerOrder;
    private Server server;
    private Path pluginsDir;

    /** Creates a runtime; logging goes through {@link Log}. */
    public PluginRuntime() {
    }

    /** @return the plugin manager handed to plugins */
    public PluginManager pluginManager() {
        return this;
    }

    /** @return every discovered plugin, in load order */
    public List<LoadedPlugin> plugins() {
        return List.copyOf(plugins.values());
    }

    /** @return problems found while discovering or loading */
    public List<String> problems() {
        return List.copyOf(problems);
    }

    /** @return how many plugins are currently enabled */
    public int enabledCount() {
        long count = plugins.values().stream().filter(LoadedPlugin::isEnabled).count();
        return (int) count;
    }

    /**
     * Discovers, validates and loads every plugin in {@code dir}.
     *
     * @param dir the plugins directory
     * @param server the server handed to plugins
     */
    public void loadAll(Path dir, Server server) {
        this.pluginsDir = dir;
        this.server = server;
        List<Path> jars = listJars(dir);
        if (jars.isEmpty()) {
            Log.info("plugins: no plugin JARs in " + dir);
            return;
        }

        // 1. Parse descriptors and validate them.
        List<LoadedPlugin> candidates = new ArrayList<>();
        for (Path jar : jars) {
            LoadedPlugin loaded;
            try {
                PluginDescriptor descriptor = PluginYaml.read(jar).descriptor();
                loaded = new LoadedPlugin(descriptor, jar);
                String reason = validate(descriptor);
                if (reason != null) {
                    loaded.state = State.INCOMPATIBLE;
                    loaded.error = reason;
                    problems.add(descriptor.name() + ": " + reason);
                } else {
                    loaded.state = State.VALIDATED;
                }
            } catch (java.io.IOException | RuntimeException e) {
                String name = jar.getFileName().toString();
                problems.add(name + ": " + e.getMessage());
                Log.warn("plugins: " + name + " could not be read: " + e.getMessage());
                continue;
            }
            candidates.add(loaded);
        }

        // 2. Order by dependency depth so dependencies enable first.
        List<LoadedPlugin> ordered = order(candidates);
        for (LoadedPlugin loaded : ordered) {
            plugins.put(loaded.descriptor.name().toLowerCase(Locale.ROOT), loaded);
        }

        // 3. Load and enable in order.
        for (LoadedPlugin loaded : ordered) {
            if (loaded.state != State.VALIDATED) {
                continue;
            }
            Set<String> unmet = missingDependencies(loaded);
            if (!unmet.isEmpty()) {
                loaded.state = State.INCOMPATIBLE;
                loaded.error = "missing required dependency: " + String.join(", ", unmet);
                problems.add(loaded.descriptor.name() + ": " + loaded.error);
                continue;
            }
            enable(loaded);
        }
        int failed = (int) plugins.values().stream()
                .filter(p -> p.state == State.FAILED || p.state == State.INCOMPATIBLE).count();
        Log.info(String.format(Locale.ROOT,
                "plugins: %d discovered, %d enabled, %d not running",
                plugins.size(), enabledCount(), failed));
    }

    /** Lists plugin JARs, sorted by file name, ignoring anything else. */
    private static List<Path> listJars(Path dir) {
        if (!Files.isDirectory(dir)) {
            return List.of();
        }
        try (var stream = Files.list(dir)) {
            return stream.filter(path -> path.getFileName().toString().toLowerCase(Locale.ROOT)
                            .endsWith(".jar"))
                    .sorted()
                    .toList();
        } catch (java.io.IOException e) {
            return List.of();
        }
    }

    /** Checks the parts of a descriptor that determine whether loading is possible. */
    private String validate(PluginDescriptor descriptor) {
        if (descriptor.main().isEmpty()) {
            return "plugin.yml has no main class";
        }
        String api = descriptor.apiVersion();
        if (api != null) {
            int level = JavaPluginHost.parseApiLevel(api);
            if (level < 0) {
                return "api-version '" + api + "' is not a 1.x version";
            }
            if (level > JavaPluginHost.MAX_VALIDATED_API) {
                return "api-version " + api + " is newer than this server supports (1."
                        + JavaPluginHost.MAX_VALIDATED_API + ")";
            }
        }
        return null;
    }

    private Set<String> missingDependencies(LoadedPlugin loaded) {
        Set<String> missing = new LinkedHashSet<>();
        for (String dependency : loaded.descriptor.depend()) {
            LoadedPlugin provider = plugins.get(dependency.toLowerCase(Locale.ROOT));
            if (provider == null || !provider.isEnabled()) {
                missing.add(dependency);
            }
        }
        return missing;
    }

    /** Orders plugins so that a dependency always precedes its dependents. */
    private List<LoadedPlugin> order(List<LoadedPlugin> candidates) {
        Map<String, LoadedPlugin> byName = new LinkedHashMap<>();
        for (LoadedPlugin candidate : candidates) {
            byName.put(candidate.descriptor.name().toLowerCase(Locale.ROOT), candidate);
        }
        List<PluginDescriptor> descriptors = new ArrayList<>();
        for (LoadedPlugin candidate : candidates) {
            descriptors.add(candidate.descriptor());
        }
        List<String> orderingProblems = new ArrayList<>();
        List<String> orderedNames = PluginYaml.resolveLoadOrder(descriptors, orderingProblems);
        problems.addAll(orderingProblems);
        List<LoadedPlugin> ordered = new ArrayList<>();
        Set<String> seen = new LinkedHashSet<>();
        for (String name : orderedNames) {
            LoadedPlugin plugin = byName.get(name.toLowerCase(Locale.ROOT));
            if (plugin != null && seen.add(name.toLowerCase(Locale.ROOT))) {
                ordered.add(plugin);
            }
        }
        for (LoadedPlugin plugin : candidates) {
            if (seen.add(plugin.descriptor.name().toLowerCase(Locale.ROOT))) {
                ordered.add(plugin);
            }
        }
        return ordered;
    }

    private void enable(LoadedPlugin loaded) {
        PluginDescriptor descriptor = loaded.descriptor();
        loaded.logger = new PluginLogger(descriptor.name());
        try {
            Set<String> classes = PluginJar.entries(loaded.jar()).keySet().stream()
                    .filter(name -> name.endsWith(".class"))
                    .map(name -> name.substring(0, name.length() - ".class".length())
                            .replace('/', '.'))
                    .collect(java.util.stream.Collectors.toSet());
            if (!classes.contains(descriptor.main())) {
                throw new IllegalStateException(
                        "main class " + descriptor.main() + " is not in the JAR");
            }
            loaded.classLoader = new PluginClassLoader(this.loader, descriptor.name(),
                    loaded.jar, classes);
            Class<?> mainClass = Class.forName(descriptor.main(), true, loaded.classLoader);
            if (!JavaPlugin.class.isAssignableFrom(mainClass)) {
                throw new IllegalStateException(descriptor.main()
                        + " does not extend org.bukkit.plugin.java.JavaPlugin");
            }
            if (Modifier.isAbstract(mainClass.getModifiers())) {
                throw new IllegalStateException(descriptor.main() + " is abstract");
            }
            Constructor<?> constructor = mainClass.getDeclaredConstructor();
            constructor.setAccessible(true);
            JavaPlugin plugin = (JavaPlugin) constructor.newInstance();

            File dataFolder = pluginsDir.resolve(descriptor.name()).toFile();
            plugin.initialize(server, descriptionOf(descriptor), loader, loaded.classLoader,
                    dataFolder, loaded.jar.toFile(), loaded.logger);
            loaded.instance = plugin;

            callHook(loaded, plugin, "onLoad");
            plugin.setEnabled(true);
            callHook(loaded, plugin, "onEnable");
            loaded.state = State.ENABLED;
            loaded.notes.addAll(unsupportedNotes());
            Log.info("plugins: enabled " + descriptor.name() + " v" + descriptor.version()
                    + " (" + descriptor.main() + ")");
        } catch (Throwable failure) {
            Throwable cause = failure instanceof InvocationTargetException
                    ? ((InvocationTargetException) failure).getTargetException()
                    : failure;
            loaded.state = State.FAILED;
            loaded.error = describe(cause);
            problems.add(descriptor.name() + ": " + loaded.error);
            Log.error("plugins: " + descriptor.name() + " failed to load: " + loaded.error);
            if (loaded.instance != null) {
                disableQuietly(loaded);
            }
        }
    }

    private void callHook(LoadedPlugin loaded, JavaPlugin plugin, String hook) throws Exception {
        try {
            Method method = JavaPlugin.class.getMethod(hook);
            method.invoke(plugin);
        } catch (InvocationTargetException e) {
            Throwable cause = e.getTargetException();
            throw new InvocationTargetException(cause, loaded.descriptor().name() + "." + hook + "()");
        }
    }

    private static String describe(Throwable error) {
        String message = error.getMessage();
        return message == null || message.isBlank()
                ? error.getClass().getName()
                : error.getClass().getSimpleName() + ": " + message;
    }

    private void disableQuietly(LoadedPlugin loaded) {
        if (loaded.instance == null) {
            return;
        }
        try {
            if (loaded.instance.isEnabled()) {
                loaded.instance.onDisable();
            }
        } catch (Throwable e) {
            Log.warn("plugins: " + loaded.descriptor().name() + " threw in onDisable: "
                    + describe(e));
        } finally {
            loaded.instance.setEnabled(false);
            if (loaded.state == State.ENABLED) {
                loaded.state = State.DISABLED;
            }
            if (loaded.classLoader != null) {
                try {
                    loaded.classLoader.close();
                } catch (java.io.IOException ignored) {
                    // Closing a plugin loader is best-effort.
                }
            }
        }
    }

    /** Disables every enabled plugin, in reverse load order. */
    public void disableAll() {
        List<LoadedPlugin> all = new ArrayList<>(plugins.values());
        for (int i = all.size() - 1; i >= 0; i--) {
            LoadedPlugin loaded = all.get(i);
            if (loaded.isEnabled()) {
                Log.info("plugins: disabling " + loaded.descriptor().name());
                disableQuietly(loaded);
            }
        }
    }

    // ---------------------------------------------------------------- events

    @Override
    public void registerEvents(Listener listener, Plugin plugin) {
        Objects.requireNonNull(listener, "listener");
        Objects.requireNonNull(plugin, "plugin");
        int registered = 0;
        for (Method method : listener.getClass().getMethods()) {
            EventHandler annotation = method.getAnnotation(EventHandler.class);
            if (annotation == null) {
                continue;
            }
            if (method.getParameterCount() != 1
                    || !Event.class.isAssignableFrom(method.getParameterTypes()[0])) {
                throw new IllegalArgumentException(plugin.getName() + ": "
                        + method.getName() + " has @EventHandler but does not take exactly one Event");
            }
            if (!method.canAccess(listener) && !method.trySetAccessible()) {
                throw new IllegalArgumentException(plugin.getName() + ": " + method.getName()
                        + " is not accessible to the server");
            }
            @SuppressWarnings("unchecked")
            Class<? extends Event> eventClass =
                    (Class<? extends Event>) method.getParameterTypes()[0];
            handlers.computeIfAbsent(eventClass, key -> new ArrayList<>())
                    .add(new Handler(listener, method, annotation.priority(), plugin, handlerOrder++));
            registered++;
        }
        if (registered == 0) {
            Log.warn("plugins: " + plugin.getName() + " registered a listener with no @EventHandler methods");
        }
    }

    @Override
    public <T extends Event> void registerEvent(Class<T> eventClass, Listener listener,
            EventPriority priority, Plugin plugin) {
        for (Method method : listener.getClass().getMethods()) {
            EventHandler annotation = method.getAnnotation(EventHandler.class);
            if (annotation != null && method.getParameterCount() == 1
                    && method.getParameterTypes()[0].isAssignableFrom(eventClass)) {
                handlers.computeIfAbsent(eventClass, key -> new ArrayList<>())
                        .add(new Handler(listener, method, priority, plugin, handlerOrder++));
                return;
            }
        }
        throw new IllegalArgumentException(plugin.getName() + ": no @EventHandler method for "
                + eventClass.getSimpleName() + " in " + listener.getClass().getName());
    }

    @Override
    public void callEvent(Event event) {
        Objects.requireNonNull(event, "event");
        List<Handler> matched = new ArrayList<>();
        for (Map.Entry<Class<?>, List<Handler>> entry : handlers.entrySet()) {
            if (entry.getKey().isInstance(event)) {
                matched.addAll(entry.getValue());
            }
        }
        matched.sort(Comparator.comparing((Handler h) -> h.priority.ordinal())
                .thenComparingLong(h -> h.order));
        for (Handler handler : matched) {
            if (handler.plugin instanceof JavaPlugin && !((JavaPlugin) handler.plugin).isEnabled()) {
                continue;
            }
            try {
                handler.method.setAccessible(true);
                handler.method.invoke(handler.listener, event);
            } catch (InvocationTargetException e) {
                Throwable cause = e.getTargetException();
                Log.error("plugins: " + handler.plugin.getName() + " threw in "
                        + handler.method.getName() + " while handling "
                        + event.getClass().getSimpleName() + ": " + describe(cause));
                failPlugin(handler.plugin, cause);
            } catch (ReflectiveOperationException e) {
                Log.error("plugins: could not invoke " + handler.method + ": " + e);
            }
        }
    }

    /** Disables a plugin whose handler threw, so it cannot keep corrupting state. */
    private void failPlugin(Plugin plugin, Throwable cause) {
        if (!(plugin instanceof JavaPlugin)) {
            return;
        }
        LoadedPlugin loaded = plugins.get(plugin.getName().toLowerCase(Locale.ROOT));
        if (loaded == null || loaded.state != State.ENABLED) {
            return;
        }
        Log.error("plugins: disabling " + plugin.getName()
                + " after an unhandled exception in a handler (" + describe(cause) + ")");
        loaded.error = "disabled after an exception in an event handler: " + describe(cause);
        problems.add(plugin.getName() + ": " + loaded.error);
        disableQuietly(loaded);
        loaded.state = State.FAILED;
    }

    @Override
    public boolean isEventRegistered(Class<? extends Event> eventClass) {
        for (Map.Entry<Class<?>, List<Handler>> entry : handlers.entrySet()) {
            if (entry.getKey().isAssignableFrom(eventClass) && !entry.getValue().isEmpty()) {
                return true;
            }
        }
        return false;
    }

    // -------------------------------------------------------------- commands

    @Override
    public boolean registerCommand(String name, Plugin plugin) {
        String key = name.toLowerCase(Locale.ROOT);
        if (commands.containsKey(key)) {
            return false;
        }
        Command command = new Command(key) {
            @Override
            public boolean execute(CommandSender sender, String label, String[] args) {
                sender.sendMessage("The plugin command /" + key + " has no implementation.");
                return true;
            }
        };
        commands.put(key, command);
        commandOwners.put(key, plugin);
        return true;
    }

    @Override
    public boolean registerCommand(Command command, Plugin plugin) {
        String key = command.getName().toLowerCase(Locale.ROOT);
        if (commands.containsKey(key)) {
            return false;
        }
        for (String alias : command.getAliases()) {
            String aliasKey = alias.toLowerCase(Locale.ROOT);
            if (commands.containsKey(aliasKey)) {
                return false;
            }
        }
        commands.put(key, command);
        commandOwners.put(key, plugin);
        for (String alias : command.getAliases()) {
            commands.put(alias.toLowerCase(Locale.ROOT), command);
            commandOwners.put(alias.toLowerCase(Locale.ROOT), plugin);
        }
        LoadedPlugin loaded = plugins.get(plugin.getName().toLowerCase(Locale.ROOT));
        if (loaded != null) {
            loaded.commands.put(key, command);
        }
        return true;
    }

    @Override
    public List<String> getRegisteredCommands() {
        return commands.keySet().stream().sorted().toList();
    }

    @Override
    public Plugin getCommandOwner(String command) {
        return commandOwners.get(command.toLowerCase(Locale.ROOT));
    }

    /**
     * Runs a plugin command on behalf of the console.
     *
     * @param command command name, no slash
     * @param args arguments
     * @param sender console sender
     * @return the lines produced, or {@code null} when no plugin owns the name
     */
    public List<String> dispatchCommand(String command, List<String> args, CommandSender sender) {
        Command target = commands.get(command.toLowerCase(Locale.ROOT));
        if (target == null) {
            return null;
        }
        Plugin owner = commandOwners.get(command.toLowerCase(Locale.ROOT));
        CapturingSender capture = new CapturingSender(sender);
        try {
            boolean handled = target.execute(capture, command, args.toArray(new String[0]));
            if (!handled) {
                capture.sendMessage(target.getUsage());
            }
        } catch (Throwable e) {
            Log.error("plugins: /" + command + " threw "
                    + (owner == null ? "" : "in " + owner.getName() + ": ") + describe(e));
            capture.sendMessage("The command /" + command + " failed: " + describe(e));
            if (owner != null) {
                failPlugin(owner, e);
            }
        }
        return capture.lines();
    }

    /** Collects messages a command sends instead of printing them twice. */
    private static final class CapturingSender implements CommandSender {
        private final CommandSender delegate;
        private final List<String> lines = new ArrayList<>();

        CapturingSender(CommandSender delegate) {
            this.delegate = delegate;
        }

        List<String> lines() {
            return lines;
        }

        @Override
        public void sendMessage(String message) {
            if (message == null) {
                return;
            }
            // Capture only. The launcher prints the captured lines exactly once
            // after the command returns; delegating here as well would print every
            // plugin message twice.
            for (String line : message.split("\n", -1)) {
                lines.add(line);
            }
        }

        @Override
        public String getName() {
            return delegate == null ? "CONSOLE" : delegate.getName();
        }

        @Override
        public boolean isConsole() {
            return delegate == null || delegate.isConsole();
        }

        @Override
        public boolean dispatchCommand(String line) {
            if (delegate == null) {
                return false;
            }
            return delegate.dispatchCommand(line);
        }
    }

    // ------------------------------------------------------- engine bridges

    /**
     * Delivers a join from the Rust engine to plugin listeners.
     *
     * @param player the player
     * @return whether any handler ran
     */
    public boolean fireJoin(Player player) {
        if (!isEventRegistered(PlayerJoinEvent.class)) {
            return false;
        }
        PlayerJoinEvent event = new PlayerJoinEvent(player, player.getName() + " joined the game");
        callEvent(event);
        return true;
    }

    /**
     * Delivers a quit from the Rust engine to plugin listeners.
     *
     * @param player the player
     * @return whether any handler ran
     */
    public boolean fireQuit(Player player) {
        if (!isEventRegistered(PlayerQuitEvent.class)) {
            return false;
        }
        PlayerQuitEvent event = new PlayerQuitEvent(player, player.getName() + " left the game");
        callEvent(event);
        return true;
    }

    /** @return a summary suitable for the console and {@code /plugins} */
    public List<String> report() {
        List<String> lines = new ArrayList<>();
        if (plugins.isEmpty()) {
            lines.add("No Java plugins were found in " + (pluginsDir == null ? "plugins/" : pluginsDir));
        }
        for (LoadedPlugin loaded : plugins.values()) {
            lines.add(loaded.describe());
        }
        for (String problem : problems) {
            lines.add("problem: " + problem);
        }
        return lines;
    }

    /** @return API areas this runtime implements, for {@code /plugins} */
    public static List<String> supportedApis() {
        return List.of(
                "plugin.yml parsing (name, version, main, api-version, depend, softdepend, commands, permissions)",
                "class loading with per-plugin isolation and a server-first Bukkit API",
                "JavaPlugin lifecycle: constructor, onLoad, onEnable, onDisable",
                "PluginManager.registerEvents and synchronous event dispatch in priority order",
                "PlayerJoinEvent and PlayerQuitEvent fired from the Rust engine",
                "Command registration and console dispatch with a real CommandSender",
                "Bukkit.getServer/getPluginManager/getLogger, per-plugin data folders and logging",
                "failure isolation: a throwing plugin is disabled, not fatal");
    }

    /** @return API areas that are still missing, stated plainly */
    public static List<String> unsupportedNotes() {
        return List.of("org.bukkit world/inventory/entity/chat APIs are not implemented; "
                + "play-state packets are still missing");
    }

    private PluginDescriptionFile descriptionOf(PluginDescriptor descriptor) {
        return new PluginDescriptionFile(descriptor.name(), descriptor.version(),
                descriptor.main(), descriptor.apiVersion(), descriptor.description(),
                descriptor.authors(), descriptor.depend(), descriptor.softDepend(),
                descriptor.loadBefore(), null, descriptor.commandDefinitions());
    }

    /** Loader implementation handed to plugins. */
    private final class HostLoader extends JavaPluginLoader {

        @Override
        public void log(Plugin plugin, String level, String message) {
            String prefix = plugin.getName() + ": ";
            switch (level == null ? "INFO" : level.toUpperCase(Locale.ROOT)) {
                case "WARN" -> Log.warn(prefix + message);
                case "ERROR" -> Log.error(prefix + message);
                default -> Log.info(prefix + message);
            }
        }

        @Override
        public Plugin instantiate(String pluginName, String mainClass) {
            LoadedPlugin loaded = plugins.get(pluginName.toLowerCase(Locale.ROOT));
            if (loaded == null) {
                return null;
            }
            return loaded.instance;
        }

        @Override
        public PluginClassLoader getClassLoader(String pluginName) {
            LoadedPlugin loaded = plugins.get(pluginName.toLowerCase(Locale.ROOT));
            return loaded == null ? null : loaded.classLoader;
        }

        @Override
        public Plugin getPlugin(String name) {
            LoadedPlugin loaded = plugins.get(name.toLowerCase(Locale.ROOT));
            return loaded == null ? null : loaded.instance;
        }

        @Override
        public Command commandFor(String name, String command) {
            LoadedPlugin loaded = plugins.get(name.toLowerCase(Locale.ROOT));
            if (loaded == null) {
                throw new IllegalArgumentException("plugin " + name + " is not loaded");
            }
            Command found = loaded.commands.get(command.toLowerCase(Locale.ROOT));
            if (found == null) {
                found = commands.get(command.toLowerCase(Locale.ROOT));
            }
            if (found == null) {
                throw new IllegalArgumentException(
                        "plugin " + name + " has not registered /" + command);
            }
            return found;
        }
    }

    /** Logger used for plugin output; prefixes every line with the plugin name. */
    private static final class PluginLogger extends Logger {

        PluginLogger(String name) {
            super("plugins." + name, null);
            // No parent, no global handlers: every line goes to the server log with
            // exactly one prefix, and nothing leaks into stderr behind our back.
            // (Logger forbids setParent(null) on a fresh instance, so the resource
            // bundle argument is what detaches it.)
            setLevel(Level.ALL);
            setUseParentHandlers(false);
        }

        @Override
        public void log(Level level, String message) {
            String text = "[" + getName().substring("plugins.".length()) + "] " + message;
            if (level.intValue() >= Level.SEVERE.intValue()) {
                Log.error(text);
            } else if (level.intValue() >= Level.WARNING.intValue()) {
                Log.warn(text);
            } else {
                Log.info(text);
            }
        }

        @Override
        public void info(String message) {
            Log.info("[" + getName().substring("plugins.".length()) + "] " + message);
        }

        @Override
        public void warning(String message) {
            Log.warn("[" + getName().substring("plugins.".length()) + "] " + message);
        }

        @Override
        public void severe(String message) {
            Log.error("[" + getName().substring("plugins.".length()) + "] " + message);
        }
    }

    /** @return whether the directory exists and contains plugin JARs */
    public static boolean hasPlugins(Path dir) {
        if (!Files.isDirectory(dir)) {
            return false;
        }
        try (var stream = Files.list(dir)) {
            return stream.anyMatch(path -> path.getFileName().toString().endsWith(".jar"));
        } catch (java.io.IOException e) {
            return false;
        }
    }
}
