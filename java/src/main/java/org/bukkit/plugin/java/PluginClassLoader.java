package org.bukkit.plugin.java;

import java.io.IOException;
import java.io.InputStream;
import java.net.URL;
import java.net.URLClassLoader;
import java.util.Enumeration;
import java.util.HashSet;
import java.util.Set;

/**
 * Class loader for one plugin JAR.
 *
 * <h2>Isolation rules (and why)</h2>
 *
 * <ul>
 *   <li>Plugin classes come from the plugin's own JAR first, so two plugins can
 *       ship different versions of the same library.</li>
 *   <li>The API packages this host implements ({@code org.bukkit.*}) always come
 *       from the server, never from a plugin: a plugin cannot shadow the server's
 *       event classes and break dispatch for everyone else.</li>
 *   <li>JDK classes ({@code java.*}, {@code javax.*}, {@code jdk.*},
 *       {@code sun.*}) are delegated to the platform, so a plugin cannot replace
 *       {@code java.lang.String} or similar.</li>
 *   <li>Everything else falls back to the server class loader, then the plugin
 *       JAR.</li>
 * </ul>
 *
 * <p>Loading is lazy, so a plugin that never touches an optional library never
 * pays for it.</p>
 */
public final class PluginClassLoader extends URLClassLoader {

    static {
        ClassLoader.registerAsParallelCapable();
    }

    /** Packages that must always resolve to the server's copy. */
    private static final String[] SERVER_FIRST = {
        "org.bukkit.plugin.java.",
        "org.bukkit.plugin.Plugin",
        "org.bukkit.plugin.PluginDescriptionFile",
        "org.bukkit.plugin.PluginManager",
    };

    /** Packages a plugin must never be able to replace. */
    private static final String[] PLATFORM_ONLY = {
        "java.", "javax.", "jdk.", "sun.", "com.sun.",
    };

    private final PluginLoader owner;
    private final String pluginName;
    private final Set<String> pluginPackages = new HashSet<>();
    private final Set<String> ownClasses = new HashSet<>();

    /**
     * @param owner loader that owns logging
     * @param pluginName plugin name, for messages
     * @param jar the plugin JAR
     * @param classesInsideJar class names found in the JAR (used to report a
     *     precise error when a plugin references a class it does not ship)
     * @throws IOException if the JAR cannot be opened
     */
    public PluginClassLoader(PluginLoader owner, String pluginName, java.nio.file.Path jar,
            Set<String> classesInsideJar) throws IOException {
        super(new URL[] {jar.toUri().toURL()}, PluginClassLoader.class.getClassLoader());
        this.owner = owner;
        this.pluginName = pluginName;
        this.ownClasses.addAll(classesInsideJar);
        for (String name : classesInsideJar) {
            int dot = name.lastIndexOf('.');
            if (dot > 0) {
                pluginPackages.add(name.substring(0, dot));
            }
        }
    }

    /** @return the plugin this loader belongs to */
    public String getPluginName() {
        return pluginName;
    }

    @Override
    protected Class<?> loadClass(String name, boolean resolve) throws ClassNotFoundException {
        synchronized (getClassLoadingLock(name)) {
            Class<?> loaded = findLoadedClass(name);
            if (loaded != null) {
                if (resolve) {
                    resolveClass(loaded);
                }
                return loaded;
            }

            if (name.startsWith("[")) {
                return super.loadClass(name, resolve);
            }

            // 1. The server's own API must win, whatever the plugin ships.
            if (isServerFirst(name)) {
                return super.loadClass(name, resolve);
            }

            // 2. The plugin's own classes, but never JDK internals.
            if (!isPlatformOnly(name)) {
                try {
                    Class<?> own = findClass(name);
                    if (resolve) {
                        resolveClass(own);
                    }
                    return own;
                } catch (ClassNotFoundException ignored) {
                    // Fall through to the parent.
                }
            }

            // 3. The rest: platform, then the server class path.
            try {
                Class<?> parent = super.loadClass(name, resolve);
                if (isPlatformOnly(name) || !ownClasses.contains(name)) {
                    return parent;
                }
                return parent;
            } catch (ClassNotFoundException notFound) {
                throw new ClassNotFoundException(
                        "plugin '" + pluginName + "' could not load " + name
                                + ": it is not in the plugin JAR, the server API or the JDK. "
                                + "Bundle the library inside the plugin JAR.",
                        notFound);
            }
        }
    }

    private static boolean isServerFirst(String name) {
        for (String prefix : SERVER_FIRST) {
            if (name.startsWith(prefix) && !name.equals(prefix)) {
                return true;
            }
        }
        // Interfaces and event classes the host supplies.
        return name.equals("org.bukkit.Server")
                || name.equals("org.bukkit.Player")
                || name.equals("org.bukkit.event.Listener")
                || name.startsWith("org.bukkit.event.player.")
                || name.startsWith("org.bukkit.command.")
                || name.startsWith("org.bukkit.plugin.")
                || name.equals("org.bukkit.Bukkit");
    }

    private static boolean isPlatformOnly(String name) {
        for (String prefix : PLATFORM_ONLY) {
            if (name.startsWith(prefix)) {
                return true;
            }
        }
        return false;
    }

    @Override
    public InputStream getResourceAsStream(String name) {
        InputStream stream = super.getResourceAsStream(name);
        if (stream != null) {
            return stream;
        }
        return PluginClassLoader.class.getClassLoader().getResourceAsStream(name);
    }

    @Override
    public Enumeration<URL> getResources(String name) throws IOException {
        return super.getResources(name);
    }

    @Override
    public String toString() {
        return "PluginClassLoader[" + pluginName + "]";
    }

    /** @return true when the JAR contains this class (no loading attempt) */
    public boolean shipsClass(String className) {
        return ownClasses.contains(className);
    }

    /** @return packages that come from the plugin JAR */
    public Set<String> getPluginPackages() {
        return Set.copyOf(pluginPackages);
    }

    /** @return the owner, for logging */
    public PluginLoader getOwner() {
        return owner;
    }
}
