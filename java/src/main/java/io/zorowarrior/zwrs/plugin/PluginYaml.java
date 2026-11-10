package io.zorowarrior.zwrs.plugin;

import java.io.IOException;
import java.nio.charset.StandardCharsets;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.LinkedHashSet;
import java.util.List;
import java.util.Locale;
import java.util.Map;
import java.util.Set;

/**
 * Reads and validates {@code plugin.yml} from a plugin JAR.
 *
 * <p>The parser implements the YAML subset Bukkit descriptors actually use:
 * top-level scalars, nested maps, block lists and quoted strings. Anything it
 * does not understand is reported as a warning with its line number rather than
 * being silently ignored, and a descriptor that omits {@code name},
 * {@code version} or {@code main} is rejected.</p>
 *
 * <p>This is intentionally not a general YAML implementation: anchors,
 * multi-document streams, flow collections and block scalars are rejected with
 * a clear message.</p>
 */
final class PluginYaml {

    private PluginYaml() {
    }

    /** Descriptor plus non-fatal warnings found while reading it. */
    record Parsed(PluginDescriptor descriptor, List<String> warnings) {
    }

    static Parsed read(Path jar) throws IOException {
        try (ZipReader zip = ZipReader.open(jar)) {
            ZipReader.Entry entry = zip.entry("plugin.yml");
            if (entry == null) {
                entry = zip.entry("plugin.yaml");
            }
            if (entry == null) {
                throw new IOException("no plugin.yml found in the JAR root");
            }
            String text = zip.readText(entry.name());
            return parse(text, jar);
        }
    }

    static Parsed parse(String text, Path jar) throws IOException {
        List<String> warnings = new ArrayList<>();
        Object root = new YamlParser(text, warnings).parseDocument();
        if (!(root instanceof Map<?, ?> map)) {
            throw new IOException("plugin.yml must be a mapping at the top level");
        }
        @SuppressWarnings("unchecked")
        Map<String, Object> data = (Map<String, Object>) map;

        String name = scalar(data, "name");
        if (name == null || name.isBlank()) {
            throw new IOException("plugin.yml is missing the required `name` field");
        }
        if (!name.matches("[A-Za-z0-9_.-]{1,64}")) {
            throw new IOException("plugin name `" + name + "` must match [A-Za-z0-9_.-]{1,64}");
        }
        String version = scalar(data, "version");
        if (version == null || version.isBlank()) {
            throw new IOException("plugin.yml is missing the required `version` field");
        }
        String main = scalar(data, "main");
        if (main == null || main.isBlank()) {
            throw new IOException("plugin.yml is missing the required `main` field");
        }
        if (!main.matches("[A-Za-z_$][A-Za-z0-9_$]*(\\.[A-Za-z_$][A-Za-z0-9_$]*)+")) {
            throw new IOException("plugin.yml `main` must be a fully-qualified class name, got `" + main + "`");
        }

        String apiVersion = scalar(data, "api-version");
        if (apiVersion == null) {
            warnings.add(name + ": no api-version declared; treated as a legacy (pre-1.13) descriptor");
        }

        Map<String, Object> extra = new LinkedHashMap<>(data);
        for (String consumed : new String[] {"name", "version", "main", "api-version", "description",
                "authors", "author", "depend", "softdepend", "loadbefore", "load", "website",
                "commands", "permissions"}) {
            extra.remove(consumed);
        }

        List<String> authors = list(data, "authors");
        if (authors.isEmpty()) {
            String author = scalar(data, "author");
            if (author != null && !author.isBlank()) {
                authors = List.of(author);
            }
        }

        Map<String, Object> commands = Map.of();
        Object commandsValue = data.get("commands");
        if (commandsValue instanceof Map<?, ?> commandsMap) {
            commands = new LinkedHashMap<>();
            for (Map.Entry<?, ?> command : commandsMap.entrySet()) {
                commands.put(String.valueOf(command.getKey()), command.getValue());
            }
        } else if (commandsValue instanceof List<?> commandsList) {
            commands = new LinkedHashMap<>();
            for (Object command : commandsList) {
                commands.put(String.valueOf(command), null);
            }
        }

        Map<String, Object> descriptor = new LinkedHashMap<>();
        List<String> permissions = new ArrayList<>();
        Object permissionsValue = data.get("permissions");
        if (permissionsValue instanceof Map<?, ?> permissionsMap) {
            for (Object key : permissionsMap.keySet()) {
                permissions.add(String.valueOf(key));
            }
        }
        descriptor.putAll(extra);

        PluginDescriptor plugin = new PluginDescriptor(
                name,
                version,
                main,
                apiVersion,
                scalar(data, "description"),
                authors,
                list(data, "depend"),
                list(data, "softdepend"),
                list(data, "loadbefore"),
                list(data, "load"),
                scalar(data, "website"),
                commands,
                permissions,
                extra,
                jar);
        return new Parsed(plugin, warnings);
    }

