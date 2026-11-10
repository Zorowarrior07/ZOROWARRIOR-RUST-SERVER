package io.zorowarrior.zwrs.test;

import static io.zorowarrior.zwrs.test.TestSupport.contains;
import static io.zorowarrior.zwrs.test.TestSupport.equal;
import static io.zorowarrior.zwrs.test.TestSupport.expectThrows;

import java.util.List;
import java.util.Map;

/** Exercises the launcher's JSON reader against the shapes the bridge produces. */
public final class JsonTest {

    private JsonTest() {
    }

    @SuppressWarnings("unchecked")
    public static void testParsesBridgeDocuments() {
        String text = """
                {
                  "running": true,
                  "address": "0.0.0.0:25565",
                  "online_players": 3,
                  "capabilities": {"java_plugins": "metadata only"},
                  "lines": ["a", "b"]
                }
                """;
        Map<String, Object> doc = io.zorowarrior.zwrs.test.TestSupportBridge.parseObject(text);
        equal(Boolean.TRUE, doc.get("running"), "running flag");
        equal("0.0.0.0:25565", doc.get("address"), "address");
        equal(3.0, doc.get("online_players"), "count");
        Map<String, Object> caps = (Map<String, Object>) doc.get("capabilities");
        equal("metadata only", caps.get("java_plugins"), "nested object");
        List<Object> lines = (List<Object>) doc.get("lines");
        equal(2, lines.size(), "array length");
    }

    public static void testRejectsTrailingContent() {
        expectThrows(IllegalArgumentException.class,
                () -> io.zorowarrior.zwrs.test.TestSupportBridge.parseObject("{} junk"),
                "trailing content must be rejected");
    }

    public static void testRejectsDuplicateKeys() {
        expectThrows(IllegalArgumentException.class,
                () -> io.zorowarrior.zwrs.test.TestSupportBridge.parseObject("{\"a\":1,\"a\":2}"),
                "duplicate keys must be rejected");
    }

    public static void testHandlesEscapesAndUnicode() {
        String text = "{\"v\":\"line\\nbreak \\\"quoted\\\" \\u00e9\"}";
        Map<String, Object> doc = io.zorowarrior.zwrs.test.TestSupportBridge.parseObject(text);
        equal("line\nbreak \"quoted\" \u00e9", doc.get("v"), "escapes");
    }

    public static void testQuotingRoundTrips() {
        String original = "a \"quoted\" \\ value\nwith newline";
        String quoted = io.zorowarrior.zwrs.test.TestSupportBridge.quote(original);
        Map<String, Object> doc = io.zorowarrior.zwrs.test.TestSupportBridge.parseObject("{\"v\":" + quoted + "}");
        equal(original, doc.get("v"), "quote/parse round trip");
        contains(quoted, "\\\"", "quotes are escaped");
    }
}
