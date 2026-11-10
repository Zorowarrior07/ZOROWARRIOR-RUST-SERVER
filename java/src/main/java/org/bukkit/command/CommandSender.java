package org.bukkit.command;

/**
 * Anything that can run a command and receive messages.
 *
 * <p>ZWRS passes the console as a sender; there is no in-game command sender
 * because play-state packets are not implemented.</p>
 */
public interface CommandSender {

    /**
     * Sends a message to this sender.
     *
     * @param message message text
     */
    void sendMessage(String message);

    /** @return the sender's name */
    String getName();

    /** @return whether this sender is the console */
    boolean isConsole();

    /**
     * Runs a command as this sender.
     *
     * @param line command line, with or without a leading slash
     * @return whether the command was recognised
     */
    boolean dispatchCommand(String line);
}
