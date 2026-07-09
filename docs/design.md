# Town-fire LARP — design document

A live-action game about carrying information when the network is gone.
Players are couriers in a town cut in two by fires; Raspberry Pi "stations"
running Dash Chat mailboxes are the only communication infrastructure left,
and bots impersonating town characters produce messages that players must
physically carry to their destinations.

This document is the plan of record for what we build next. It builds on the
[raspberry-pi-mailbox-server](https://github.com/dash-chat/raspberry-pi-mailbox-server)
repo (the plain Pi AP + mailbox appliance image, consumed here as the
`mailbox-image` flake input) and on the `dashchat-node` crate from the
dash-chat repo (the headless chat node the bots are built on).

---

## 1. Narrative & game mechanics

Fires have broken out across the town. All networks are down; only a handful
of solar-powered relief stations survived, each hosting a short-range Wi-Fi
mailbox. Worse, a wall of flames cuts straight through town: each member of
a player pair can only move on their own side. The only shared point is the
**base station** in the firebreak at the center of the map.

Four characters live at the stations and keep producing urgent messages, each
with a clear recipient ("We detected a fire near Orange Street! Please get
this message to the firefighters!"). Players deliver a message by physically
walking into the destination station's Wi-Fi bubble so their phone syncs it
into that station's mailbox — where the character's bot sees it and replies
with a clear success message ("Okey! Thanks for bringing this to us, we'll get
right on it!"). Messages that originate on the far side of the fire must be
relayed: carrier walks it to the base station, partner picks it up there and
carries it to the destination.

### Physical layout (2×2 grid, fire line down the middle)

```
   FIREFIGHTERS                          HOSPITAL
   (Pi: AP + mailbox + bot)             (Pi: AP + mailbox + bot)
        ┌────────────────────╥────────────────────┐
        │                    ║                    │
        │    Player A's      ║     Player B's     │
        │      side          ║       side         │
        │                BASE STATION             │
        │      (Pi 5 hosting its own Wi-Fi AP,    │
        │       the mayor captive portal and      │
        │       the mailbox — reachable from      │
        │       both sides; QRs on the wall)      │
        │                    ║                    │
        └────────────────────╨────────────────────┘
   LINK to RIVERSIDE (nearby town)       UPLINK to the JOURNALIST
   (near Pi: AP + mailbox + LoRa)       (phone hotspot with internet; the
        ~ LoRa link ~                    journalist herself is OUTSIDE the
   RIVERSIDE (far Pi: mailbox +          town — her bot runs on Digital
   Aunt Anna bot + LoRa)                 Ocean via the existing cloud mailbox)
```

Corner assignment is arbitrary — the only requirement is two characters per
side so both players have destinations. The fire line (║) is a rule, not a fence:
players agree not to cross it.

### The cast

| Character | Persona | Infrastructure |
|---|---|---|
| **firefighters** | **Cindy the firefighter**, at the brigade HQ | Pi 5: Wi-Fi AP + mailbox + bot |
| **hospital** | **James the nurse**, at the town hospital | Pi 5: Wi-Fi AP + mailbox + bot |
| **journalist** | **Marta the journalist** — news desk **outside the town**, telling the world what's happening inside; the hotspot corner is the town's only surviving uplink to her | Phone hotspot (internet); bot on a Digital Ocean droplet syncing through the **existing cloud mailbox** |
| **relative** | **Aunt Anna**, a relative in Riverside, the nearby town, desperate for news of her family | Two Pi 5s: the on-map one is AP + mailbox + RNS gateway; the far-away one runs mailbox + bot + RNS gateway. Messages to/from Anna take a LoRa round-trip (§4) |
| *(base station)* | **The town mayor** — captive portal only, not a chat character | Pi 5 running the `base-station` image: hosts its own Wi-Fi AP like the character stations and serves the mayor's captive portal + the mailbox. *(The mAP-lite-as-AP variant — a MikroTik mAP lite broadcasting the wifi with the Pi wired behind it, `nix/base-station.nix` — is kept but currently unused, in case the Pi's radio can't carry the 30-40 concurrent base-station clients)* |

Total hardware: **5 × Pi 5** (base, firefighters, hospital, relative-near,
relative-far) + **2 ×
Heltec LoRa dev kits flashed with RNode firmware** (USB-C serial on the two
relative-link Pis) + **1 phone**
(journalist hotspot) + **1 DO droplet** (already running the cloud mailbox;
gains the journalist bot).

### Game setup (at the base station)

The game begins at the base station — the mayor's office. Players join the
base station's Wi-Fi and the captive portal opens: **the town mayor** explains
the fires, pleads for help, and gives the first instructions —

1. Add **each other** as Dash Chat contacts (mutual QR scan, in person).
2. Add the four characters as contacts by scanning their **QR posters on the
   wall** around the base station.
3. Create a **group** containing: both players + the four characters.
4. Split up — one player per side of the fire line — and start carrying messages.

The base Pi's mailbox is on that same Wi-Fi, so the group is seeded into the
base mailbox immediately.

The contact requests and group invitations only *reach* each bot when a player
first syncs at that bot's station — that's fine and thematic: each character
"comes online" when first visited, greets the group in character, and starts
producing missions. There is no separate facilitator trigger (auto-start on
group join was the chosen design).

**Game rule that must be enforced socially:** players keep **mobile data off**
(Wi-Fi on) and **forget all other saved Wi-Fi networks**. A phone with LTE
would sync everything through the cloud mailbox from anywhere and
short-circuit the entire sneakernet, and a phone with saved networks in range
keeps auto-switching away from the station APs.

### Play loop

1. A bot, for each group it's in, fires a mission message at a random interval
   (default: uniform 3–8 min, configurable), drawn from its character's
   template pool. Every mission names its destination character in the prose
   itself ("…get this message to the firefighters!") — there is **no visible
   machine metadata**; recognition works by author identity + known template
   text (see §3).
2. Players walking into a station's AP bubble auto-sync (Dash Chat's existing
   mDNS local-mailbox discovery) — they pick up whatever blips that mailbox
   holds and deposit whatever they carry.
3. When the destination character's bot sees a mission addressed to it
   (authored by a known cast bot, text matching a template with `to = me`),
   it replies once with that template's in-character success message.
4. The ack travels back through the same courier network, so the pair sees
   their success confirmed in the group chat.

To avoid flooding, a bot keeps **at most one outstanding unacked mission per
group** — it pauses its timer until the ack comes back.

Ending: the facilitator calls time; the group chat itself is the score sheet
(count success replies). No formal end state in software.

### The anonymous informant (side plot)

A hidden sixth character, **Anonymous**, has an identity but **no scenario
pack and no cast entry** — no other character knows he exists
(`characters.just` keeps him out of `larp-cast.toml`). His QR poster is hidden
somewhere on the map instead of hanging on the base-station wall.

A player who scans it sends a contact request that travels, like everything
else, through the mailboxes in players' pockets. **Two** stations run the
informant daemon (`larp-bot anonymous`): `just characters::flash` always arms
the firefighters card with the `portal` variant and the hospital card with the
`code` variant — one per side of the fire line. When a request reaches one of
them, the bot accepts
and whispers into the direct chat: the mayor is lying — he lit the fires, he
shut down the internet, and he is using the emergency to control the town.
Then each station adds *its half* of the secret (`anonymous.toml`):

- the **portal** station (firefighters): the mayor's head on his portrait in
  the base-station captive portal is the secret trigger — five taps in a row
  reveal a hidden password prompt;
- the **code** station (hospital): the password — `ahawegotyou`.

The pair must combine their halves. Five taps on the portrait's head reveal
the prompt; entering `ahawegotyou` replaces the mayor's broadcast with the
endgame page: his files are out and **the mayor flees town**.

Both stations run the **same** identity (one printed poster). p2panda logs
are per `(device, topic)`, and a failed op ingest is dropped per-op, so the
two instances only ever collide on the announcements topic (both branches
carry the same "Anonymous" profile — first one in wins) and on a direct chat
when both stations accept the *same* player (that player keeps the first
station's chat; the pair still assembles both hints through their own
accepts). Degradation, not breakage.

---

## 2. What already exists (reused unmodified)

- **The `mailbox-image` flake input** (raspberry-pi-mailbox-server): NixOS SD
  image for Pi 5 with `hostapd` AP (range-limited, RSSI-gated), dnsmasq,
  captive portal, and the `replicating-local-mailbox-server` from the
  dash-chat flake input. Per-card configuration via env files on the FAT boot
  partition (`wifi-ap.env`). This repo's station image is that image
  `extendModules`-ed with the bot.
- **mDNS announce/discovery**: stations announce `_dashchat._tcp.local.`, so
  players' apps auto-discover the mailbox when they join a station's Wi-Fi.
- **Mailbox replication** (`replicating-local-mailbox-server`): bidirectional
  `/blips/get` sync of known topics between mDNS-discovered mailboxes. On
  this map stations are out of each other's range, so LAN replication is
  idle — but it's exactly the machinery the LoRa gateway rides: a LoRa peer
  is surfaced to the manager as one more mDNS service (§4).
- **`dashchat-node`** (dash-chat repo): headless node with everything a bot
  needs — `new_qr_code()` / `add_contact()`, auto-join of group invitations
  (already handled in stream processing), `send_message()`, `get_messages()`,
  and a `Notification` mpsc channel that streams every processed operation
  (header + payload) to the embedding application.
- **Cloud mailbox**: already running; the journalist bot and any
  hotspot-connected player sync through it.
- **mAP lite tooling (this repo, currently unused)**: `just
  base-station::map-lite::provision` turns a stock device into the
  base-station AP (ether1 bridged to the Pi, DHCP off, range clamped; the Pi
  serves the portal, see `nix/base-station.nix`). Kept in case the Pi's own
  AP can't carry the base-station load; for now the base station hosts its
  own Pi wifi like every other station. The generic `../map-lite-portal`
  repo is no longer involved; the mayor page is `portal/index.html` here.

## 3. New component: `larp-bot` crate

A new Rust crate **in this repo** (new `crates/` workspace), depending on
`dashchat-node` as a **git dependency pinned to the same rev as the flake's
`dash-chat` input** (the message/payload format must match the app version
players run — version skew here is the #1 way to break the game silently).

One binary, one character per process:

```
larp-bot keygen --out larp-identity.toml           # provision an identity bundle (run on the laptop)
larp-bot qr     --identity larp-identity.toml --out qr.png   # derive the printed QR (offline, no Pi needed)
larp-bot run    --config /etc/larp-bot/config.toml # the daemon (loads the flashed bundle)
```

### Flashable identity (survives wipes and re-flashes)

The character's identity is **not** generated on the Pi — it's a small
**identity bundle** generated once on the laptop and flashed onto each card's
FAT boot partition alongside `wifi-ap.env`/`larp.env`. Re-flashing the image
or wiping `/var/lib/larp-bot` must never invalidate the printed QR posters.

What has to be in the bundle (all three, or a wipe kills the QR):

- the **device private key** (ed25519 seed — `NodeKeys.private_key`),
- the **agent id** (random, generated once at `keygen` time — upstream derives
  it from a throwaway key on first run, so it's not recoverable from the
  device key),
- the **inbox topic id + expiry** — the printed QR points contact requests at
  this topic, and stream processing drops requests whose topic isn't in the
  local store's `active_inboxes` table. A surviving key with a lost inbox
  topic still means a dead QR.

On every boot the bot loads the bundle from `/boot/firmware/`, passes the
reconstructed `NodeKeys` to `Node::init` (which accepts them directly), and
idempotently re-registers the bundle's inbox topic as active
(`node.local_store` is public, so `add_active_inbox_topic` + topic
initialization need no upstream patch). `/var/lib/larp-bot` is thereby demoted
to a cache: after a wipe the bot forgets group memberships and ack-dedup
state, but players can simply re-scan the *same printed QR* and re-invite it —
the posters stay valid for the character's lifetime.

The bundle sits plaintext on the FAT partition; for a game prop that's fine.

### Responsibilities

- **Contact QR with long expiry.** The bundle's inbox expiry is set long
  (e.g. 1 year), overriding the short `contact_code_expiry` default. The `qr`
  subcommand derives the `QrCode` (device pubkey, agent id, inbox topic) from
  the bundle alone — so the wall posters can be printed before any Pi ever
  boots — and must encode it **exactly as the app encodes it** (reuse the
  app's serialization; verify against a real phone scan early).
- **Auto-accept contacts.** Watch the `Notification` stream for
  `InboxPayload::ContactRequest { code, .. }` and call `add_contact(code)` to
  complete the handshake. (Group invitations need nothing: stream processing
  already auto-joins.)
- **Greeting.** On joining a new group, send the character's in-character
  intro line ("This is Mercy Hospital, we're overwhelmed, please help…").
- **Scenario engine.** Per group: a timer loop firing at
  `rand_range(min_interval, max_interval)`, drawing a not-yet-used template
  from the character's pool (reshuffle when exhausted), holding fire while
  the group's one pending mission awaits its ack.
- **Mission recognition & acks — no visible metadata.** Messages are pure
  in-character prose; the machine layer rides on facts both ends already
  know, since we author every bot:

  - All bots ship with **all four template packs** (they live in this repo)
    and a **cast file**: each character's public agent id, extracted from the
    identity bundles at `keygen` time.
  - *Recipient side:* a group message is a mission for me iff its **author is
    a cast bot's agent id** and its text **exactly matches** a template with
    `to = <my character>`. Then reply once with that template's success line.
    Players typing identical text can't spoof this — they aren't the signing
    author.
  - *Origin side:* a mission counts as delivered when a message **authored by
    the recipient character's agent id** matches that template's success
    line. Success lines must be unique within each pack (enforced by a test),
    and templates never repeat within a group, so the ack→mission mapping is
    unambiguous.
  - *Dedup:* the recipient persists the **header hash** of every mission
    operation it has acked (sqlite/file), so restarts and re-syncs don't
    double-ack. Hashes exist only in the protocol layer — nothing machine-ish
    ever appears on screen.
- **Mailbox wiring.** The node's `Mailboxes` manager is pointed at exactly one
  mailbox URL: `http://127.0.0.1:<port>` on the Pis, the cloud mailbox URL on
  the DO droplet. No iroh internet connectivity is assumed on the Pis (offline
  LAN blob sync already works per the mailbox-image repo's README; missions
  are text-only anyway).

### Configuration (`config.toml`)

```toml
character   = "firefighters"          # persona selection
mailbox_url = "http://127.0.0.1:8080"
identity    = "/boot/firmware/larp-identity.toml"  # flashed bundle (see above)
cast        = "/etc/larp-bot/cast.toml"            # all characters' public agent ids
data_dir    = "/var/lib/larp-bot"                  # cache only — safe to wipe

[timing]
min_interval_secs = 180
max_interval_secs = 480

[templates]                            # per-character scenario file
path = "/etc/larp-bot/firefighters.toml"
```

Template file: a list of `{ to = "hospital", text = "…", success = "…" }`
entries plus `greeting`. Authored in Spanish/Catalan/English as needed — pure
content, no code. All four character packs live in this repo under
`scenarios/`, and every bot loads all of them (recognition depends on it —
see above). A unit test lints the packs: `text` and `success` unique across
each pack, `to` values valid.

A pack may also carry an optional `[comeback]` (`after_secs` + `text`): after
that long without any *player* message in a group, the character answers the
next player message with `text`, once per quiet spell. Only Aunt Anna uses it
("Hey! How is everything over there?") — a sign of life from the far end of
the LoRa link when players resurface. Tracking is in-memory and baselined on
the first scan, so bot restarts never trigger it.

## 4. New component: the RNS mailbox gateway (`gateway/`)

*Implemented — full design in [rns-gateway.md](rns-gateway.md).*

Carries mailbox sync between the relative's two Pis over **Heltec dev kits
flashed with RNode firmware** (`just lora::flash-rnode`) — beyond Wi-Fi
range, no infrastructure. The Reticulum Network Stack (RNS) runs on each Pi
with the RNode as a serial-attached modem; a Python sidecar (the gateway)
relays mailbox HTTP over RNS request/response exchanges. In short:

- **The mailbox is untouched.** Each discovered LoRa peer is advertised on
  the station's LAN as its own `_dashchat._tcp.local.` mDNS service (the
  instance name is the far mailbox's MailboxId, carried in the RNS
  announce), so the mailbox manager discovers and syncs it through the exact
  LAN path it already uses — a LoRa peer is just a peer with a funny URL.
- **HTTP relayed at the application layer, never TCP-over-radio.** Requests
  are packed semantically (msgpack, allowlisted headers), shipped over an
  encrypted RNS link, and re-issued as fresh loopback HTTP calls on the far
  side; RNS `Resource` handles fragmentation of multi-KB responses.
- **The manager's 10 s HTTP timeout never meets the radio**: `/blips/get` is
  answered from a request-keyed cache filled by background radio exchanges
  (the manager's ~30 s re-poll picks up the result); `/blips/store` returns
  201 and relays in the background (blip inserts are idempotent and the far
  side keeps re-listing what it lacks, so drops self-heal).
- **Text blips only, no blobs**: blob announcements are answered locally
  with "already stored" (players learn: "photos don't reach Anna").
- **Topic seeding for free**: far-side blips are POSTed into the local
  mailbox, which creates watermarks for unknown topics — the mailbox's own
  topic enumeration takes it from there.

Deployment: `rns-gateway` runs on both relative-link Pis, gated on
`/boot/firmware/lora.env` (radio parameters; see `just lora::flash-near /
flash-far`).

## 5. NixOS & deployment changes

### One image, per-card station selection

Keep the single-SD-image philosophy. **Implemented:** the bot service
(`nix/larp-bot.nix`, baked into every image with `services.larp-bot`) is gated
at runtime with `ConditionPathExists` on two files the FAT boot partition may
carry, next to `wifi-ap.env`:

- `larp-identity.toml` — the character's flashed identity bundle
- `larp-cast.toml` — the public cast file (flashed too, **not** baked into the
  image: it changes per game, the image doesn't)

No file → no bot: the card is a plain mailbox appliance. The LoRa gateway
follows the same convention with its own file — `lora.env` (radio
parameters) — so no `STATION=` switch is needed; the station variants are
just combinations of flashed files:

| Station | mailbox | AP (hostapd) | larp-bot | rns-gateway |
|---|---|---|---|---|
| base | ✓ | ✓ (`base-station` image) | – | – |
| firefighters / hospital | ✓ | ✓ | ✓ (identity flashed) | – |
| relative-near | ✓ | ✓ | – | ✓ (lora.env flashed) |
| relative-far | ✓ | ✓ (out-of-play: random SSID, password-protected; the gateway↔mailbox mDNS hop needs a live multicast interface, and it doubles as debug access) | ✓ (identity flashed) | ✓ (lora.env flashed) |

The base station Pi runs the `base-station` image (`just base-station::build`):
the station image with the captive portal re-enabled and the mayor page in
place of the generic captive-portal SPA — it is the only station with a
portal at all. It hosts its own wifi like every other station —
`just base-station::flash` writes the `wifi-ap.env` (SSID
`internet-shutdown-larp` by default).

### Base station: mayor portal

- **Mayor page** *(implemented)*: `portal/index.html` in this repo — a single
  static page (mayor's speech + portrait + step-by-step instructions + a
  mailbox health check via the module's `/api/` proxy), no build step. The
  `base-station` config overrides `dashchat.captivePortal.package` with it.
  The portrait hides the informant side plot's endgame: tapping the mayor's
  head five times in a row reveals a hidden password prompt, and entering
  `ahawegotyou` swaps the broadcast for the mayor-flees-town page
  (per device, remembered in `localStorage`).
- **Nothing is gated**: the portal is onboarding UX, and every client
  (phones, headless Pis) reaches the mailbox without logging in to anything.

*(Currently unused alternative)* If the Pi's brcmfmac AP can't carry the
30-40 concurrent base-station clients, the mAP-lite variant is kept:
`nix/base-station.nix` (re-add it to the `base-station` modules in
`flake.nix`) makes the Pi host no wifi and instead own DHCP + wildcard DNS
on the cable to a MikroTik mAP lite, provisioned as a plain AP with
`just base-station::map-lite::provision` (ether1 bridged to the Pi, DHCP
off, range clamped natively — RouterOS tracks per-client signal, unlike the
Pi's fail-open RSSI gate). mDNS passes the mAP's L2 bridge, and no RouterOS
hotspot is involved.

Also per-station: the AP SSID defaults to the station name plus a game
suffix (`SSID=firefighters-larp` etc. via `wifi-ap.env`),
so the facilitator can see at a glance which bubble they're in. Character
stations run **no captive portal** (`dashchat.captivePortal.enable = false`
in the station image): joining one looks like a dead network, and the app
still finds the mailbox via mDNS + its own port. Only the base station pops
a portal (the mayor).

`larp-bot` builds with `rustPlatform.buildRustPackage` from this repo's
workspace (git deps via `cargoLock.allowBuiltinFetchGit`, so no outputHashes
to maintain), exposed as flake packages for x86_64 (dev/DO) and aarch64 (Pi);
`rns-gateway` is a Python environment around `gateway/rns_gateway.py`
(`nix/rns-gateway-package.nix` — nixpkgs' `rns` needs an unfree allowance,
see the flake). Scenario packs (`scenarios/`) are pure repo content baked
into the image at `services.larp-bot.scenariosDir`.

Provisioning flow (all offline, on the laptop — implemented as `just` recipes):

1. `just characters::generate` — one identity bundle per scenario pack into
   `secrets/` (gitignored; existing bundles are kept, since re-generating
   would invalidate the printed posters), plus the public
   `secrets/larp-cast.toml` assembled from all of them. Idempotent, and the
   cast is complete by construction: the character list *is*
   `scenarios/*.toml`.
2. `just characters::posters` — renders the QR wall-poster PNGs for printing.
4. `just characters::flash <character> /dev/sdX` — flashes the station image and
   puts the character's files (`wifi-ap.env` with `SSID=<character><ssid_suffix>`,
   **open network** unless a password argument is given,
   `larp-identity.toml`, `larp-cast.toml`, assembled on the fly from
   `secrets/`) on the card's boot partition.

The base station's portal can additionally serve the mayor's QR as a
fallback onboarding path (the character stations run no portal).

**Seed the base mailbox with the cast's profiles** (once, after the bots have
booted): each character's profile lives on its bot's announcements topic,
seeded only at its own station (Marta: only in the cloud) — and replication
never introduces a mailbox to topics it doesn't know. Without seeding,
contacts added from the wall posters appear *nameless* at the base station,
right when players are telling the four characters apart to create the group.
The fix uses the client push path: on a phone with internet, add all four
characters (Marta's profile arrives via the cloud; the others via their
stations — or plug all the Pis into one ethernet switch, where the mailboxes
discover and push to each other), then stand on the base station's Wi-Fi for
a minute. The phone pushes all four announcements topics into the base
mailbox, permanently. Do NOT seed by running a character bot against the
base mailbox with a fresh data dir — same identity, second op log, forked
history.

### Journalist: cloud host (or laptop)

The journalist is just the same `larp-bot` service pointed at the cloud
mailbox — no new mailbox is deployed (chosen design), and the phone hotspot
needs zero config: any internet gets players to the cloud mailbox, which the
app already knows about.

*Implemented:* `just journalist::deploy` provisions the whole thing with
doctl — first run creates an Ubuntu droplet whose cloud-init converts it to
NixOS in place (nixos-infect), then pushes the flake's `journalist-droplet`
config plus the secrets (same `keygen` artifacts, delivered over SSH to
`/var/lib/larp-secrets/` instead of a FAT partition); later runs skip
straight to the push. `journalist::logs` follows the bot's journal,
`journalist::destroy` tears the droplet down (the identity survives in
`secrets/`). The flake also still exports `nixosModules.larp-bot` with a
usage example (see flake.nix) for wiring the bot into an existing NixOS
host instead — e.g. the droplet already running the cloud mailbox.

For testing without touching the droplet, `just characters::run journalist
[mailbox_url]` runs the bot on the laptop against the cloud mailbox — the
laptop has internet, which is all the journalist needs. State lives in
`.run/journalist/` (wipe it to simulate a reset; identity survives, it's in
`secrets/`).

**Pick the mailbox URL to match the players' app build**: release builds use
the production mailbox, dev builds may point at staging. A journalist synced
to a different cloud mailbox than the players' apps never sees their group.

## 6. End-to-end message walk-through (sanity check)

Hospital bot fires: *"Injured people trapped on Elm St — get this to the
firefighters!"* into the group topic, via the hospital Pi's localhost
mailbox — plain prose, no visible metadata.

1. Player B (east side) visits the hospital bubble → phone syncs the blip.
2. B walks to the base station → phone deposits it into the base mailbox.
3. Player A (west side) visits base → picks it up.
4. A walks to the firefighters bubble → deposits into the firefighters mailbox.
5. The firefighters bot's node polls its localhost mailbox and sees a message
   authored by the hospital's known agent id whose text matches a template
   with `to = "firefighters"` — it replies *"Okey! Crews dispatched to Elm
   St, thanks!"* (that template's success line).
6. The ack rides the same courier chain back; both players see it, and the
   hospital bot decrements its outstanding count when a message authored by
   the firefighters' agent id matching that success line reaches *its*
   mailbox.

For Aunt Anna, steps 4–5 gain a LoRa hop (near-Pi → far-Pi) before the bot
sees it, and the ack hops back. For the journalist, step 4 is "join the
hotspot", the deposit goes to the cloud mailbox, and the DO bot answers
usually within seconds.

## 7. Risks & open questions

- **Nameless contacts at the base station** — profiles ride each bot's
  announcements topic, which the base mailbox doesn't know until seeded (see
  the seeding step in §5). Re-seed if a profile ever changes.
- **QR encoding fidelity** — the printed QR must decode in the real app.
  Verify with a phone in week 1; this gates the whole onboarding flow.
- **QR/inbox expiry semantics** — besides the bundle's inbox expiry, check
  that nothing else garbage-collects the bot's inbox topic before game day.
- **Post-wipe state loss** — a wipe preserves identity (flashed bundle) but
  loses group memberships and ack-dedup, so a mid-game re-flash means players
  re-invite the character (same printed QR) and already-acked missions may be
  acked twice. Acceptable; don't wipe mid-game.
- **dashchat-node offline behaviour** — the node embeds iroh/p2panda
  networking that may want internet (relays, DNS). Must verify a node on an
  offline LAN talking only to a localhost mailbox is healthy. (The mailbox
  side is already proven offline; the *node* side is not.)
- **Ack routing asymmetry** — an ack is just another group message; nothing
  guarantees players carry it back. Acceptable (it's gameplay), but templates
  should nudge: "let the hospital know we got this!"
- **LoRa link throughput** — verify real-world sync latency under the EU868
  duty cycle with a two-RNode bench test (`just lora::run` on the laptop):
  a group's initial history is tens of KB and will take minutes to cross;
  steady-state missions (~300 B) should take seconds. Tune the announce
  interval and cache TTL accordingly (docs/rns-gateway.md).
- **Clocks** — Pi 5 has an RTC header but no battery by default; offline Pis
  wake with wrong time. Blip ordering must not depend on wall clock across
  devices (p2panda ordering is causal, so likely fine — verify), and the
  bot's random timers only need monotonic time. QR expiry comparison uses
  wall clock though — set expiry to years, not days.
- **Player phones auto-leaving the AP** — phones drop Wi-Fi networks with no
  internet. The base station's captive portal mitigates there; character
  stations now run no portal, so this risk is live on them — test with the
  actual target phones.
- **Base station hotspot plumbing** — the mailbox Pi must be reachable by
  phones through the RouterOS hotspot (MAC bypass via ip-binding) and mDNS
  multicast must cross the hotspot bridge; verify both with real hardware
  before game day (milestone 2).

## 8. Implementation milestones

1. **`larp-bot` core** — workspace scaffolding, config, `keygen`/`qr`
   (offline identity bundles), bundle loading + inbox re-registration,
   auto-accept, greeting, scenario engine, mission/ack recognition (author id
   + template matching). E2E test on a laptop:
   two `dashchat-node` test instances + one local mailbox + one bot; assert a
   mission → courier(simulated) → ack round-trip, then wipe the bot's data
   dir, restart it, and assert the same identity/QR still onboards.
2. **Nix integration + base station** — `nix/larp.nix`, `larp.env` station
   switch, per-station env dirs with flashed identity bundles, packages,
   image build for a bot station; the base-station image (mAP lite as AP,
   Pi wired behind it serving DHCP/DNS + portal); live tests:
   phone joins a bot station's AP, scans the printed QR poster, creates
   group, gets greeted, receives mission — and at the base, portal opens,
   phone syncs with the base mailbox through the mAP's bridge.
3. **LoRa link** — *(implemented as the RNS gateway, `gateway/` +
   `nix/rns-gateway.nix` — see docs/rns-gateway.md)*: RNode-flashed Heltecs,
   HTTP relayed over Reticulum, LoRa peers surfaced to the mailbox via mDNS.
   Remaining: two-RNode bench test, then the two relative-link Pis
   end-to-end.
4. **Journalist droplet** — NixOS config on DO against the cloud mailbox;
   test through a real phone hotspot.
5. **Scenario content + dress rehearsal** — write the four template packs,
   full field test (5 Pis + mAP lite), print the QR wall posters and finalize
   the mayor's portal content, tune intervals/caps.
