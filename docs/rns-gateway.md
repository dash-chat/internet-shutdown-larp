# Dash Chat — RNS Mailbox Gateway

Syncing Dash Chat mailboxes over LoRa using the Reticulum Network Stack
(RNS), so mailbox-to-mailbox sync keeps working in off-grid and
internet-shutdown conditions. In this repo it carries the relative's link
(the on-map Pi ↔ the Riverside Pi, design.md §4); the gateway itself is
game-agnostic. Implemented in `gateway/rns_gateway.py`, deployed by
`nix/rns-gateway.nix`, radios flashed with `just lora::flash-rnode`.

## Goal

Let two (or more) mailbox nodes synchronise over a LoRa radio link with no IP
connectivity between them, while changing **nothing** in the existing mailbox
sync logic. The mailbox continues to speak plain HTTP; a gateway process makes
a remote mailbox reachable as if it were a local HTTP peer, relaying the
traffic over the radio.

## Topology

```
LoRa peer ↔ [Heltec / RNode] ──serial──> [Pi: RNS gateway] ──HTTP──> [mailbox]
```

- **RNode** (RNode firmware flashed onto the Heltec via `rnodeconf
  --autoinstall`) is a *modem*, not an API. In normal (host-controlled) mode
  it just does the LoRa TX/RX and speaks a KISS-like serial protocol. Radio
  parameters (frequency, SF, bandwidth, TX power) come from the Reticulum
  config the gateway generates from `lora.env` on every boot — the radio
  itself needs no per-game provisioning beyond the firmware flash.
- **RNS** — the Reticulum stack — runs on the **Pi**, in-process in the
  gateway, with the RNode as its only interface (transport off, no
  AutoInterface: the station LAN must not become a mesh segment).
- The **mailbox** (`replicating-local-mailbox-server`) is untouched and never
  learns that RNS exists. It talks HTTP to local URLs that happen to relay
  over LoRa.

## Key decisions

### 1. RNS as dumb transport, not LXMF

We use Reticulum purely to move opaque bytes. We do **not** adopt LXMF: its
propagation-node model overlaps the mailbox's own store-and-forward, and
adopting it would mean two store-and-forward layers with two identity systems.
The mailbox stays the source of truth; RNS is the pipe.

### 2. Application-layer HTTP relay, **not** a TUN/IP tunnel

The gateway does **not** tunnel TCP/IP over the radio. Instead it relays HTTP
at the application layer:

1. Serialize the incoming HTTP request to msgpack (`{m, p, h, b}`).
2. Ship the bytes over an RNS request/response exchange (`link.request`).
3. On the far side, reconstruct and re-issue the request as a **fresh local
   HTTP call** against that node's mailbox.
4. Pack the HTTP response back the same way (`{s, h, b}`).

TCP terminates locally at both ends; only HTTP *semantics* cross LoRa.

**Why not TUN:** MTU is not the obstacle — RNS `Resource` fragments, sequences
and reassembles arbitrary-size payloads transparently. The obstacle is
**TCP-over-radio**: a 1.5-RTT handshake means tens of seconds just to connect
at LoRa RTTs, and Linux's 200 ms minimum RTO fires long before a legitimate
ACK returns, causing spurious retransmits that double airtime and can
duty-cycle-lock the radio. Reticulum is deliberately *not* IP for exactly
these reasons; tunneling IP back over it discards the fit.

### 3. Port-per-peer via mDNS

Each discovered LoRa peer is advertised on the LAN as its **own
`_dashchat._tcp.local.` service on its own port**. The mailbox manager then
discovers each peer through the exact LAN discovery path it already uses,
treating a LoRa peer as indistinguishable from a local one. Because each peer
has a dedicated listener, the request handler closes over that peer's
`dest_hash` — **the port itself is the routing**; no path- or header-based
dispatch.

The mDNS instance name is the far mailbox's MailboxId (carried in the RNS
announce's `app_data`), which is exactly what the manager keys peers by. The
A records include loopback — the manager's discovery TCP-probes routable
addresses first and falls back to loopback, which is precisely the same-host
gateway case.

Trade-off accepted: one port + one small listener per peer (stable ports
persisted in `ports.json`). Fine for a LoRa neighbourhood; a shared-port +
TXT-record scheme would scale to hundreds at the cost of the manager having
to read TXT.

### 4. Never block the manager on the radio (resolved sub-decision)

The mailbox's HTTP client has a **hard 10-second timeout**
(`mailbox-client/src/lib.rs`), far below LoRa round-trip times, and its sync
manager consumes fetch responses **inline** — it cannot use a `202 +
callback`. So the per-peer proxy blocks on neither model and adapts per
endpoint, keeping every sync decision in the mailboxes:

- **`POST /blips/get`** — *cache-and-refresh.* Responses are cached keyed by
  the request bytes. Hit → returned inline, well inside the timeout. Miss →
  `503` now and a background radio exchange; the manager polls each topic
  every ~30 s with the same heights (same key) until data lands, so the next
  poll gets the real response. One radio round trip of extra latency per
  sync step, no protocol change.
- **`POST /blips/store`** — *store-and-forward.* `201` immediately; the
  publish is relayed in the background with retries. Blip inserts are
  idempotent, and the far mailbox keeps listing what it still lacks in
  `missing`, so the manager itself re-drives anything that got dropped.
