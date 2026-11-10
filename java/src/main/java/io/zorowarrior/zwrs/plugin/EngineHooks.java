package io.zorowarrior.zwrs.plugin;

import java.util.List;
import java.util.Map;

import io.zorowarrior.zwrs.Json;
import io.zorowarrior.zwrs.Log;

/**
 * Receives player events from the Rust engine and runs plugin code.
 *
 * <p>This class is called <strong>from native threads</strong>: the engine
 * attaches each of its threads to the JVM and calls one of the static methods
 * below with a JSON payload. It must therefore be:</p>
 *
 * <ul>
 *   <li>static and synchronous (the native side expects the call to return);</li>
 *   <li>defensive: a malformed payload is logged and ignored, never thrown, so a
 *       protocol surprise cannot poison a JNI call;</li>
 *   <li>fast: heavy work blocks the engine's connection thread.</li>
 * </ul>
 *
 * <p>Returning {@code null} from {@link #onCommand(String)} means "not mine"; the
 * engine then reports the command as unknown.</p>
 */
public final class EngineHooks {

    private static volatile PluginRuntime runtime;
    private static volatile ServerImpl server;
    private static volatile String lastError;

    private EngineHooks() {
    }

    /**
     * Installs the runtime that receives events.
     *
     * @param pluginRuntime the runtime, or {@code null} to detach
     * @param serverImpl the server handed to plugins
     */
    public static void install(PluginRuntime pluginRuntime, ServerImpl serverImpl) {
        runtime = pluginRuntime;
        server = serverImpl;
    }

    /** @return the runtime currently installed, or {@code null} */
    public static PluginRuntime runtime() {
        return runtime;
    }

    /** @return the last payload that could not be parsed, or {@code null} */
    public static String lastError() {
        return lastError;
    }

    /**
     * Called by the engine after a login is admitted and persisted.
     *
     * @param payload JSON event document
     */
    public static void onPlayerJoin(String payload) {
        handle(payload, true);
    }

    /**
     * Called by the engine after a session ended and playtime was credited.
     *
     * @param payload JSON event document
     */
    public static void onPlayerQuit(String payload) {
        handle(payload, false);
    }

    private static void handle(String payload, boolean join) {
        PluginRuntime active = runtime;
        if (active == null) {
            return;
        }
        if (payload == null) {
            // Defensive: a null here means the native side passed no payload. It is
            // a bug in the bridge, so say so instead of failing with an NPE deep in
            // string concatenation.
            lastError = "received a null " + (join ? "join" : "quit") + " payload from the engine";
            Log.error("plugins: " + lastError);
            return;
        }
        Log.debug("plugins: raw " + (join ? "join" : "quit") + " payload (" + payload.length()
                + " chars): " + payload);
        try {
            Map<String, Object> event = Json.parseObject(payload);
            ServerImpl current = server;
            if (current == null) {
                return;
            }
            PlayerImpl player = join ? current.track(event) : current.untrack(event);
            if (player == null) {
                return;
            }
            if (join) {
                active.fireJoin(player);
            } else {
                active.fireQuit(player);
            }
        } catch (RuntimeException | Error e) {
            java.io.StringWriter trace = new java.io.StringWriter();
            e.printStackTrace(new java.io.PrintWriter(trace));
            lastError = "could not process the " + (join ? "join" : "quit")
                    + " payload: " + e + "\n" + trace;
            Log.error("plugins: " + lastError);
        }
    }

    /**
     * Called by the engine for a console command no built-in matched.
     *
     * @param payload JSON {@code {command, args}}
     * @return JSON array of output lines, or {@code null} when no plugin owns it
     */
    public static String onCommand(String payload) {
        PluginRuntime active = runtime;
        if (active == null) {
            return null;
        }
        try {
            Map<String, Object> request = Json.parseObject(payload);
            Object command = request.get("command");
            if (!(command instanceof String name)) {
                return null;
            }
            List<String> args = new java.util.ArrayList<>();
            if (request.get("args") instanceof List<?> raw) {
                for (Object value : raw) {
                    args.add(String.valueOf(value));
                }
            }
            ServerImpl current = server;
            List<String> lines = active.dispatchCommand(name, args,
                    current == null ? null : current.consoleSender());
            if (lines == null) {
                return null;
            }
            return Json.toJsonArray(lines);
        } catch (RuntimeException e) {
            lastError = "could not run a plugin command from " + payload + ": " + e;
            Log.error("plugins: " + lastError);
            return Json.toJsonArray(List.of("The plugin command failed: " + e.getMessage()));
        }
    }
}
