package io.zorowarrior.zwrs;

import java.io.File;
import java.io.IOException;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.Paths;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.List;
import java.util.Locale;

/**
 * Locates and loads the native Rust engine for the current OS/architecture.
 *
 * <p>Search order, first hit wins:</p>
 * <ol>
 *   <li>an explicit path from {@code --native=<path>} or {@code -Dzwrs.native.path}</li>
 *   <li>{@code <server dir>/native/<os>-<arch>/<library name>}</li>
 *   <li>{@code <server dir>/native/<library name>}</li>
 *   <li>inside the JAR itself: {@code /native/<os>-<arch>/<library name>}, extracted
 *       to a private temporary directory with a checksum-verified copy</li>
 *   <li>the system library path ({@code zwrs_jni})</li>
 * </ol>
 *
 * <p>Nothing is executed before {@link #load} is called; extraction refuses to
 * overwrite an existing file, uses a per-process directory and checks the
 * extracted bytes against the resource stream before loading.</p>
 */
final class NativeLibraryLoader {

    /** Result of a successful load. */
    record Loaded(String path, String how) {
    }

    private NativeLibraryLoader() {
    }

    static String libraryFileName() {
        String os = System.getProperty("os.name", "").toLowerCase(Locale.ROOT);
        if (os.contains("win")) {
            return "zwrs_jni.dll";
        }
        if (os.contains("mac")) {
            return "libzwrs_jni.dylib";
        }
        return "libzwrs_jni.so";
    }

    /** Normalised {@code os}-{@code arch} tag used for artifact directories. */
    static String platformTag() {
        String os = System.getProperty("os.name", "").toLowerCase(Locale.ROOT);
        String arch = System.getProperty("os.arch", "").toLowerCase(Locale.ROOT);
        String osTag;
        if (os.contains("android")) {
            osTag = "android";
        } else if (os.contains("win")) {
            osTag = "windows";
        } else if (os.contains("mac") || os.contains("darwin")) {
            osTag = "macos";
        } else {
            osTag = "linux";
        }
        String archTag = switch (arch) {
            case "aarch64", "arm64" -> "aarch64";
            case "x86_64", "amd64", "x64" -> "x86_64";
            case "arm", "armv7l", "armv7" -> "armv7";
            default -> arch.replaceAll("[^a-z0-9_]", "");
        };
        return osTag + "-" + archTag;
    }

    private static boolean isAndroid() {
        String os = System.getProperty("os.name", "").toLowerCase(Locale.ROOT);
        String vm = System.getProperty("java.vm.vendor", "").toLowerCase(Locale.ROOT);
        return os.contains("android") || vm.contains("android");
    }

    private static List<String> checkAndroid(String platformTag) {
        List<String> warnings = new ArrayList<>();
        if (!isAndroid()) {
            return warnings;
        }
        warnings.add("Android/Termux detected (" + platformTag + "). Termux uses the Bionic libc:"
                + " a glibc-built linux .so will NOT load here. Build the engine with the Android"
                + " NDK target (aarch64-linux-android).");
        return warnings;
    }

    static Loaded load(String explicitPath) {
        String libraryName = libraryFileName();
        String platformTag = platformTag();
        List<String> notes = checkAndroid(platformTag);
        for (String note : notes) {
            Log.warn(note);
        }

        File serverDir = Paths.get(System.getProperty("user.dir", ".")).toFile();
        List<String> candidates = new ArrayList<>();
        if (explicitPath != null && !explicitPath.isBlank()) {
            candidates.add(explicitPath);
        }
        String property = System.getProperty("zwrs.native.path");
        if (property != null && !property.isBlank()) {
            candidates.add(property);
        }
        candidates.add(new File(new File(serverDir, "native"), platformTag + File.separator + libraryName).getPath());
        candidates.add(new File(new File(serverDir, "native"), libraryName).getPath());
        candidates.add(new File(new File(new File(serverDir, "native"), System.getProperty("os.name").toLowerCase(Locale.ROOT)), libraryName).getPath());

        List<String> tried = new ArrayList<>();
        for (String candidate : candidates) {
            Path path = Paths.get(candidate);
            if (!Files.isRegularFile(path)) {
                tried.add(candidate + " (missing)");
                continue;
            }
            try {
                System.load(path.toAbsolutePath().toString());
                return new Loaded(path.toAbsolutePath().toString(), "filesystem");
            } catch (UnsatisfiedLinkError error) {
                tried.add(candidate + " (" + error.getMessage() + ")");
            }
        }

        // Fall back to a library embedded in the JAR.
        String resource = "/native/" + platformTag + "/" + libraryName;
        try (var stream = NativeLibraryLoader.class.getResourceAsStream(resource)) {
            if (stream != null) {
                Path extracted = extractVerified(stream, platformTag, libraryName);
                System.load(extracted.toAbsolutePath().toString());
                return new Loaded(extracted.toAbsolutePath().toString(), "embedded in the JAR");
            }
            tried.add("classpath:" + resource + " (not packaged)");
        } catch (IOException error) {
            tried.add("classpath:" + resource + " (" + error.getMessage() + ")");
        }

        try {
            System.loadLibrary("zwrs_jni");
            return new Loaded("zwrs_jni", "java.library.path");
        } catch (UnsatisfiedLinkError error) {
            tried.add("java.library.path (" + error.getMessage() + ")");
        }

        throw new IllegalStateException(buildHelpMessage(platformTag, libraryName, tried));
    }

