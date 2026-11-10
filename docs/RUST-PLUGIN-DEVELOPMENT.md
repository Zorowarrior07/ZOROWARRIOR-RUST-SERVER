# Rust plugin development

## Status

| Piece | State |
|---|---|
| Plugin directory layout and `manifest.json` schema | **defined and validated** |
| `\0asm` header check, size limit, API version check | **implemented** (`zwrs_engine::plugins`) |
| SHA-256 of the plugin binary | **implemented** |
| WASM runtime (execution) | **not implemented** — no runtime is linked |
| Host functions (events, commands, world access) | **not implemented** |
| Plugin SDK crate | **not published** (schema documented below) |

So a plugin can be *recognised and validated* today, and it will be *refused at
execution* because there is nothing to execute it. `/status` and the launcher
report both facts; nothing pretends to run.

## Layout

```text
plugins/
└── hello/
    ├── manifest.json
    └── plugin.wasm
```

## `manifest.json`

```json
{
  "name": "hello",
  "version": "0.1.0",
  "api_version": 1,
  "entry": "plugin.wasm"
}
```

| Field | Rules |
|---|---|
| `name` | `[A-Za-z0-9_-]{1,64}` |
| `version` | non-empty string |
| `api_version` | integer; must equal `zwrs_engine::plugins::PLUGIN_API_VERSION` (currently `1`) |
| `entry` | plain file name ending in `.wasm`; no path separators, no `..` |

Validation errors are reported per plugin and do not stop the scan.

## Why WASM and not `dylib`

* **Portability**: `wasm32-wasip1` bytes run on any host the engine supports.
* **Isolation**: a WASM runtime bounds memory and CPU; a `cdylib` runs with the
  server's full privileges and can only be "trusted", never "sandboxed".
* **ABI stability**: the Rust compiler ABI is unstable. Exposing it as a plugin
  contract would break on every toolchain update. WASM + a versioned host
  interface is stable by construction; a C ABI would be the alternative.

## Planned ABI (API v1)

Host functions exposed to a plugin (names illustrative until the runtime lands):

```text
zwrs_api_version() -> u32
zwrs_log(level: u32, ptr: *const u8, len: u32)
zwrs_player_count() -> u32
zwrs_player_uuid(index: u32, out: *mut u8) -> u32
zwrs_subscribe(event_id: u32) -> u32
zwrs_register_command(name_ptr: *const u8, name_len: u32) -> u32
```

Plugin exports:

```text
zwrs_plugin_init(api_version: u32) -> u32     // 0 = ok
zwrs_plugin_shutdown() -> u32
zwrs_on_event(event_id: u32, payload_ptr: *const u8, payload_len: u32) -> u32
```

Capabilities will be declared in the manifest (`"capabilities": ["log", "players"]`)
and enforced by the host: a plugin without `players` cannot call
`zwrs_player_count`. Resource limits (memory pages, fuel/instruction budget, host
call budget) will be configured per plugin.

## Building a plugin today

The toolchain is the normal Rust WASM target:

```bash
rustup target add wasm32-wasip1
cargo build --release --target wasm32-wasip1
mkdir -p plugins/hello
cp target/wasm32-wasip1/release/hello.wasm plugins/hello/plugin.wasm
cat > plugins/hello/manifest.json <<'JSON'
{ "name": "hello", "version": "0.1.0", "api_version": 1, "entry": "plugin.wasm" }
JSON
```

Start the server; `/plugins` will list `hello` and explain that this build does
not link a WASM runtime. The manifest and binary header have been validated.

## Security notes

* Treat every plugin as untrusted input: validate the manifest, bound the binary
  size, and only load what is listed in the manifest.
* When execution exists, state clearly in `SECURITY.md` what the sandbox does and
  does not enforce. Do not call it "sandboxed" until a runtime actually enforces
  memory and time limits.
* A native `cdylib` plugin would run with the server's privileges; it must be
  documented as trusted code, with no isolation claim.
