package io.zorowarrior.zwrs;

import java.nio.charset.StandardCharsets;
import java.security.MessageDigest;
import java.security.NoSuchAlgorithmException;
import java.util.UUID;

/**
 * The vanilla offline-mode UUID derivation, implemented independently on the
 * Java side so the bridge self-test can cross-check the Rust implementation.
 *
 * <p>Vanilla uses {@code UUID.nameUUIDFromBytes(("OfflinePlayer:" + name).getBytes(UTF_8))},
 * which is MD5 with the RFC 4122 version-3 and variant bits forced. This class
 * reproduces that result without depending on the Rust code.</p>
 */
public final class OfflineUuid {

    private OfflineUuid() {
    }

    public static UUID forUsername(String username) {
        try {
            MessageDigest digest = MessageDigest.getInstance("MD5");
            byte[] hash = digest.digest(("OfflinePlayer:" + username).getBytes(StandardCharsets.UTF_8));
            hash[6] = (byte) ((hash[6] & 0x0f) | 0x30);
            hash[8] = (byte) ((hash[8] & 0x3f) | 0x80);
            long msb = 0;
            long lsb = 0;
            for (int i = 0; i < 8; i++) {
                msb = (msb << 8) | (hash[i] & 0xff);
            }
            for (int i = 8; i < 16; i++) {
                lsb = (lsb << 8) | (hash[i] & 0xff);
            }
            return new UUID(msb, lsb);
        } catch (NoSuchAlgorithmException error) {
            // MD5 is mandatory in every Java platform.
            throw new IllegalStateException("MD5 is unavailable in this JVM", error);
        }
    }
}
