package io.zorowarrior.zwrs.plugin;

import java.io.IOException;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.zip.DataFormatException;
import java.util.zip.Inflater;

/**
 * A minimal, defensive ZIP/JAR reader.
 *
 * <p>Only what plugin discovery needs: iterate the central directory and read
 * one small entry (usually {@code plugin.yml}). Entries are decompressed with
 * {@link Inflater} and every size is validated so a maliciously crafted JAR
 * cannot exhaust memory.</p>
 *
 * <p>Limitations are deliberate: no ZIP64, no encryption, no multi-disk, no
 * data descriptors beyond what the central directory declares. Anything
 * unusual is reported as a problem instead of being guessed at.</p>
 */
final class ZipReader implements AutoCloseable {

    /** Maximum size of an entry this reader will decompress. */
    static final int MAX_ENTRY_BYTES = 4 * 1024 * 1024;
    /** Maximum number of entries inspected. */
    static final int MAX_ENTRIES = 4096;

    /** One central-directory entry. */
    record Entry(String name, int method, long compressedSize, long uncompressedSize, long localHeaderOffset) {
    }

    private final byte[] data;
    private final List<Entry> entries;

    private ZipReader(byte[] data, List<Entry> entries) {
        this.data = data;
        this.entries = entries;
    }

    static ZipReader open(Path jar) throws IOException {
        byte[] data = Files.readAllBytes(jar);
        return open(data);
    }

    static ZipReader open(byte[] data) throws IOException {
        int eocd = findEndOfCentralDirectory(data);
        if (eocd < 0) {
            throw new IOException("not a ZIP/JAR archive (no end-of-central-directory record)");
        }
        int entryCount = readU16(data, eocd + 10);
        long centralSize = readU32(data, eocd + 12);
        long centralOffset = readU32(data, eocd + 16);
        if (entryCount > MAX_ENTRIES) {
            throw new IOException("archive declares " + entryCount + " entries, refusing to scan more than "
                    + MAX_ENTRIES);
        }
        if (centralOffset + centralSize > data.length) {
            throw new IOException("central directory points outside the file");
        }
        if (entryCount == 0xFFFF || centralOffset == 0xFFFFFFFFL) {
            throw new IOException("ZIP64 archives are not supported by this reader");
        }

        List<Entry> entries = new ArrayList<>();
        int pos = (int) centralOffset;
        for (int i = 0; i < entryCount; i++) {
            if (pos + 46 > data.length || readU32(data, pos) != 0x02014b50L) {
                throw new IOException("corrupt central directory entry " + i);
            }
            int method = readU16(data, pos + 10);
            long compressed = readU32(data, pos + 20);
            long uncompressed = readU32(data, pos + 24);
            int nameLen = readU16(data, pos + 28);
            int extraLen = readU16(data, pos + 30);
            int commentLen = readU16(data, pos + 32);
            long localOffset = readU32(data, pos + 42);
            if (pos + 46 + nameLen > data.length) {
                throw new IOException("entry name runs past the end of the file");
            }
            String name = new String(data, pos + 46, nameLen, StandardCharsets.UTF_8);
            entries.add(new Entry(name, method, compressed, uncompressed, localOffset));
            pos += 46 + nameLen + extraLen + commentLen;
        }
        return new ZipReader(data, entries);
    }

    List<Entry> entries() {
        return List.copyOf(entries);
    }

    Entry entry(String name) {
        for (Entry entry : entries) {
            if (entry.name().equals(name)) {
                return entry;
            }
        }
        return null;
    }

    /** Reads and decompresses one entry as text. */
    String readText(String name) throws IOException {
        Entry entry = entry(name);
        if (entry == null) {
            throw new IOException("entry " + name + " not present");
        }
        byte[] bytes = readEntry(entry);
        return new String(bytes, StandardCharsets.UTF_8);
    }

