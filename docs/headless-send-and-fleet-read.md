# Headless `cinch send` + fleet-scoped MCP read

This is the recipe for the agent loop the 0.5 build is designed around:

> A headless box (SSH / Docker / CI) runs `cmd | cinch send`, and a coding
> agent on a **different** machine reads that clip back through MCP with
> `scope:"fleet"`.

Both halves need the **AES-256 master key present on the box**. This page is
the supported way to provision it on a machine that can't do an interactive
browser login.

## The honest limitation (read this first)

There is **no `CINCH_ENCRYPTION_KEY` environment variable** today. The master
key reaches a new device either through the interactive ECDH key-exchange
handshake (which needs a paired, online device) or by being written into
`~/.cinch/config.json`. So a fresh Docker/CI box **cannot** E2EE-send or
fleet-read with `--token`/`--relay` (or `CINCH_TOKEN`/`CINCH_RELAY_URL`)
alone — those carry the token and relay URL, never the key.

`cinch send` **fails fast** rather than pretending to work:

| Box state | `cinch send` result |
|---|---|
| token + relay + **master key** present | sends (E2EE), exit `0` |
| token + relay, key **pending** (paired, awaiting handshake) | exit `6` `ENCRYPTION_PENDING` — run `cinch auth retry-key` |
| token + relay, **no key** (bare provisioned box) | exit `5` `ENCRYPTION_REQUIRED` — provision the key (below) |

It never silently queues an un-encrypted clip on an ephemeral box. The
provisioned-`config.json` recipe below is what turns the exit-5 box into a
working sender + fleet reader.

## Provision `~/.cinch/config.json`

`~/.cinch/config.json` is a multi-relay config. The credential-bearing fields
live in `relays[0]`. The simplest reliable way to build one for a headless box
is to **copy the `relays[]` entry from a machine you've already logged in on**
(your laptop), then change the `hostname` (see the collision note below).

Minimal valid shape (mode `0600`):

```json
{
  "config_version": 1,
  "active_relay_id": "default",
  "relays": [
    {
      "id": "default",
      "label": "hosted",
      "relay_url": "https://api.cinchcli.com",
      "user_id": "<your user id>",
      "device_id": "<this box's device id>",
      "hostname": "ci-runner-7",
      "token": "<this box's device token>",
      "encryption_key": "<base64 AES-256 master key>",
      "device_private_key": "<base64 X25519 private key>"
    }
  ]
}
```

- `active_relay_id` **must** equal the `id` of the relay entry you want active.
- `id`, `label`, `relay_url`, `user_id`, `device_id`, `hostname` are required
  (the file won't parse without them). `token`, `encryption_key`, and
  `device_private_key` are what make `send`/fleet-read actually function —
  copy `encryption_key` and `device_private_key` verbatim from your laptop's
  config so the box shares the same per-user master key.
- Write it with `0600` perms; it carries credentials.

```bash
install -m 600 /dev/stdin ~/.cinch/config.json < config.json
cinch auth status      # confirm authenticated + key present
echo "hi from CI" | cinch send   # exit 0 → it worked
```

> The existing `cinch fleet add <ssh-target>` (was `cinch device pair`) already
> performs a partial version of this provisioning over SSH; this manual recipe
> is the fallback for Docker images / CI where you bake the file in.

### Use a distinct hostname per box

A clip's origin is keyed on `remote:<hostname>`. Two boxes sharing a hostname
(common with default container hostnames like `localhost` or a reused base
image) become **indistinguishable** as sources, which weakens the
`scope:"fleet"` exclude-self filter (a box could see its own clips as
"fleet", or fail to). Set a unique `hostname` in the config (and ideally the
OS hostname) on each provisioned box.

## Reading it back: fleet-scoped MCP

On the **reader** machine (your laptop running Claude Code, or another
provisioned box), point the agent's MCP config at `cinch mcp` and enable the
fleet scope with `CINCH_MCP_FLEET=1`:

```json
{
  "mcpServers": {
    "cinch": {
      "command": "cinch",
      "args": ["mcp"],
      "env": { "CINCH_MCP_FLEET": "1" }
    }
  }
}
```

Then the agent calls `list_recent_clipboard` / `search_clipboard` with
`scope:"fleet"` to read clips that originated on **other** machines (this
device's own clips are excluded). The same master-key requirement applies:
the reader decrypts fleet clips locally, so it must have the key
(fleet clips it can't decrypt are simply skipped, never shown as ciphertext).

### Freshness contract

With `CINCH_MCP_FLEET=1`, the **first** `scope:"fleet"` request triggers a
one-shot, bounded (2s) backfill from the relay, then serves from the local
store. For the rest of that MCP session, fleet reads reflect relay state **as
of that first fleet call** — clips sent *after* it won't appear until the MCP
server is restarted (or a desktop/CLI writer on the box refreshes the store).
Combined with the relay's ~1-day default retention, a fleet clip is visible
iff it was on the relay at the first fleet call and still within retention.
Each returned clip carries `created_at`, so an agent can judge staleness.

Without `CINCH_MCP_FLEET=1`, `cinch mcp` serves only the local store
(no network, no backfill) — `scope:"fleet"` then returns just whatever
remote-origin clips already happen to be local.

## Validating the loop

`scripts/headless-loop-smoke.sh` exercises the full send → fleet-read path
against a configured relay. See its header for required env.
