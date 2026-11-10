#!/usr/bin/env python3
"""End-to-end test that a real Java plugin runs inside the real server.

This is the acceptance test for Java plugin support. It does not simulate the
plugin host: it

1. compiles a real plugin (source below) against the launcher's own Bukkit API
   subset, plus ``plugin.yml``,
2. packages it into a genuine JAR,
3. starts the server with ``java -jar``,
4. connects over TCP and performs a real Minecraft login,
5. asserts on the plugin's own output — a JSON file it wrote from its
   ``PlayerJoinEvent`` handler and from a command it registered,
6. checks the console dispatches that plugin command, and that a broken plugin
   alongside it does not stop the server.

Nothing here is mocked: the plugin is ordinary plugin code, the events come from
the Rust engine through JNI, and the assertions are on files the plugin wrote.

Usage:
    scripts/acceptance_plugins.py --jar dist/.../ZOROWARRIOR-Rust-Server.jar
"""

from __future__ import annotations

import argparse
import json
import os
import shutil
import socket
import struct
import subprocess
import sys
import tempfile
import threading
import time
from pathlib import Path

PLUGIN_SOURCE = """
package acceptance;

import java.io.File;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.util.ArrayList;
import java.util.List;

import org.bukkit.command.Command;
import org.bukkit.command.CommandSender;
import org.bukkit.event.EventHandler;
import org.bukkit.event.Listener;
import org.bukkit.event.player.PlayerJoinEvent;
import org.bukkit.event.player.PlayerQuitEvent;
import org.bukkit.plugin.java.JavaPlugin;

/** A plugin that records what really happened into a file and registers a command. */
public final class AcceptancePlugin extends JavaPlugin implements Listener {

    private final List<String> lines = new ArrayList<>();

    private void record(String line) {
        lines.add(line);
        getLogger().info("recorded: " + line);
        try {
            File folder = getDataFolder();
            if (!folder.exists() && !folder.mkdirs()) {
                return;
            }
            Files.write(new File(folder, "events.log").toPath(),
                    String.join("\\n", lines).getBytes(StandardCharsets.UTF_8));
        } catch (Exception e) {
            getLogger().warning("could not write events.log: " + e);
        }
    }

    @Override
    public void onEnable() {
        record("enable");
        getServer().getPluginManager().registerEvents(this, this);
        Command greet = new Command("hello") {
            @Override
            public boolean execute(CommandSender sender, String label, String[] args) {
                String who = args.length > 0 ? args[0] : "nobody";
                record("command:" + label + ":" + who);
                sender.sendMessage("Hello from the plugin, " + who + "!");
                return true;
            }
        };
        greet.setDescription("says hello from a plugin");
        if (!getServer().getPluginManager().registerCommand(greet, this)) {
            getLogger().severe("/hello was already registered");
        }
    }

    @EventHandler
    public void onJoin(PlayerJoinEvent event) {
        record("join:" + event.getPlayer().getName()
                + ":" + event.getPlayer().getUniqueId()
                + ":protocol=" + event.getPlayer().getProtocolVersion()
                + ":online=" + event.getPlayer().isOnline());
    }

    @EventHandler
    public void onQuit(PlayerQuitEvent event) {
        record("quit:" + event.getPlayer().getName()
                + ":online=" + event.getPlayer().isOnline());
    }

    @Override
    public void onDisable() {
        record("disable");
    }
}
"""

BROKEN_SOURCE = """
package acceptance;

import org.bukkit.plugin.java.JavaPlugin;

/** Deliberately throws in onEnable to prove failures are isolated. */
public final class BrokenPlugin extends JavaPlugin {
    @Override
    public void onEnable() {
        throw new IllegalStateException("intentional failure from the acceptance test");
    }
}
"""

PLUGIN_YML = """name: Acceptance
version: 1.0.0
main: acceptance.AcceptancePlugin
api-version: 1.20
description: end-to-end plugin acceptance fixture
authors:
  - ZWRS
commands:
  hello:
    description: says hello from a plugin
    usage: /hello [name]
"""

BROKEN_YML = """name: Broken
version: 1.0.0
main: acceptance.BrokenPlugin
api-version: 1.20
"""


class Failure(Exception):
    pass


def check(condition: bool, message: str) -> None:
    if not condition:
        raise Failure(message)


# ---------------------------------------------------------------- Minecraft IO


def varint(value: int) -> bytes:
    out = bytearray()
    value &= 0xFFFFFFFF
    while True:
        byte = value & 0x7F
        value >>= 7
        if value:
            out.append(byte | 0x80)
        else:
            out.append(byte)
            return bytes(out)