    byte[] readEntry(Entry entry) throws IOException {
        if (entry.uncompressedSize() > MAX_ENTRY_BYTES) {
            throw new IOException("entry " + entry.name() + " is " + entry.uncompressedSize()
                    + " bytes, above the " + MAX_ENTRY_BYTES + " byte limit");
        }
        int offset = (int) entry.localHeaderOffset();
        if (offset + 30 > data.length || readU32(data, offset) != 0x04034b50L) {
            throw new IOException("bad local header for " + entry.name());
        }
        int nameLen = readU16(data, offset + 26);
        int extraLen = readU16(data, offset + 28);
        int start = offset + 30 + nameLen + extraLen;
        int length = (int) entry.compressedSize();
        if (start + length > data.length) {
            throw new IOException("entry " + entry.name() + " runs past the end of the file");
        }
        byte[] raw = new byte[length];
        System.arraycopy(data, start, raw, 0, length);

        if (entry.method() == 0) {
            expectedSize(entry, raw.length);
            return raw;
        }
        if (entry.method() != 8) {
            throw new IOException("unsupported compression method " + entry.method()
                    + " for " + entry.name() + " (only stored and deflate are supported)");
        }
        Inflater inflater = new Inflater(true);
        try {
            inflater.setInput(raw);
            byte[] out = new byte[(int) Math.min(Math.max(entry.uncompressedSize(), 1), MAX_ENTRY_BYTES)];
            int written = 0;
            while (!inflater.finished()) {
                if (written == out.length) {
                    throw new IOException("entry " + entry.name() + " decompresses past its declared size");
                }
                int n = inflater.inflate(out, written, out.length - written);
                if (n == 0) {
                    if (inflater.needsInput() || inflater.needsDictionary()) {
                        throw new IOException("truncated deflate stream in " + entry.name());
                    }
                    throw new IOException("deflate stream made no progress in " + entry.name());
                }
                written += n;
            }
            if (written != entry.uncompressedSize()) {
                throw new IOException("entry " + entry.name() + " declares " + entry.uncompressedSize()
                        + " bytes but decompressed to " + written);
            }
            byte[] exact = new byte[written];
            System.arraycopy(out, 0, exact, 0, written);
            return exact;
        } catch (DataFormatException error) {
            throw new IOException("invalid deflate data in " + entry.name() + ": " + error.getMessage(), error);
        } finally {
            inflater.end();
        }
    }

    private static void expectedSize(Entry entry, int actual) throws IOException {
        if (entry.uncompressedSize() != actual) {
            throw new IOException("entry " + entry.name() + " declares " + entry.uncompressedSize()
                    + " bytes but contains " + actual);
        }
    }

    private static int findEndOfCentralDirectory(byte[] data) {
        int minOffset = Math.max(0, data.length - 22 - 0xFFFF);
        for (int i = data.length - 22; i >= minOffset; i--) {
            if (readU32(data, i) == 0x06054b50L) {
                return i;
            }
        }
        return -1;
    }

    private static int readU16(byte[] data, int offset) {
        if (offset < 0 || offset + 2 > data.length) {
            throw new IndexOutOfBoundsException("read past end of archive at " + offset);
        }
        return (data[offset] & 0xff) | ((data[offset + 1] & 0xff) << 8);
    }

    private static long readU32(byte[] data, int offset) {
        if (offset < 0 || offset + 4 > data.length) {
            return -1;
        }
        return (data[offset] & 0xffL)
                | ((data[offset + 1] & 0xffL) << 8)
                | ((data[offset + 2] & 0xffL) << 16)
                | ((data[offset + 3] & 0xffL) << 24);
    }

    /** Names of all entries, for diagnostics. */
    Map<String, Long> entrySizes() {
        Map<String, Long> sizes = new LinkedHashMap<>();
        for (Entry entry : entries) {
            sizes.put(entry.name(), entry.uncompressedSize());
        }
        return sizes;
    }

    @Override
    public void close() {
        // Nothing to release; kept for try-with-resources symmetry.
    }
}