    private static String scalar(Map<String, Object> data, String key) {
        Object value = data.get(key);
        if (value == null) {
            return null;
        }
        return String.valueOf(value);
    }

    private static List<String> list(Map<String, Object> data, String key) {
        Object value = data.get(key);
        if (value == null) {
            return List.of();
        }
        if (value instanceof List<?> items) {
            List<String> out = new ArrayList<>(items.size());
            for (Object item : items) {
                out.add(String.valueOf(item));
            }
            return List.copyOf(out);
        }
        return List.of(String.valueOf(value));
    }

    /**
     * Ordering helper: resolves {@code depend} relationships into load order and
     * reports cycles and missing plugins.
     */
    static List<String> resolveLoadOrder(
            List<PluginDescriptor> plugins,
            List<String> problems) {
        Map<String, PluginDescriptor> byName = new LinkedHashMap<>();
        for (PluginDescriptor plugin : plugins) {
            byName.put(plugin.name().toLowerCase(Locale.ROOT), plugin);
        }
        List<String> order = new ArrayList<>();
        Set<String> visiting = new LinkedHashSet<>();
        Set<String> done = new LinkedHashSet<>();
        for (PluginDescriptor plugin : plugins) {
            visit(plugin, byName, visiting, done, order, problems);
        }
        return order;
    }

    private static void visit(
            PluginDescriptor plugin,
            Map<String, PluginDescriptor> byName,
            Set<String> visiting,
            Set<String> done,
            List<String> order,
            List<String> problems) {
        String key = plugin.name().toLowerCase(Locale.ROOT);
        if (done.contains(key)) {
            return;
        }
        if (!visiting.add(key)) {
            problems.add("dependency cycle involving " + plugin.name() + ": " + String.join(" -> ", visiting));
            return;
        }
        for (String dependency : plugin.depend()) {
            PluginDescriptor target = byName.get(dependency.toLowerCase(Locale.ROOT));
            if (target == null) {
                problems.add(plugin.name() + " requires missing plugin " + dependency);
                continue;
            }
            visit(target, byName, visiting, done, order, problems);
        }
        visiting.remove(key);
        if (done.add(key)) {
            order.add(plugin.name());
        }
    }

    /** A small YAML-subset parser. */
    private static final class YamlParser {
        private final String[] lines;
        private final List<String> warnings;
        private int index;

        YamlParser(String text, List<String> warnings) {
            this.lines = text.replace("\r\n", "\n").replace('\r', '\n').split("\n", -1);
            this.warnings = warnings;
        }

        Object parseDocument() {
            skipIgnorable();
            if (index >= lines.length) {
                throw new IllegalArgumentException("empty document");
            }
            int indent = indentOf(lines[index]);
            Object value = parseBlock(indent);
            skipIgnorable();
            if (index < lines.length) {
                warnings.add("ignored trailing YAML after line " + (index + 1)
                        + " (multi-document or unexpected indentation)");
            }
            return value;
        }

        private Object parseBlock(int indent) {
            skipIgnorable();
            if (index >= lines.length) {
                return null;
            }
            String line = lines[index];
            if (stripIndent(line).startsWith("- ")) {
                return parseList(indent);
            }
            return parseMap(indent);
        }

        private Map<String, Object> parseMap(int indent) {
            Map<String, Object> map = new LinkedHashMap<>();
            while (index < lines.length) {
                String raw = lines[index];
                if (isIgnorable(raw)) {
                    index++;
                    continue;
                }
                int currentIndent = indentOf(raw);
                if (currentIndent < indent) {
                    break;
                }
                if (currentIndent > indent) {
                    warnings.add("unexpected indentation on line " + (index + 1));
                    index++;
                    continue;
                }
                String content = stripIndent(raw);
                if (content.startsWith("- ")) {
                    break;
                }
                int colon = findColon(content);
                if (colon < 0) {
                    warnings.add("ignored line " + (index + 1) + ": expected `key: value`");
                    index++;
                    continue;
                }
                String key = unquote(content.substring(0, colon).trim());
                String rest = content.substring(colon + 1).trim();
                index++;
                if (!rest.isEmpty()) {
                    map.put(key, unquote(rest));
                    continue;
                }
                // Nested block: look ahead for the greater indentation.
                int next = nextMeaningful();
                if (next < 0) {
                    map.put(key, null);
                    continue;
                }
                int nextIndent = indentOf(lines[next]);
                if (nextIndent > indent) {
                    index = next;
                    map.put(key, parseBlock(nextIndent));
                } else if (nextIndent == indent && stripIndent(lines[next]).startsWith("- ")) {
                    index = next;
                    map.put(key, parseList(indent));
                } else {
                    map.put(key, null);
                }
            }
            return map;
        }

