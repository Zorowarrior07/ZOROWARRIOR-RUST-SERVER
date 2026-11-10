package io.zorowarrior.zwrs.plugin;

import java.io.PrintStream;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.security.MessageDigest;
import java.security.NoSuchAlgorithmException;
import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Locale;
import java.util.Map;
import java.util.Objects;

import io.zorowarrior.zwrs.Log;

/**
 * Host for Java plugins: discovery, metadata validation and lifecycle logging.
 *
 * <h2>What is verified, and what is not</h2>
 *
 * <p>This host reads and validates plugin manifests, computes checksums, checks
 * API/runtime requirements, resolves dependency order and reports exactly which
 * fields are unsupported. It does <strong>not</strong> yet call into plugins:
 * instantiating a Bukkit plugin requires the Bukkit API classes
 * ({@code org.bukkit.plugin.java.JavaPlugin}, event bus, scheduler, services,
 * and the server singleton) which this project has not implemented. A plugin
 * therefore appears as {@code discovered} or {@code compatible-metadata}, never
 * as {@code loaded}, and the console says so.</p>
 *
 * <p>Available APIs are published through {@link #supportedApis()} so plugin
 * authors and operators can see the real surface instead of a marketing claim.</p>
 */
public final class JavaPluginHost {

    /** Newest Bukkit API level whose descriptor shape this host validates. */
    static final int MAX_VALIDATED_API = 21;

    /** Lifecycle states reported for a plugin artifact. */
    public enum State {
        /** discovered and metadata parsed */
        DISCOVERED,
        /** metadata parsed and API/version checks passed; still not executed */
        METADATA_COMPATIBLE,
        /** present but unusable */
        INCOMPATIBLE
    }

    /** A plugin artifact as reported by the host. */
    public record PluginStatus(
            String name,
            String version,
            String mainClass,
            String apiVersion,
            State state,
            String sha256,
            long sizeBytes,
            Path jar,
            List<String> dependencies,
            List<String> notes,
            List<String> commands) {
    }

    private final List<PluginStatus> discovered = new ArrayList<>();
    private final List<String> problems = new ArrayList<>();
    private final Map<String, Object> services = new LinkedHashMap<>();
    private volatile PrintStream output = System.out;

    /** Injects a stream for plugin console output (used by tests). */
    public void setOutput(PrintStream stream) {
        this.output = Objects.requireNonNull(stream);
    }

    /** Registers a service object a plugin may look up in a future release. */
    public void registerService(String name, Object service) {
        services.put(name, service);
    }

    /** Registered services, by name. */
    public Map<String, Object> services() {
        return Map.copyOf(services);
    }

    /** The Bukkit/Spigot/Paper API surface this project actually implements. */
    public static List<String> supportedApis() {
        return List.of(
                "plugin.yml metadata: name, version, main, api-version, depend, softdepend, loadbefore, load, commands, permissions",
                "dependency ordering and cycle detection",
                "JAR container validation and SHA-256 checksums",
                "console logging from the host");
    }

    /** API areas that are recognised but not implemented. */
    public static List<String> unsupportedApis() {
        return List.of(
                "org.bukkit.* (server, world, player, entity, inventory, event, scheduler, services, configuration)",
                "plugin instantiation, onEnable/onDisable, event delivery and command dispatch",
                "Bukkit/Spigot/Paper version differences");
    }

    /** Loads and validates every {@code .jar} in {@code pluginsDir}. */
    public List<String> discover(Path pluginsDir) {
        discovered.clear();
        problems.clear();
        List<String> lines = new ArrayList<>();
        if (!Files.isDirectory(pluginsDir)) {
            lines.add("plugins directory does not exist: " + pluginsDir);
            return lines;
        }
        List<Path> jars = new ArrayList<>();
        try (var stream = Files.list(pluginsDir)) {
            stream.filter(Files::isRegularFile)
                    .filter(path -> path.getFileName().toString().toLowerCase(Locale.ROOT).endsWith(".jar"))
                    .sorted()
                    .forEach(jars::add);
        } catch (Exception error) {
            problems.add("could not list " + pluginsDir + ": " + error.getMessage());
            lines.addAll(problems);
            return lines;
        }

        if (jars.isEmpty()) {
            lines.add("plugins directory is empty (" + pluginsDir + ")");
            return lines;
        }

        List<PluginDescriptor> descriptors = new ArrayList<>();
        for (Path jar : jars) {
            try {
                long size = Files.size(jar);
                String checksum = sha256(jar);
                PluginYaml.Parsed parsed = PluginYaml.read(jar);
                PluginDescriptor descriptor = parsed.descriptor();
                List<String> notes = new ArrayList<>(parsed.warnings());
                State state = State.METADATA_COMPATIBLE;

                String api = descriptor.apiVersion();
                if (api != null) {
                    Integer level = parseApiLevel(api);
                    if (level == null) {
                        state = State.INCOMPATIBLE;
                        notes.add("api-version `" + api + "` is not a recognised value"
                                + " (expected forms like 1.13, 1.20 or 1.21)");
                    } else if (level > MAX_VALIDATED_API) {
                        state = State.INCOMPATIBLE;
                        notes.add("api-version " + api + " targets a Bukkit API newer than anything"
                                + " this project validates (max 1." + MAX_VALIDATED_API + ")");
                    } else {
                        notes.add("api-version " + api + " is within the validated range");
                    }
                }
                int classVersion = classFileVersion(jar, descriptor.mainClass());
                if (classVersion > 0) {
                    int requiredJava = classVersion - 44;
                    int runningJava = Runtime.version().feature();
                    notes.add("main class is compiled for Java " + requiredJava + " (class file "
                            + classVersion + "), this runtime is Java " + runningJava);
                    if (requiredJava > runningJava) {
                        state = State.INCOMPATIBLE;
                        notes.add("plugin requires a newer Java runtime than the launcher is running");
                    }
                } else {
                    notes.add("main class " + descriptor.mainClass().replace('.', '/') + ".class not found in the JAR");
                    state = State.INCOMPATIBLE;
                }
                notes.add("not executed: the Bukkit API surface required to instantiate this plugin is not implemented");

                discovered.add(new PluginStatus(
                        descriptor.name(),
                        descriptor.version(),
                        descriptor.mainClass(),
                        api,
                        state,
                        checksum,
                        size,
                        jar,
                        descriptor.depend(),
                        List.copyOf(notes),
                        List.copyOf(descriptor.commands().keySet())));
                descriptors.add(descriptor);
                lines.add("plugin " + descriptor.summary() + " [" + state + ", sha256 " + checksum.substring(0, 16) + "]");
                output.println("[plugins] discovered " + descriptor.summary() + " (" + state + ")");
            } catch (Exception error) {
                problems.add(jar.getFileName() + ": " + error.getMessage());
                lines.add("plugin " + jar.getFileName() + " rejected: " + error.getMessage());
            }
        }

        List<String> order = PluginYaml.resolveLoadOrder(descriptors, problems);
        if (!order.isEmpty()) {
            lines.add("dependency load order: " + String.join(" -> ", order));
        }
        lines.add("host status: metadata validation only; no plugin code has been executed");
        return lines;
    }