- **`POST /blobs/store`** — answered locally with "already stored" for every
  hash: blips only, **no blob bytes over the radio** (players learn: photos
  don't reach Anna). `/peers/register` → local `204` (no iroh dialing across
  LoRa). `/health` → canned local answer (the manager never health-checks
  LAN peers; this is for humans with curl).

## Components

### Serving side (answering remote peers)

- **Periodic announce** of the local mailbox destination (aspect
  `dashchat.mailbox`), `app_data` = the local MailboxId — tiny, since
  announces cost airtime. Interval minutes-scale (default 5 min), plus one
  immediate announce when a brand-new peer is heard (rate-limited), so two
  freshly-booted gateways converge fast.
- **HTTP-relay request handler** registered on the local RNS destination
  (`/http`): unpacks a serialized request, replays it against the local
  mailbox over loopback, packs the response back.

### Discovery side (reaching remote peers)

- **Announce handler** (`aspect_filter = "dashchat.mailbox"`): on hearing a
  peer's announce, allocate/reuse its port, stand up its per-peer HTTP
  listener, register its mDNS service under the announced MailboxId.
- **Per-peer proxy listener**: applies the endpoint policies above; radio
  exchanges go through one global worker queue (one radio, serialized use).

### Shared infrastructure

- **Warm link cache** — one `RNS.Link` per peer, reused across sync cycles;
  link establishment is several LoRa round trips, never paid per request.
  Rebuilt on `closed_callback`.
- **Reaper with grace period** — withdraw a peer's mDNS service and close its
  listener only after a real timeout (default 15 min), not on the first
  missed announce: LoRa announces are lossy, and a flapping record makes the
  manager chase ghosts. Ports stay reserved.
- **Persisted RNS identity** — the mailbox's `dest_hash` *is* its mesh
  identity; if it churned on restart, every peer would re-register it as
  brand new (`data_dir/rns_identity`).
- **Persisted `dest_hash → port` map** — stable ports across gateway restarts
  (`data_dir/ports.json`), avoiding mDNS churn.

## RNS primitives used

- **`RNS.Link(destination)`** — encrypted session between two destinations.
- **`register_request_handler` / `link.request`** — the request/response pair
  the relay rides on; HTTP's shape maps one-to-one onto it.
- **`RNS.Resource`** — responses larger than a packet auto-upgrade:
  fragmentation, sequencing, checksumming for free.
- `set_proof_strategy(PROVE_ALL)` for delivery confirmation.

## Byte-budget discipline

Every byte is metered airtime under the EU868 duty cycle:

- **Allowlist headers, never denylist.** Only `Content-Type` crosses the
  radio (mailbox sync carries auth *in the body*, signed over the mailbox
  key). `Host`, `Content-Length`, `User-Agent`, `Accept-*` etc. are envelope;
  the far HTTP client regenerates what it needs.
- **Reconstruct semantically, don't ship raw wire bytes** — msgpack fields,
  not `POST /… HTTP/1.1\r\n` text.
- **Compression stays on** (RNS Resource default), deviating from the
  original draft: these bodies are base64-blips-in-JSON, not raw ciphertext,
  and zlib reliably claws back the base64 inflation (~25%) plus the JSON
  envelope.

## Division of responsibility

- **Mailbox owns persistence and trust.** It verifies app-layer signatures
  and performs the sync. Because auth is enforced there, the RNS endpoint
  and the LAN proxy ports are open to all callers — nothing is trusted at
  the transport layer.
- **Gateway owns RNS state.** Persisted identity, announce cadence, link
  lifecycle, retries, mDNS advertisement. It relays opaque bytes and never
  inspects a blip.

## Resolved questions (as built)

- *Inline or register-then-schedule?* The manager fetches inline with a 10 s
  timeout and syncs every subscribed topic against every registered peer
  every ~30 s → the cache-and-refresh / store-and-forward split above.
- *Sync trigger today?* Polled: the `Mailboxes` manager re-syncs on its own
  clock; new topics are discovered from watermarks
  (`enumerate_topics_loop`). The gateway needs no "sync now" hook.
- *Headers beyond Content-Type?* None — verified against
  `ToyMailboxClient`: JSON bodies, signature material in-body, no auth
  headers, no conditional requests.
- *Native Rust vs sidecar?* Python sidecar now (the `rns` PyPI package is
  the reference implementation; in nixpkgs, unfree license). The Rust-native
  path — `reticulum-rs`, dropping the sidecar, loopback hop and mDNS layer by
  registering RNS peers in-process — stays open; audit its
  request/response-handler API first, the whole relay hangs on it.

## Deployment in this repo

Baked into every station image, gated like the bot: a card whose FAT boot
partition carries `lora.env` (radio parameters; see `just lora::flash-near /
flash-far`) runs the gateway, any other card doesn't. The relative-far card
also broadcasts an out-of-play AP: the gateway↔mailbox mDNS hop needs a live
multicast interface, and the bubble doubles as facilitator debug access in
the far location. Its SSID is a random id and it is password-protected
(both printed by `flash-far`) so it doesn't read as part of the game.