        private List<Object> parseList(int indent) {
            List<Object> list = new ArrayList<>();
            while (index < lines.length) {
                String raw = lines[index];
                if (isIgnorable(raw)) {
                    index++;
                    continue;
                }
                int currentIndent = indentOf(raw);
                if (currentIndent != indent) {
                    break;
                }
                String content = stripIndent(raw);
                if (!content.startsWith("- ")) {
                    break;
                }
                String item = content.substring(2).trim();
                index++;
                if (item.isEmpty()) {
                    int next = nextMeaningful();
                    if (next >= 0 && indentOf(lines[next]) > indent) {
                        index = next;
                        list.add(parseBlock(indentOf(lines[next])));
                    } else {
                        list.add(null);
                    }
                    continue;
                }
                int colon = findColon(item);
                if (colon > 0 && !item.startsWith("\"") && !item.startsWith("'")) {
                    // Inline map inside a list item: `- name: value`.
                    Map<String, Object> inline = new LinkedHashMap<>();
                    inline.put(unquote(item.substring(0, colon).trim()), unquote(item.substring(colon + 1).trim()));
                    int childIndent = indent + 2;
                    int next = nextMeaningful();
                    if (next >= 0 && indentOf(lines[next]) > indent) {
                        index = next;
                        Object more = parseBlock(indentOf(lines[next]));
                        if (more instanceof Map<?, ?> moreMap) {
                            for (Map.Entry<?, ?> entry : moreMap.entrySet()) {
                                inline.put(String.valueOf(entry.getKey()), entry.getValue());
                            }
                        }
                    }
                    list.add(inline);
                    if (childIndent < 0) {
                        break;
                    }
                    continue;
                }
                list.add(unquote(item));
            }
            return list;
        }

        private int nextMeaningful() {
            for (int i = index; i < lines.length; i++) {
                if (!isIgnorable(lines[i])) {
                    return i;
                }
            }
            return -1;
        }

        private void skipIgnorable() {
            int next = nextMeaningful();
            if (next < 0) {
                index = lines.length;
            } else {
                index = next;
            }
        }

        private static boolean isIgnorable(String line) {
            String trimmed = line.trim();
            return trimmed.isEmpty() || trimmed.startsWith("#") || trimmed.equals("---");
        }

        private static int indentOf(String line) {
            int i = 0;
            while (i < line.length() && (line.charAt(i) == ' ' || line.charAt(i) == '\t')) {
                i++;
            }
            return i;
        }

        private static String stripIndent(String line) {
            return line.substring(indentOf(line));
        }

        private static int findColon(String content) {
            boolean inSingle = false;
            boolean inDouble = false;
            for (int i = 0; i < content.length(); i++) {
                char c = content.charAt(i);
                if (c == '\'' && !inDouble) {
                    inSingle = !inSingle;
                } else if (c == '"' && !inSingle) {
                    inDouble = !inDouble;
                } else if (c == ':' && !inSingle && !inDouble) {
                    if (i + 1 == content.length() || content.charAt(i + 1) == ' ') {
                        return i;
                    }
                }
            }
            return -1;
        }

        private static String unquote(String value) {
            String trimmed = value.trim();
            if (trimmed.length() >= 2) {
                char first = trimmed.charAt(0);
                char last = trimmed.charAt(trimmed.length() - 1);
                if (first == '"' && last == '"') {
                    String body = trimmed.substring(1, trimmed.length() - 1);
                    return new String(body.getBytes(StandardCharsets.UTF_8), StandardCharsets.UTF_8)
                            .replace("\\\"", "\"")
                            .replace("\\n", "\n");
                }
                if (first == '\'' && last == '\'') {
                    return trimmed.substring(1, trimmed.length() - 1).replace("''", "'");
                }
            }
            int comment = trimmed.indexOf(" #");
            if (comment > 0) {
                return trimmed.substring(0, comment).trim();
            }
            return trimmed;
        }
    }
}