def read_varint(data: bytes, offset: int) -> tuple[int, int]:
    result = 0
    for i in range(5):
        byte = data[offset + i]
        result |= (byte & 0x7F) << (7 * i)
        if not byte & 0x80:
            return result, offset + i + 1
    raise Failure("truncated varint")


def string(value: str) -> bytes:
    encoded = value.encode()
    return varint(len(encoded)) + encoded


def frame(payload: bytes) -> bytes:
    return varint(len(payload)) + payload


def read_frame(sock: socket.socket) -> bytes:
    header = b""
    while True:
        chunk = sock.recv(1)
        if not chunk:
            raise Failure("connection closed while reading a frame header")
        header += chunk
        try:
            length, consumed = read_varint(header, 0)
        except (Failure, IndexError):
            continue
        if consumed == len(header):
            break
    body = b""
    while len(body) < length:
        chunk = sock.recv(length - len(body))
        if not chunk:
            raise Failure("connection closed while reading a frame body")
        body += chunk
    return body


def java_bin() -> str:
    home = os.environ.get("JAVA_HOME")
    if home:
        candidate = Path(home) / "bin" / "java"
        if candidate.is_file():
            return str(candidate)
    return shutil.which("java") or "java"


def javac_bin() -> str:
    home = os.environ.get("JAVA_HOME")
    if home:
        candidate = Path(home) / "bin" / "javac"
        if candidate.is_file():
            return str(candidate)
    return shutil.which("javac") or "javac"


def jar_bin() -> str:
    home = os.environ.get("JAVA_HOME")
    if home:
        candidate = Path(home) / "bin" / "jar"
        if candidate.is_file():
            return str(candidate)
    return shutil.which("jar") or "jar"


def free_port() -> int:
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


# ----------------------------------------------------------------- the server


class ServerUnderTest:
    """A real server process with a single, continuously-reading console thread.

    One reader thread owns the pipe for the whole run: per-command readers race
    each other and silently steal output, which produces false failures in a test
    that is supposed to be trustworthy.
    """

    def __init__(self, jar: Path, directory: Path, port: int) -> None:
        self.jar = jar
        self.directory = directory
        self.port = port
        self.process: subprocess.Popen | None = None
        self.lines: list[str] = []
        self._lock = threading.Condition()
        self._reader: threading.Thread | None = None

    def _pump(self) -> None:
        assert self.process is not None and self.process.stdout is not None
        for raw in self.process.stdout:
            with self._lock:
                self.lines.append(raw.rstrip("\n"))
                self._lock.notify_all()

    def start(self, wait_for: str) -> None:
        self.directory.mkdir(parents=True, exist_ok=True)
        self.process = subprocess.Popen(
            [java_bin(), "-jar", str(self.jar), f"--dir={self.directory}",
             f"--port={self.port}", "--offline-mode"],
            stdout=subprocess.PIPE, stderr=subprocess.STDOUT, stdin=subprocess.PIPE,
            text=True, bufsize=1,
        )
        self._reader = threading.Thread(target=self._pump, daemon=True)
        self._reader.start()
        self.wait_for(wait_for, timeout=90)

    def wait_for(self, text: str, timeout: float = 30.0) -> list[str]:
        deadline = time.time() + timeout
        with self._lock:
            while time.time() < deadline:
                if any(text in line for line in self.lines):
                    return list(self.lines)
                if self.process is not None and self.process.poll() is not None:
                    raise Failure(f"server exited while waiting for {text!r}:\n"
                                  + "\n".join(self.lines))
                self._lock.wait(0.1)
            raise Failure(f"timed out waiting for {text!r}; console so far:\n"
                          + "\n".join(self.lines[-40:]))

    def wait_for_line(self, predicate, timeout: float = 30.0, what: str = "output") -> list[str]:
        deadline = time.time() + timeout
        with self._lock:
            while time.time() < deadline:
                if any(predicate(line) for line in self.lines):
                    return list(self.lines)
                self._lock.wait(0.1)
            raise Failure(f"timed out waiting for {what}; console so far:\n"
                          + "\n".join(self.lines[-40:]))

    def command(self, line: str, quiet: float = 1.5) -> list[str]:
        """Runs a console command and returns the lines it produced."""
        assert self.process is not None and self.process.stdin is not None
        with self._lock:
            start_index = len(self.lines)
        self.process.stdin.write(line + "\n")
        self.process.stdin.flush()
        deadline = time.time() + 20
        with self._lock:
            last_count = -1
            while time.time() < deadline:
                self._lock.wait(quiet)
                count = len(self.lines)
                # Wait until output has stopped growing for `quiet` seconds, so a
                # command's complete output is returned instead of its first line.
                if count == last_count and count > start_index:
                    break
                last_count = count
            return list(self.lines[start_index:])

    def stop(self) -> None:
        if self.process is None:
            return
        if self.process.poll() is None:
            try:
                self.command("/stop", quiet=1.0)
            except (BrokenPipeError, ValueError):
                pass
            try:
                self.process.wait(timeout=30)
            except subprocess.TimeoutExpired:
                self.process.kill()
                self.process.wait(timeout=10)
        with self._lock:
            self.lines.extend([])
        self.process = None


