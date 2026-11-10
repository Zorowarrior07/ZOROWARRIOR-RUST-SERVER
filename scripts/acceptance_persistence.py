#!/usr/bin/env python3
"""Mandatory player-persistence acceptance test.

This drives the *real* server (`java -jar ZOROWARRIOR-Rust-Server.jar`) over a
real TCP socket with hand-built Minecraft Java Edition packets, and then asserts
on the *persisted JSON files* the engine wrote. Nothing is mocked and nothing is
inferred from log output.

It is not a real Minecraft client: it speaks the same handshake/login/status
packets a client does and labels itself accordingly. Real-client testing is
tracked separately in docs/COMPATIBILITY.md.

Usage:
    scripts/acceptance_persistence.py --jar dist/.../ZOROWARRIOR-Rust-Server.jar

Exit code 0 means every assertion passed.
"""

from __future__ import annotations

import argparse
import json
import os
import re
import shutil
import socket
import struct
import subprocess
import sys
import tempfile
import time
from pathlib import Path

USERNAME = "PersistProbe"
PROTOCOL = 763  # 1.20.1


class Failure(Exception):
    pass


def check(condition: bool, message: str) -> None:
    if not condition:
        raise Failure(message)


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
        if offset + i >= len(data):
            raise Failure("truncated varint")
        byte = data[offset + i]
        result |= (byte & 0x7F) << (7 * i)
        if not byte & 0x80:
            if result >= 2**31:
                result -= 2**32
            return result, offset + i + 1
    raise Failure("varint too long")


def string(value: str) -> bytes:
    encoded = value.encode("utf-8")
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
        except Failure:
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


def offline_uuid(name: str) -> str:
    import hashlib

    digest = bytearray(hashlib.md5(("OfflinePlayer:" + name).encode()).digest())
    digest[6] = (digest[6] & 0x0F) | 0x30
    digest[8] = (digest[8] & 0x3F) | 0x80
    hexed = digest.hex()
    return f"{hexed[0:8]}-{hexed[8:12]}-{hexed[12:16]}-{hexed[16:20]}-{hexed[20:]}"


def connect(port: int, timeout: float = 10.0) -> socket.socket:
    deadline = time.time() + timeout
    last_error: Exception | None = None
    while time.time() < deadline:
        try:
            sock = socket.create_connection(("127.0.0.1", port), timeout=5)
            sock.settimeout(10)
            return sock
        except OSError as error:  # server still starting
            last_error = error
            time.sleep(0.1)
    raise Failure(f"could not connect to the server on port {port}: {last_error}")


def handshake(sock: socket.socket, next_state: int, protocol: int = PROTOCOL) -> None:
    payload = varint(0) + varint(protocol) + string("127.0.0.1") + struct.pack(">H", 25565) + varint(next_state)
    sock.sendall(frame(payload))


def status(port: int) -> dict:
    with connect(port) as sock:
        handshake(sock, 1)
        sock.sendall(frame(varint(0)))
        response = read_frame(sock)
        _packet_id, offset = read_varint(response, 0)
        length, offset = read_varint(response, offset)
        document = json.loads(response[offset : offset + length].decode("utf-8"))
        # Answer the ping so the server can close the connection cleanly.
        sock.sendall(frame(varint(1) + struct.pack(">q", 12345)))
        read_frame(sock)
        return document


def login(port: int, username: str, protocol: int = PROTOCOL, keep_open: bool = False):
    """Performs a login. Returns (disconnect_message, socket_or_None)."""
    sock = connect(port)
    handshake(sock, 2, protocol)
    sock.sendall(frame(varint(0) + string(username)))
    payload = read_frame(sock)
    packet_id, offset = read_varint(payload, 0)
    check(packet_id == 0, f"expected a disconnect packet, got id {packet_id}")
    length, offset = read_varint(payload, offset)
    text = payload[offset : offset + length].decode("utf-8")
    try:
        message = json.loads(text).get("text", text)
    except json.JSONDecodeError:
        message = text
    if keep_open:
        return message, sock
    sock.close()
    return message, None


