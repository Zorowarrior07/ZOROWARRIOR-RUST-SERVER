package org.bukkit.command;

/**
 * A command a plugin has registered.
 *
 * <p>The host calls {@link #execute} with the console sender and the arguments
 * that followed the name; returning {@code false} produces the standard usage
 * line {@link #getUsage()}.</p>
 */
public abstract class Command {

    private final String name;
    private String description = "";
    private String usage = "";
    private String permission;
    private String[] aliases = new String[0];

    /**
     * @param name command name, without a slash
     */
    protected Command(String name) {
        if (name == null || name.isBlank()) {
            throw new IllegalArgumentException("command name must not be blank");
        }
        this.name = name.toLowerCase(java.util.Locale.ROOT);
    }

    /** @return the command name */
    public String getName() {
        return name;
    }

    /** @return the description shown in help */
    public String getDescription() {
        return description;
    }

    /**
     * @param description description text
     * @return this command, for chaining
     */
    public Command setDescription(String description) {
        this.description = description == null ? "" : description;
        return this;
    }

    /** @return the usage line */
    public String getUsage() {
        return usage.isEmpty() ? "/" + name : usage;
    }

    /**
     * @param usage usage line
     * @return this command, for chaining
     */
    public Command setUsage(String usage) {
        this.usage = usage == null ? "" : usage;
        return this;
    }

    /** @return the required permission, or {@code null} */
    public String getPermission() {
        return permission;
    }

    /**
     * @param permission required permission, or {@code null}
     * @return this command, for chaining
     */
    public Command setPermission(String permission) {
        this.permission = permission;
        return this;
    }

    /** @return alternate names */
    public String[] getAliases() {
        return aliases.clone();
    }

    /**
     * @param aliases alternate names
     * @return this command, for chaining
     */
    public Command setAliases(java.util.List<String> aliases) {
        this.aliases = aliases == null ? new String[0]
                : aliases.stream().map(a -> a.toLowerCase(java.util.Locale.ROOT))
                        .toArray(String[]::new);
        return this;
    }

    /**
     * Runs the command.
     *
     * @param sender who ran it
     * @param commandLabel the name actually used (including an alias)
     * @param args arguments after the name
     * @return whether the command did something; {@code false} shows the usage
     */
    public abstract boolean execute(CommandSender sender, String commandLabel, String[] args);
}
