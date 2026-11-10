package io.zorowarrior.zwrs;

import java.util.List;
import java.util.Map;

/**
 * Java side of the JNI bridge.
 *
 * <p>Every method returns JSON or a primitive. Failures arrive as Java
 * exceptions thrown from the native code, so callers can use ordinary
 * try/catch. See {@code docs/ARCHITECTURE.md} for the contract and versioning
 * rules.</p>
 */
public final class NativeBridge {

    /** Bridge contract version this Java code was written against. */
    public static final int EXPECTED_BRIDGE_VERSION = 2;

    static boolean loaded;

    private NativeBridge() {
    }

    /**
     * Loads the native library, verifies the bridge version and fails with an
     * actionable message when anything is missing.
     *
     * @param explicitPath optional path from {@code --native} or a system property
     * @throws IllegalStateException when the engine cannot be loaded
     */
    public static void load(String explicitPath) {
        if (loaded) {
            return;
        }
        NativeLibraryLoader.Loaded loadedLibrary = NativeLibraryLoader.load(explicitPath);
        Log.info("native bridge loaded from " + loadedLibrary.path()
                + " (" + loadedLibrary.how() + ")");
        int version = nativeBridgeVersion();
        if (version != EXPECTED_BRIDGE_VERSION) {
            throw new IllegalStateException("native bridge version " + version
                    + " does not match the launcher's expected version "
                    + EXPECTED_BRIDGE_VERSION
                    + "; use the JAR and native library from the same release");
        }
        loaded = true;
    }

    public static native int nativeBridgeVersion();

    public static native String nativeBuildInfo();

    public static native String nativeSelfTest(String username);

    public static native String nativeStart(String directory, int port, boolean onlineMode);

    public static native String nativeCommand(String line);

    public static native String nativeStop();

    public static native String nativeState();

    /**
     * Installs the Java plugin host so player events and plugin commands reach it.
     *
     * @param hooks the hook object (io.zorowarrior.zwrs.plugin.EngineHooks statics)
     * @return the bridge reply
     */
    public static native String nativeInstallHost(Object hooks);

    /** Detaches the plugin host; later events are dropped. */
    public static native String nativeRemoveHost();

    // ---- typed helpers -----------------------------------------------------

    /** Result of a console command. */
    public record CommandResult(List<String> lines, boolean keepRunning, boolean recognised) {
    }

    public static Map<String, Object> buildInfo() {
        return Json.parseObject(nativeBuildInfo());
    }

    public static Map<String, Object> selfTest(String username) {
        return Json.parseObject(nativeSelfTest(username));
    }

    public static Map<String, Object> start(String directory, int port, boolean onlineMode) {
        return Json.parseObject(nativeStart(directory, port, onlineMode));
    }

    public static CommandResult command(String line) {
        Map<String, Object> doc = Json.parseObject(nativeCommand(line));
        List<String> lines = Json.array(doc, "lines").stream()
                .filter(String.class::isInstance)
                .map(String.class::cast)
                .toList();
        return new CommandResult(lines,
                Json.bool(doc, "keep_running", true),
                Json.bool(doc, "recognised", false));
    }

    public static Map<String, Object> state() {
        return Json.parseObject(nativeState());
    }

    public static Map<String, Object> stop() {
        return Json.parseObject(nativeStop());
    }

    /**
     * Installs the plugin host.
     *
     * @param hooks the hook class instance
     * @return the bridge reply
     */
    public static Map<String, Object> installHost(Object hooks) {
        return Json.parseObject(nativeInstallHost(hooks));
    }

    /** Detaches the plugin host. */
    public static Map<String, Object> removeHost() {
        return Json.parseObject(nativeRemoveHost());
    }
}