class Server:
    def __init__(self, jar: Path, directory: Path, port: int) -> None:
        self.jar = jar
        self.directory = directory
        self.port = port
        self.process: subprocess.Popen | None = None
        self.log_path = directory.parent / f"server-{port}.log"

    def start(self) -> None:
        self.directory.mkdir(parents=True, exist_ok=True)
        self.process = subprocess.Popen(
            [java_bin(), "-jar", str(self.jar), f"--dir={self.directory}",
             f"--port={self.port}", "--offline-mode"],
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            stdin=subprocess.PIPE,
            text=True,
            bufsize=1,
        )
        # Wait until the engine reports a listening socket.
        deadline = time.time() + 60
        output: list[str] = []
        assert self.process.stdout is not None
        while time.time() < deadline:
            line = self.process.stdout.readline()
            if not line:
                if self.process.poll() is not None:
                    raise Failure("server exited during startup:\n" + "".join(output))
                continue
            output.append(line)
            if "listening on" in line:
                self.log_path.write_text("".join(output))
                return
        self.stop()
        raise Failure("server did not start within 60s:\n" + "".join(output))

    def command(self, line: str) -> None:
        assert self.process is not None and self.process.stdin is not None
        self.process.stdin.write(line + "\n")
        self.process.stdin.flush()

    def stop(self) -> None:
        if self.process is None:
            return
        if self.process.poll() is None:
            try:
                self.command("/stop")
            except (BrokenPipeError, ValueError):
                pass
            try:
                self.process.wait(timeout=20)
            except subprocess.TimeoutExpired:
                self.process.kill()
                self.process.wait(timeout=10)
        if self.process.stdout is not None:
            try:
                with self.log_path.open("a") as handle:
                    handle.write(self.process.stdout.read() or "")
            except ValueError:
                pass
        self.process = None


def java_bin() -> str:
    home = os.environ.get("JAVA_HOME")
    if home:
        candidate = Path(home) / "bin" / "java"
        if candidate.is_file():
            return str(candidate)
    return shutil.which("java") or "java"


def free_port() -> int:
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


def read_json(path: Path) -> dict:
    check(path.is_file(), f"expected {path} to exist")
    return json.loads(path.read_text())


def find_record(document: dict, uuid: str) -> dict:
    for record in document.get("records", []):
        if record.get("uuid") == uuid:
            return record
    raise Failure(f"no record with uuid {uuid} in {document}")


def wait_for(predicate, timeout: float, description: str):
    deadline = time.time() + timeout
    last = None
    while time.time() < deadline:
        try:
            last = predicate()
            if last:
                return last
        except (Failure, json.JSONDecodeError):
            pass
        time.sleep(0.05)
    raise Failure(f"timed out waiting for {description} (last value: {last})")


