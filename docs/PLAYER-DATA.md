# Player data

## Layout

```text
data/
└── players/
    ├── players.json          authoritative index (atomic writes)
    ├── sessions.json         session history (atomic writes)
    └── profiles/
        ├── <uuid>.json       one readable mirror per player
        └── ...
```

`players.json` is what the engine trusts. The per-UUID files exist so an operator
can read or repair a single player, and so a damaged index can be rebuilt.

## Document format

```json
{
  "format": 1,
  "written_at": "2026-11-10T10:00:00Z",
  "records": [
    {
      "uuid": "b50ad385-829d-3141-a216-7e7d7539ba7f",
      "username": "Notch",
      "previous_usernames": [],
      "first_join": "2026-11-10T10:00:00Z",
      "last_join": "2026-11-10T10:00:00Z",
      "last_logout": "2026-11-10T10:30:00Z",
      "total_playtime_seconds": 1800,
      "join_count": 1,
      "online": false,
      "identity_mode": "offline",
      "last_ip": null
    }
  ]
}
```

Field rules:

| Field | Rule |
|---|---|
| `uuid` | canonical UUID; parsed and re-serialised, never a random value |
| `username` | validated `[A-Za-z0-9_]{3,16}`; the most recent observed spelling |
| `previous_usernames` | oldest first, case-insensitively de-duplicated |
| `first_join` | never changes after creation |
| `last_join` | updated on every admitted join |
| `last_logout` | `null` until a real logout has been recorded |
| `total_playtime_seconds` | only credited from measured sessions (`logout - join`) |
| `join_count` | increments only on admitted joins |
| `online` | `true` only while a session is active in the running process |
| `identity_mode` | `online` or `offline`; recorded from the last join |
| `last_ip` | `null` unless `store-ip-addresses = true` |

A document whose `format` is newer than the build is refused rather than
rewritten:

```text
document format 2 is newer than this build supports (1); upgrade ZWRS instead of
overwriting newer data
```

## Write path

Every write goes through `zwrs_core::storage::write_atomic`:

1. write the whole document to `.players.json.tmp-<pid>-<n>` in the same directory
2. `fsync` the file
3. `rename` over the target (atomic on POSIX)
4. `fsync` the directory

A crash leaves either the previous complete document or the new complete
document. Temporary files are removed if any step fails.

## Corrupt or empty files

* The file is **moved aside** to `players.json.corrupt-<timestamp>` and kept
  byte-for-byte.
* The engine then tries to rebuild the index from `profiles/*.json`.
* If there is nothing to rebuild from, **startup fails** with the quarantine path
  in the message. The engine never starts with an empty database in place of
  real data.

## Restoring a player

```bash
# stop the server first
ls data/players/                       # look for players.json.corrupt-*
python3 - <<'PY'
import json, glob, os
path = "data/players/players.json"
doc = json.load(open(path)) if os.path.exists(path) else {"format": 1, "written_at": "manual", "records": []}
known = {r["uuid"] for r in doc["records"]}
for file in glob.glob("data/players/profiles/*.json"):
    record = json.load(open(file))
    if record["uuid"] not in known:
        doc["records"].append(record)
json.dump(doc, open(path, "w"), indent=2)
print("recovered", len(doc["records"]), "profiles")
PY
```

Or restore one file from your backup:

```bash
cp backups/backup-20261110T100000Z/players/players.json data/players/players.json
```

Restarting the server also forces every profile offline, so a stale `online:
true` cannot survive a crash.

## Backups

`backup-interval-seconds` and `backup-keep` in `config/zwrs.properties` control
periodic snapshots of the data directory (`<data>/backups/backup-<UTC>`).
`storage::prune_backups` keeps the newest `backup-keep` snapshots. Backups are
plain files: copy them off the device.

## Privacy

* IP addresses are **not** stored unless `store-ip-addresses = true`; turning the
  option off also strips stored addresses on the next start.
* Only identity data the server must have is kept: UUID, username, timestamps,
  playtime, join count and online flag.
* Deleting a player is a deliberate manual action: stop the server, remove the
  record from `players.json` and its `profiles/<uuid>.json`, restart.
