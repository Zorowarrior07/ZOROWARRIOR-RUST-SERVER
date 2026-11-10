package io.zorowarrior.zwrs.test;

import static io.zorowarrior.zwrs.test.TestSupport.equal;

import io.zorowarrior.zwrs.OfflineUuid;

/** The Java side of the identity cross-check used by the bridge self-test. */
public final class OfflineUuidTest {

    private OfflineUuidTest() {
    }

    public static void testMatchesKnownVanillaVectors() {
        // These are the values a vanilla server derives for the same names.
        equal("b50ad385-829d-3141-a216-7e7d7539ba7f",
                OfflineUuid.forUsername("Notch").toString(), "Notch");
        equal("a762f560-4fce-3236-812a-b80efff0b62b",
                OfflineUuid.forUsername("jeb_").toString(), "jeb_");
        equal("1eedc501-8469-326d-99d8-7de09a3c0fdf",
                OfflineUuid.forUsername("Zorowarrior").toString(), "Zorowarrior");
    }

    public static void testIsVersion3Variant() {
        java.util.UUID uuid = OfflineUuid.forUsername("AnyName");
        equal(3, uuid.version(), "version 3 UUID");
        equal(2, uuid.variant(), "RFC 4122 variant");
    }

    public static void testIsStableAndNameSensitive() {
        equal(OfflineUuid.forUsername("Case"), OfflineUuid.forUsername("Case"), "deterministic");
        TestSupport.check(!OfflineUuid.forUsername("Case").equals(OfflineUuid.forUsername("case")),
                "offline UUIDs are case sensitive, matching vanilla");
    }
}
