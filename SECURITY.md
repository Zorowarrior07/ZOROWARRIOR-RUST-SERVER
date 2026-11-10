# Security policy

## Supported versions

| Version | Supported |
|---|---|
| 0.1.x | yes (architecture proof; see the caveats below) |

## Reporting a vulnerability

Report privately through GitHub's **Security → Report a vulnerability** on
`Zorowarrior07/ZOROWARRIOR-RUST-SERVER`. Please do not open a public issue for
an exploitable problem. Include the version, platform, a reproduction and the
impact you observed. You will get an acknowledgement and a timeline; fixes are
released with a changelog entry that does not disclose the exploit before users
can upgrade.

## Security model (what is and is not protected)

* **Offline mode is not authentication.** With `online-mode = false` any client
  can claim any username; UUIDs are derived from names. Use it on a trusted
  network only. This is documented in the startup log, `/version`, `/status` and
  the login disconnect text.
* **Online mode is not implemented.** Logins are refused rather than admitted
  without verification. Do not expose the server to the internet expecting
  authenticated players.
* **No world/gameplay execution.** Play packets are not processed, so there is no
  in-game command surface to attack yet.
* **Plugin code.** Java plugins are read and validated, never executed. WASM
  plugins are validated, never executed. When execution lands, native plugins
  will run with the server's privileges — that will be documented as a trust
  boundary, not as a sandbox.
* **Input handling.** Frame length is capped at 2 MiB, VarInts are limited to 5
  bytes, strings are length- and UTF-8-checked, `plugin.yml` entries are limited
  to 4 MiB, and ZIP decompression is bounded by the declared size.
* **Player data.** Writes are atomic (temp file + `fsync` + rename + directory
  `fsync`); corrupt files are quarantined, never replaced with an empty
  database. IP storage is off by default.
* **Native loading.** The launcher only loads a library from an explicit path,
  `native/<os>-<arch>/`, the JAR itself (size-verified, private temp directory),
  or `java.library.path`. It never downloads or executes code from the network.
* **No secrets in the repository.** There are no credentials, tokens or keys in
  the source tree. Do not commit player databases or logs.