    /** Status of every discovered plugin. */
    public List<PluginStatus> statuses() {
        return List.copyOf(discovered);
    }

    /** Problems found during discovery. */
    public List<String> problems() {
        return List.copyOf(problems);
    }

    /**
     * Converts a Bukkit {@code api-version} such as {@code 1.20} or {@code 1.20-1}
     * into the Minecraft minor level (20). Returns {@code null} when the value is
     * not a version this host recognises.
     */
    static Integer parseApiLevel(String api) {
        String cleaned = api.trim().toLowerCase(Locale.ROOT);
        int dash = cleaned.indexOf('-');
        String head = dash > 0 ? cleaned.substring(0, dash) : cleaned;
        String[] parts = head.split("\\.");
        try {
            if (parts.length == 1) {
                return Integer.parseInt(parts[0]);
            }
            if (parts.length == 2) {
                int major = Integer.parseInt(parts[0]);
                int minor = Integer.parseInt(parts[1]);
                if (major != 1) {
                    return null;
                }
                return minor;
            }
            return null;
        } catch (NumberFormatException error) {
            return null;
        }
    }

    /** Reads the class-file version of the plugin's main class, or -1. */
    private static int classFileVersion(Path jar, String mainClass) {
        String entry = mainClass.replace('.', '/') + ".class";
        try (ZipReader zip = ZipReader.open(jar)) {
            ZipReader.Entry classEntry = zip.entry(entry);
            if (classEntry == null) {
                return -1;
            }
            byte[] header = zip.readEntry(classEntry);
            if (header.length < 8 || (header[0] & 0xff) != 0xCA || (header[1] & 0xff) != 0xFE
                    || (header[2] & 0xff) != 0xBA || (header[3] & 0xff) != 0xBE) {
                return -1;
            }
            return ((header[6] & 0xff) << 8) | (header[7] & 0xff);
        } catch (Exception error) {
            Log.debug("could not read class file for " + mainClass + ": " + error.getMessage());
            return -1;
        }
    }

    static String sha256(Path file) {
        try {
            MessageDigest digest = MessageDigest.getInstance("SHA-256");
            byte[] bytes = Files.readAllBytes(file);
            byte[] hash = digest.digest(bytes);
            StringBuilder hex = new StringBuilder(hash.length * 2);
            for (byte b : hash) {
                hex.append(Character.forDigit((b >> 4) & 0xf, 16));
                hex.append(Character.forDigit(b & 0xf, 16));
            }
            return hex.toString();
        } catch (NoSuchAlgorithmException | java.io.IOException error) {
            throw new IllegalStateException("could not hash " + file + ": " + error.getMessage(), error);
        }
    }

    /** Rendered plugin report for {@code /plugins} style output. */
    public List<String> report() {
        List<String> lines = new ArrayList<>();
        if (discovered.isEmpty()) {
            lines.add("No Java plugins discovered.");
        } else {
            lines.add(discovered.size() + " Java plugin artifact(s) discovered (none executed):");
            for (PluginStatus status : discovered) {
                lines.add("  - " + status.name() + " v" + status.version() + " [" + status.state() + "]");
                for (String note : status.notes()) {
                    lines.add("      " + note);
                }
            }
        }
        for (String problem : problems) {
            lines.add("  problem: " + problem);
        }
        lines.add("Descriptors are decoded as " + StandardCharsets.UTF_8.name() + ".");
        return lines;
    }
}