    private static String buildHelpMessage(String platformTag, String libraryName, List<String> tried) {
        StringBuilder message = new StringBuilder();
        message.append("Could not load the ZOROWARRIOR Rust engine (").append(libraryName)
                .append(") for ").append(platformTag).append(".\n");
        message.append("Searched:\n");
        for (String entry : tried) {
            message.append("  - ").append(entry).append('\n');
        }
        message.append("\nFix one of the following:\n");
        message.append("  * put the library at native/").append(platformTag).append('/')
                .append(libraryName).append('\n');
        message.append("  * pass --native=/absolute/path/to/").append(libraryName).append('\n');
        message.append("  * set -Dzwrs.native.path=/absolute/path/to/").append(libraryName).append('\n');
        message.append("  * build it: cargo build --release -p zwrs-jni\n");
        message.append("The Rust engine is required: the JAR is only the launcher.");
        return message.toString();
    }

    private static Path extractVerified(java.io.InputStream stream, String platformTag, String libraryName)
            throws IOException {
        Path directory = Files.createTempDirectory("zwrs-native-");
        directory.toFile().deleteOnExit();
        Path target = directory.resolve(libraryName);
        byte[] bytes = stream.readAllBytes();
        if (bytes.length == 0) {
            throw new IOException("embedded native library is empty");
        }
        Files.write(target, bytes);
        target.toFile().deleteOnExit();
        long onDisk = Files.size(target);
        if (onDisk != bytes.length) {
            throw new IOException("extracted " + onDisk + " bytes but packaged " + bytes.length);
        }
        if (!target.toFile().setExecutable(true) && !Files.isExecutable(target)) {
            // Not fatal: shared libraries do not need the executable bit.
            Log.debug("could not mark " + target + " executable");
        }
        Log.info("extracted embedded engine to " + target + " (" + bytes.length + " bytes, "
                + platformTag + ")");
        return target;
    }

    /**
     * Loads the engine from an explicit classpath resource (used by tests to
     * verify that a JAR-packaged engine is extracted and verified correctly).
     *
     * @param resource absolute resource path, e.g. {@code /native/test/libzwrs_jni.so}
     */
    static Loaded loadFromResource(String resource, String platformTag) throws IOException {
        String libraryName = libraryFileName();
        try (var stream = NativeLibraryLoader.class.getResourceAsStream(resource)) {
            if (stream == null) {
                throw new IOException("resource not found: " + resource);
            }
            Path extracted = extractVerified(stream, platformTag, libraryName);
            System.load(extracted.toAbsolutePath().toString());
            return new Loaded(extracted.toAbsolutePath().toString(), "resource " + resource);
        }
    }

    /** Diagnostics for {@code --diagnose}. */
    static List<String> diagnostics() {
        List<String> lines = new ArrayList<>();
        lines.add("java.version        = " + System.getProperty("java.version"));
        lines.add("java.vm.name        = " + System.getProperty("java.vm.name"));
        lines.add("java.vm.vendor      = " + System.getProperty("java.vm.vendor"));
        lines.add("os.name             = " + System.getProperty("os.name"));
        lines.add("os.arch             = " + System.getProperty("os.arch"));
        lines.add("os.version          = " + System.getProperty("os.version"));
        lines.add("jnu.arch            = " + System.getProperty("sun.arch.data.model", "?") + "-bit");
        lines.add("platform tag        = " + platformTag());
        lines.add("library file        = " + libraryFileName());
        lines.add("user.dir            = " + System.getProperty("user.dir"));
        lines.add("java.library.path   = " + System.getProperty("java.library.path"));
        lines.add("android             = " + isAndroid());
        lines.add("availableProcessors = " + Runtime.getRuntime().availableProcessors());
        lines.add("maxMemoryMB         = " + Runtime.getRuntime().maxMemory() / (1024 * 1024));
        try {
            Path nativeDir = Paths.get(System.getProperty("user.dir", "."), "native");
            if (Files.isDirectory(nativeDir)) {
                lines.add("native/ contents    = " + Arrays.toString(nativeDir.toFile().list()));
            } else {
                lines.add("native/ contents    = (no native directory)");
            }
        } catch (RuntimeException error) {
            lines.add("native/ contents    = (unreadable: " + error.getMessage() + ")");
        }
        return lines;
    }
}
