# Linux installation

Verified on Debian 12 x86_64 with OpenJDK 17.0.8.

## Requirements

* Java 17 or newer (`java -version`)
* x86_64 or aarch64 Linux, glibc 2.17+
* ~40 MB disk space, 64 MB RAM for the current build

## Install

```bash
mkdir -p ~/zwrs && cd ~/zwrs
tar xf ZOROWARRIOR-Rust-Server-0.1.0-linux-x86_64.tar.gz   # or unzip
cd ZOROWARRIOR-Rust-Server-0.1.0-linux-x86_64
sha256sum -c SHA256SUMS
java -jar ZOROWARRIOR-Rust-Server.jar --diagnose
java -jar ZOROWARRIOR-Rust-Server.jar --selftest
```

`--diagnose` prints the platform tag, the library name the launcher looks for and
what it found. `--selftest` proves the JAR can drive the Rust engine before you
expose the port.

## Run

```bash
java -jar ZOROWARRIOR-Rust-Server.jar
```

First start creates `config/zwrs.properties`, `data/`, `logs/`, `plugins/`.
Edit the configuration and restart to apply changes (`/reload` only flushes data
and rescans plugins).

Playing from a client on the same machine: connect to `localhost:25565`.
From another machine on your network: connect to the host's LAN address and make
sure the port is reachable.

## systemd unit

```ini
# /etc/systemd/system/zwrs.service
[Unit]
Description=ZOROWARRIOR Rust Server
After=network-online.target

[Service]
Type=simple
User=minecraft
WorkingDirectory=/srv/zwrs
ExecStart=/usr/bin/java -jar /srv/zwrs/ZOROWARRIOR-Rust-Server.jar --dir=/srv/zwrs
Restart=on-failure
RestartSec=5
# SIGTERM is handled: sessions are closed and data is flushed before exit.
TimeoutStopSec=30

# Hardening
NoNewPrivileges=yes
PrivateTmp=yes
ProtectSystem=strict
ProtectHome=yes
ReadWritePaths=/srv/zwrs

[Install]
WantedBy=multi-user.target
```

```bash
sudo systemctl daemon-reload
sudo systemctl enable --now zwrs
journalctl -u zwrs -f
```

To send commands, use `--status` for a snapshot or attach a console:

```bash
sudo systemctl stop zwrs
sudo -u minecraft java -jar /srv/zwrs/ZOROWARRIOR-Rust-Server.jar --dir=/srv/zwrs
```

## Upgrades

1. Stop the service (graceful `/stop` or SIGTERM).
2. Back up `data/` — the engine never deletes player data, but a backup is cheap.
3. Replace the JAR **and** `native/` together: they are version-locked through
   the JNI bridge version, and a mismatch is refused at startup.
4. Start and run `--selftest`.

## Troubleshooting

| Symptom | Cause and fix |
|---|---|
| `Could not load the ZOROWARRIOR Rust engine ...` | wrong `native/<os>-<arch>/` directory or missing file; run `--diagnose` |
| `native bridge version N does not match ...` | JAR and `native/` from different releases; replace both |
| `could not bind 0.0.0.0:25565` | port in use (`ss -lntp | grep 25565`) or no permission |
| `configuration problem: line N: unknown key ...` | typo in `config/zwrs.properties`; the message names the line |
| Server appears in the list but joining disconnects | expected in v0.1.0: play-state packets are not implemented |
| `players.json.corrupt-*` appears | data was damaged; the original is preserved. See `docs/PLAYER-DATA.md` |