def scenario(jar: Path, work: Path) -> list[str]:
    steps: list[str] = []
    port = free_port()
    directory = work / "server"
    uuid = offline_uuid(USERNAME)

    server = Server(jar, directory, port)
    server.start()
    try:
        steps.append("server started through java -jar (Rust engine listening)")

        document = status(port)
        check(document["version"]["protocol"] == 772, "status advertises protocol 772")
        check(document["players"]["online"] == 0, "no players online at start")
        steps.append("status ping answered with real protocol/player data")

        message, held = login(port, USERNAME, keep_open=True)
        check("identity was accepted" in message, f"login was not admitted: {message}")
        steps.append(f"offline login admitted: {message.splitlines()[0]}")

        players_file = directory / "data" / "players" / "players.json"
        record = wait_for(
            lambda: find_record(read_json(players_file), uuid) if players_file.exists() else None,
            10,
            "the profile to be persisted",
        )
        check(record["username"] == USERNAME, "profile username matches")
        check(record["online"] is True, "profile is online while connected")
        check(record["join_count"] == 1, "first join counted once")
        check(record["identity_mode"] == "offline", "identity mode recorded")
        check(record["first_join"].endswith("Z"), "first_join is an ISO-8601 UTC timestamp")
        steps.append("profile created for the real offline UUID and marked online")

        sessions_file = directory / "data" / "players" / "sessions.json"
        session = wait_for(
            lambda: next(
                (s for s in read_json(sessions_file)["records"] if s["uuid"] == uuid),
                None,
            ),
            10,
            "the session to be recorded",
        )
        check(session["state"] == "active", "session is active while connected")
        steps.append("session opened and persisted while the player is online")

        check(held is not None, "the client socket must be open")
        held.close()

        def profile_offline():
            record = find_record(read_json(players_file), uuid)
            return record if record["online"] is False else None

        record = wait_for(profile_offline, 15, "the profile to be marked offline")
        check(record["last_logout"], "last_logout timestamp recorded")
        check(record["total_playtime_seconds"] >= 0, "playtime is not negative")
        steps.append(f"disconnect recorded: online=false, last_logout={record['last_logout']}")

        closed = wait_for(
            lambda: next(
                (s for s in read_json(sessions_file)["records"]
                 if s["uuid"] == uuid and s["state"] == "closed"),
                None,
            ),
            15,
            "the session to be closed with a duration",
        )
        check(closed["duration_seconds"] is not None, "closed session has a duration")
        check(closed["ended_at"], "closed session has an end timestamp")
        steps.append(f"session closed with duration {closed['duration_seconds']}s")

        before = find_record(read_json(players_file), uuid)

        server.stop()
        steps.append("server stopped through /stop")

        server2 = Server(jar, directory, port)
        server2.start()
        try:
            after = find_record(read_json(players_file), uuid)
            check(after["uuid"] == before["uuid"], "UUID survives the restart")
            check(after["username"] == before["username"], "username survives the restart")
            check(after["join_count"] == before["join_count"], "join count survives the restart")
            check(
                after["total_playtime_seconds"] == before["total_playtime_seconds"],
                "playtime survives the restart",
            )
            check(after["online"] is False, "player is not reported online after a restart")
            check(after["last_logout"] == before["last_logout"], "logout timestamp survives")

            sessions_after = read_json(sessions_file)
            check(
                any(s["uuid"] == uuid and s["state"] == "closed" for s in sessions_after["records"]),
                "session history survives the restart",
            )
            steps.append("profile, playtime, logout timestamp and session history survive a restart")

            # 1.8.x login path (protocol 47) must behave the same way.
            old_message, _ = login(port, "OldClient", protocol=47)
            check("identity was accepted" in old_message,
                  f"protocol 47 login was not admitted: {old_message}")
            steps.append("protocol 47 (1.8.x) login admitted")
        finally:
            server2.stop()
    finally:
        server.stop()
    return steps


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--jar", required=True, type=Path)
    parser.add_argument("--keep", action="store_true", help="keep the temporary server directory")
    args = parser.parse_args()

    if not args.jar.is_file():
        print(f"FAIL: jar not found: {args.jar}")
        return 2

    work = Path(tempfile.mkdtemp(prefix="zwrs-acceptance-"))
    try:
        steps = scenario(args.jar.resolve(), work)
    except Failure as error:
        print(f"FAIL: {error}")
        for log in sorted(work.rglob("*.log")):
            print(f"--- {log} ---")
            print(log.read_text()[-4000:])
        if args.keep:
            print(f"kept: {work}")
        return 1
    except Exception as error:  # noqa: BLE001 - report anything unexpected
        print(f"FAIL (unexpected): {type(error).__name__}: {error}")
        for log in sorted(work.rglob("*.log")):
            print(f"--- {log} ---")
            print(log.read_text()[-4000:])
        return 1

    for index, step in enumerate(steps, 1):
        print(f"  {index}. {step}")
    print(f"PASS: player persistence verified end to end ({len(steps)} checks) "
          f"using the real jar, real sockets and real stored files")
    if not args.keep:
        shutil.rmtree(work, ignore_errors=True)
    return 0


if __name__ == "__main__":
    sys.exit(main())
