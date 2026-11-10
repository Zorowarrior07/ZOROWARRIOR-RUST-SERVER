package io.zorowarrior.zwrs.test;

import java.util.Map;

/**
 * Test-only access to the launcher's package-private JSON helpers.
 *
 * <p>Tests live in a separate package so they see only the public launcher API;
 * this bridge re-exports the two helpers the tests need.</p>
 */
public final class TestSupportBridge {

    private TestSupportBridge() {
    }

    public static Map<String, Object> parseObject(String text) {
        return io.zorowarrior.zwrs.Json.parseObject(text);
    }

    public static String quote(String value) {
        return io.zorowarrior.zwrs.Json.quote(value);
    }
}
