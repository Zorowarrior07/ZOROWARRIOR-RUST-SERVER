package io.zorowarrior.zwrs.test.fixture;

import java.util.ArrayList;
import java.util.List;

/**
 * Shared observation point for fixtures.
 *
 * <p>A plugin is loaded by its own class loader, so its static fields are a
 * <em>different</em> copy from the test's. This class is not inside the plugin
 * JAR, so the plugin loader delegates it to the server loader and both sides see
 * the same instance. That is exactly how a plugin would use a server-side
 * service, and it lets the test observe real plugin behaviour instead of trusting
 * a reference to a class copy the plugin never touched.</p>
 */
public final class Recorder {

    /** Ordered event log written by fixtures. */
    public static final List<String> EVENTS = new ArrayList<>();

    /** Set by fixtures in {@code onEnable()}. */
    public static boolean enabled;

    /** Set by fixtures in {@code onDisable()}. */
    public static boolean disabled;

    private Recorder() {
    }

    /** Clears everything (called by the test, not by the plugin). */
    public static void reset() {
        EVENTS.clear();
        enabled = false;
        disabled = false;
    }
}