def build_plugin(work: Path, classes: Path) -> Path:
    """Compiles the fixture against the server's API and packages a real JAR."""
    src = work / "src" / "acceptance"
    src.mkdir(parents=True, exist_ok=True)
    (src / "AcceptancePlugin.java").write_text(PLUGIN_SOURCE)
    (src / "BrokenPlugin.java").write_text(BROKEN_SOURCE)

    out = work / "classes"
    out.mkdir(exist_ok=True)
    compile_cmd = [javac_bin(), "-encoding", "UTF-8", "-cp", str(classes), "-d", str(out),
                   str(src / "AcceptancePlugin.java"), str(src / "BrokenPlugin.java")]
    result = subprocess.run(compile_cmd, capture_output=True, text=True)
    if result.returncode != 0:
        raise Failure("the acceptance plugin did not compile against the server API:\n"
                      + result.stdout + result.stderr)

    (out / "plugin.yml").write_text(PLUGIN_YML)
    jar_path = work / "Acceptance.jar"
    result = subprocess.run([jar_bin(), "cf", str(jar_path), "-C", str(out), "."],
                            capture_output=True, text=True)
    if result.returncode != 0:
        raise Failure("could not create the plugin JAR:\n" + result.stderr)

    (out / "plugin.yml").write_text(BROKEN_YML)
    broken_path = work / "Broken.jar"
    subprocess.run([jar_bin(), "cf", str(broken_path), "-C", str(out), "."],
                   capture_output=True, text=True, check=True)
    (out / "plugin.yml").write_text(PLUGIN_YML)

    return jar_path


def login(port: int, username: str, protocol: int = 763, keep_open: bool = True):
    sock = socket.create_connection(("127.0.0.1", port), timeout=10)
    sock.settimeout(10)
    payload = (varint(0) + varint(protocol) + string("127.0.0.1")
               + struct.pack(">H", 25565) + varint(2))
    sock.sendall(frame(payload))
    sock.sendall(frame(varint(0) + string(username)))
    response = read_frame(sock)
    packet_id, offset = read_varint(response, 0)
    check(packet_id == 0, f"expected a disconnect packet, got id {packet_id}")
    length, offset = read_varint(response, offset)
    text = response[offset:offset + length].decode()
    try:
        message = json.loads(text).get("text", text)
    except json.JSONDecodeError:
        message = text
    if not keep_open:
        sock.close()
        return message, None
    return message, sock


def wait_for_file(path: Path, timeout: float = 20.0, predicate=None) -> str:
    deadline = time.time() + timeout
    last = ""
    while time.time() < deadline:
        if path.is_file():
            last = path.read_text()
            if predicate is None or predicate(last):
                return last
        time.sleep(0.1)
    raise Failure(f"timed out waiting for {path} (last content: {last!r})")


