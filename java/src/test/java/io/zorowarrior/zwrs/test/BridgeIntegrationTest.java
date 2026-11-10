package io.zorowarrior.zwrs.test;

import static io.zorowarrior.zwrs.test.TestSupport.contains;
import static io.zorowarrior.zwrs.test.TestSupport.equal;

import java.io.IOException;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.List;
import java.util.Map;

import io.zorowarrior.zwrs.NativeBridge;

/**
 * Real Java -> Rust -> Java integration tests.
 *
 * <p>Requires the release engine library; the runner sets
 * {@code -Dzwrs.native.path} before starting. When the library is unavailable
 * the tests fail loudly rather than silently passing, because a bridge that
 * cannot be exercised is not evidence of a working bridge.</p>
 */
public final class BridgeIntegrationTest {

    private static boolean started;
    private static Path serverDir;

    private BridgeIntegrationTest() {
    }

    private static void ensureLoaded() throws IOException {
        String path = System.getProperty("zwrs.native.path");
        TestSupport.check(path != null, "tests must run with -Dzwrs.native.path set");
        TestSupport.check(Files.isRegularFile(Path.of(path)),
                "native library not found at " + path);
        NativeBridge.load(path);
    }

    public static void testBridgeVersionMatches() throws IOException {
        ensureLoaded();
        equal(NativeBridge.EXPECTED_BRIDGE_VERSION, NativeBridge.nativeBridgeVersion(),
                "bridge version");
    }

    public static void testBuildInfoDescribesTheEngine() throws IOException {
        ensureLoaded();
        Map<String, Object> info = NativeBridge.buildInfo();
        contains(io.zorowarrior.zwrs.Json.string(info, "product", "").toLowerCase(), "zoro",
                "product name");
        TestSupport.check(io.zorowarrior.zwrs.Json.number(info, "max_protocol", 0) > 0,
                "max protocol must be positive");
        TestSupport.check(io.zorowarrior.zwrs.Json.string(info, "platform", "").contains("-"),
                "platform is os-arch");
    }

    public static void testSelfTestAgreesWithJavaUuidDerivation() throws IOException {
        ensureLoaded();
        Map<String, Object> result = NativeBridge.selfTest("BridgeProbe");
        String expected = io.zorowarrior.zwrs.OfflineUuid.forUsername("BridgeProbe").toString();
        equal(expected, io.zorowarrior.zwrs.Json.string(result, "offline_uuid", ""),
                "Rust and Java must derive the same offline UUID");
        equal("BridgeProbe", io.zorowarrior.zwrs.Json.string(result, "echo", ""), "echo");
    }

    public static void testLifecycleStartCommandStateStop() throws IOException {
        ensureLoaded();
        serverDir = TestSupport.tempDir("bridge");
        Map<String, Object> start = NativeBridge.start(serverDir.toString(), 0, false);
        started = true;
        String address = io.zorowarrior.zwrs.Json.string(start, "address", "");
        contains(address, ":", "bound address");

        Map<String, Object> state = NativeBridge.state();
        equal(Boolean.TRUE, state.get("running"), "engine reports running");
        equal(address, io.zorowarrior.zwrs.Json.string(state, "address", ""), "same address");

        NativeBridge.CommandResult status = NativeBridge.command("/status");
        TestSupport.check(status.recognised(), "/status is recognised");
        contains(String.join("\n", status.lines()), "Players: 0/", "status shows real player counts");

        NativeBridge.CommandResult unknown = NativeBridge.command("/nonsense");
        TestSupport.check(!unknown.recognised(), "unknown commands are reported as such");

        Map<String, Object> stop = NativeBridge.stop();
        started = false;
        equal(Boolean.TRUE, stop.get("stopped"), "engine stopped");
        equal(Boolean.FALSE, NativeBridge.state().get("running"), "engine reports stopped");

        Map<String, Object> stopAgain = NativeBridge.stop();
        equal(Boolean.FALSE, stopAgain.get("stopped"), "second stop is a no-op");

        // The engine created the documented data layout.
        Path playersFile = serverDir.resolve("data").resolve("players").resolve("players.json");
        TestSupport.check(Files.isRegularFile(playersFile),
                "player database created at " + playersFile);
        String text = Files.readString(playersFile);
        contains(text, "\"format\"", "player database has a schema version");
        TestSupport.check(Files.isRegularFile(serverDir.resolve("config").resolve("zwrs.properties")),
                "default configuration file written by the engine");
    }

    public static void testCommandsFailCleanlyWhenStopped() throws IOException {
        ensureLoaded();
        if (started) {
            NativeBridge.stop();
            started = false;
        }
        try {
            NativeBridge.command("/status");
            throw new AssertionError("expected IllegalStateException while the engine is stopped");
        } catch (IllegalStateException expected) {
            contains(expected.getMessage(), "not running", "explains the failure");
        }
    }

    public static void testStartingTwiceIsRefused() throws IOException {
        ensureLoaded();
        Path dir = TestSupport.tempDir("bridge2");
        NativeBridge.start(dir.toString(), 0, false);
        try {
            NativeBridge.start(dir.toString(), 0, false);
            throw new AssertionError("expected IllegalStateException for a second start");
        } catch (IllegalStateException expected) {
            contains(expected.getMessage(), "already running", "explains the conflict");
        } finally {
            NativeBridge.stop();
        }
    }

    public static void testInvalidArgumentsAreRejected() throws IOException {
        ensureLoaded();
        Path dir = TestSupport.tempDir("bridge3");
        try {
            NativeBridge.nativeStart(dir.toString(), 70000, false);
            throw new AssertionError("expected an exception for an out-of-range port");
        } catch (RuntimeException expected) {
            contains(expected.getMessage(), "port", "mentions the port");
        }
        try {
            NativeBridge.nativeStart(null, 0, false);
            throw new AssertionError("expected an exception for a null directory");
        } catch (RuntimeException expected) {
            contains(expected.getMessage(), "directory", "mentions the directory");
        }
    }

    public static void testOnlineModeRefusesLoginsButStillReportsStatus() throws IOException {
        ensureLoaded();
        Path dir = TestSupport.tempDir("bridge4");
        Map<String, Object> start = NativeBridge.start(dir.toString(), 0, true);
        try {
            equal(Boolean.TRUE, start.get("online_mode"), "online mode recorded");
            String caps = String.valueOf(io.zorowarrior.zwrs.Json.object(start, "capabilities")
                    .get("minecraft_login"));
            contains(caps, "NOT implemented", "capabilities admit missing online authentication");
            List<String> status = NativeBridge.command("/status").lines();
            contains(String.join("\n", status), "online mode", "status names the auth mode");
        } finally {
            NativeBridge.stop();
        }
    }
}
