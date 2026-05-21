# cinchcli-core

Shared client primitives for [Cinch](https://cinchcli.com) — the encrypted clipboard sync service. Used by the `cinch` CLI (`crates/cli/`) and the macOS desktop app (`apps/desktop/`) inside this monorepo. Not published to crates.io.

## Internal usage

Sibling crates in this workspace depend on this via path:

```toml
client-core = { package = "cinchcli-core", path = "../client-core" }
```

The desktop app also enables the `specta` feature for tauri-specta TypeScript bindings:

```toml
client-core = { package = "cinchcli-core", path = "../../../crates/client-core", features = ["specta"] }
```

The `package =` alias keeps imports spelled `client_core::*` regardless of the crate name.

## What's inside

- **`client_core::proto::cinch::v1::*`** — generated message types from
  `proto/cinch/v1/*.proto` (the same `.proto` schema the Go relay
  serves). Wire-compatible JSON; `omitempty` semantics preserved.
- **`client_core::rest`, `client_core::http`** — REST DTOs and a typed
  HTTP client (rustls + reqwest, 3-attempt exponential backoff).
- **`client_core::ws`** — WebSocket subscriber for the relay's
  `/v1/stream` endpoint, with reconnect.
- **`client_core::crypto`, `client_core::key_exchange`** — AES-256-GCM,
  X25519 ECDH, HKDF-SHA256. Used for end-to-end encrypted clip payloads
  and cross-device key transfer.
- **`client_core::credstore`, `client_core::auth`,
  `client_core::auth_session`** — Keychain-first credential storage with
  plaintext fallback, plus the device-code login flow.
- **`client_core::config`** — multi-relay-aware `~/.cinch/config.json`
  reader/writer.
- **`client_core::store`** — local SQLite store (rusqlite + bundled
  sqlite), shared between CLI and desktop processes via filesystem locks.
- **`client_core::sync`** — `LocalPusher` for encrypt + push +
  write-through.

## Example

```rust
use client_core::config::ConfigStore;
use client_core::http::RestClient;

let config = ConfigStore::load()?;
let client = RestClient::new(&config.active_relay()?, /* token */ None)?;
```

## Features

- `specta` — derive `specta::Type` on the wire `Device` DTO for use with
  `tauri-specta`. CLI builds leave this off.

## Wire format

`proto/cinch/v1/*.proto` is the single source of truth for every
cross-language DTO. The Go relay (in a separate repo) serves the same
schema. A round-trip test against `testdata/wire-vectors.json` runs in
both languages so the wire format stays byte-equivalent.

```bash
cargo test -p cinchcli-core --test wire_vectors
```

## License

MIT — see [LICENSE](LICENSE).