def scenario(jar: Path, work: Path, classes: Path) -> list[str]:
    steps: list[str] = []
    port = free_port()
    server_dir = work / "server"
    plugins_dir = server_dir / "plugins"
    plugins_dir.mkdir(parents=True, exist_ok=True)

    plugin_jar = build_plugin(work, classes)
    shutil.copy(plugin_jar, plugins_dir / "Acceptance.jar")
    shutil.copy(work / "Broken.jar", plugins_dir / "Broken.jar")
    steps.append("compiled and packaged a real plugin JAR against the server's Bukkit API, "
                 "plus a deliberately broken plugin")

    server = ServerUnderTest(jar, server_dir, port)
    server.start("Type a command")
    try:
        output = "\n".join(server.lines)
        check("plugins: enabled Acceptance v1.0.0" in output,
              "the plugin was enabled:\n" + output)
        check("Broken" in output and "intentional failure from the acceptance test" in output,
              "the broken plugin's real error was reported:\n" + output)
        check("plugin host connected" in output,
              "the plugin host was connected to the engine:\n" + output)
        steps.append("server started; the good plugin was enabled and the broken one failed "
                     "with its own exception message while the server kept running")

        message, sock = login(port, "PluginProbe")
        check("identity was accepted" in message, f"login failed: {message}")
        steps.append("real login over TCP admitted by the Rust engine")

        events_file = server_dir / "plugins" / "Acceptance" / "events.log"
        content = wait_for_file(events_file, predicate=lambda text: "join:" in text)
        expected_uuid = offline_uuid("PluginProbe")
        check(f"join:PluginProbe:{expected_uuid}:protocol=763:online=true" in content,
              f"the plugin's PlayerJoinEvent carried the real identity:\n{content}")
        steps.append(f"plugin received PlayerJoinEvent with the real UUID {expected_uuid}")

        assert sock is not None
        sock.close()
        content = wait_for_file(events_file, predicate=lambda text: "quit:" in text)
        check("quit:PluginProbe:online=false" in content,
              f"the plugin's PlayerQuitEvent reported isOnline=false:\n{content}")
        steps.append("plugin received PlayerQuitEvent after the client disconnected")

        reply = server.command("/hello Arena")
        output = "\n".join(reply)
        check("Hello from the plugin, Arena!" in output,
              "the console ran the plugin's command:\n" + output)
        content = wait_for_file(events_file, predicate=lambda text: "command:hello:Arena" in text)
        check("command:hello:Arena" in content, f"the plugin recorded its command run:\n{content}")
        steps.append("console command /hello was dispatched to plugin code and its output shown")

        status = server.command("/status")
        check(any("Players:" in line for line in status),
              "the server still answers /status:\n" + "\n".join(status))
        steps.append("server still healthy after plugin events and a plugin command")

        server.command("/stop", quiet=0.5)
        content = wait_for_file(events_file, predicate=lambda text: "disable" in text)
        check(content.rstrip().splitlines()[-1] == "disable",
              f"onDisable() ran last:\n{content}")
        steps.append("onDisable() ran on shutdown")
    finally:
        server.stop()
    return steps


def offline_uuid(name: str) -> str:
    import hashlib
    digest = bytearray(hashlib.md5(("OfflinePlayer:" + name).encode()).digest())
    digest[6] = (digest[6] & 0x0F) | 0x30
    digest[8] = (digest[8] & 0x3F) | 0x80
    hexed = digest.hex()
    return f"{hexed[0:8]}-{hexed[8:12]}-{hexed[12:16]}-{hexed[16:20]}-{hexed[20:]}"


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--jar", required=True, type=Path)
    parser.add_argument("--classes", type=Path,
                        help="compiled launcher classes to compile the fixture against")
    parser.add_argument("--keep", action="store_true")
    args = parser.parse_args()

    if not args.jar.is_file():
        print(f"FAIL: jar not found: {args.jar}")
        return 2

    work = Path(tempfile.mkdtemp(prefix="zwrs-plugins-"))
    classes = args.classes
    try:
        if classes is None:
            # Compile the launcher API from source, exactly as the bundle does.
            classes = work / "api"
            classes.mkdir()
            repo = Path(__file__).resolve().parent.parent
            sources = list((repo / "java" / "src" / "main" / "java").rglob("*.java"))
            if not sources:
                raise Failure("java/src/main/java is missing; pass --classes")
            result = subprocess.run([javac_bin(), "-encoding", "UTF-8", "-d", str(classes)]
                                    + [str(s) for s in sources],
                                    capture_output=True, text=True)
            if result.returncode != 0:
                raise Failure("could not compile the server API:\n" + result.stderr)
        steps = scenario(args.jar.resolve(), work, classes)
    except Failure as error:
        print(f"FAIL: {error}")
        if args.keep:
            print(f"kept: {work}")
        return 1
    except Exception as error:  # noqa: BLE001
        print(f"FAIL (unexpected): {type(error).__name__}: {error}")
        if args.keep:
            print(f"kept: {work}")
        return 1

    for index, step in enumerate(steps, 1):
        print(f"  {index}. {step}")
    print(f"PASS: Java plugins run inside the Rust server ({len(steps)} checks): "
          "a real plugin JAR was loaded, received real join/quit events from the engine "
          "and ran a command, while a broken plugin failed in isolation")
    if not args.keep:
        shutil.rmtree(work, ignore_errors=True)
    return 0


if __name__ == "__main__":
    sys.exit(main())
