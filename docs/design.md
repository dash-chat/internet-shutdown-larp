# Town-fire LARP — design document

A live-action game about carrying information when the network is gone.
Players are couriers in a town ravaged by fires; Raspberry Pi "stations"
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
of solar-powered relief stations survived, each hosting a Wi-Fi mailbox. The
stations can't talk to each other any more — their messages must travel in
the players' pockets.

Four characters live at the stations, and each of them talks to the player in
a **private one-to-one chat** — there is no group chat in the game. Every
character keeps producing urgent messages with a clear recipient ("We detected
a fire near Orange Street! Copy this into the firefighters' chat!"). The
player is the wire: they **copy the message out of one character's chat and
paste it into the recipient's chat**, then physically walk into the
recipient's station Wi-Fi bubble so their phone syncs it into that station's
mailbox — where that character's bot recognizes the text and replies with a
clear success message ("Okey! Thanks for bringing this to us, we'll get right
on it!"). **One player is enough**: a solo courier can reach every station;
more players just means more pockets carrying messages.

Delivery is **one-way**: the success reply lands in the recipient's chat and
that is the payoff. The character who handed the message out never learns it
arrived (nothing carries the news back), so it keeps handing out its next
message on a timer.

### Physical layout (stations spread around the play area)

```
   FIREFIGHTERS                          HOSPITAL
   (Pi: AP + mailbox + bot)             (Pi: AP + mailbox + bot)
        ┌─────────────────────────────────────────┐
        │                                         │
        │                BASE STATION             │
        │      (Pi 5 hosting its own Wi-Fi AP,    │
        │       the mayor captive portal and      │
        │       the mailbox; QRs on the wall)     │
        │                                         │
        └─────────────────────────────────────────┘
   RADIO LINK to RIVERSIDE               UPLINK to the JOURNALIST
   (Pi: AP + mailbox +                  (phone hotspot with internet; the
    Aunt Anna's bot)                     journalist herself is OUTSIDE the
                                         town — her bot runs on Digital
                                         Ocean via the existing cloud mailbox)
```

Corner assignment is arbitrary — the only requirement is that the stations
are far enough apart that carrying a message means actually walking.

### The cast

| Character | Persona | Infrastructure |
|---|---|---|
| **firefighters** | **Cindy the firefighter**, at the brigade HQ | Pi 5: Wi-Fi AP + mailbox + bot |
| **hospital** | **James the nurse**, at the town hospital | Pi 5: Wi-Fi AP + mailbox + bot |
| **journalist** | **Marta the journalist** — news desk **outside the town**, telling the world what's happening inside; the hotspot corner is the town's only surviving uplink to her | Phone hotspot (internet); bot on a Digital Ocean droplet syncing through the **existing cloud mailbox** |
| **relative** | **Aunt Anna**, a relative in Riverside, the nearby town, desperate for news of her family. Narratively her station is the town's surviving radio link to Riverside; in hardware her bot runs right on that station's Pi, like every other character | Pi 5: Wi-Fi AP + mailbox + bot |
| *(base station)* | **The town mayor** — captive portal only, not a chat character | Pi 5 running the `base-station` image: hosts its own Wi-Fi AP like the character stations and serves the mayor's captive portal + the mailbox. *(The mAP-lite-as-AP variant — a MikroTik mAP lite broadcasting the wifi with the Pi wired behind it, `nix/base-station.nix` — is kept but currently unused, in case the Pi's radio can't carry the 30-40 concurrent base-station clients)* |

Total hardware: **4 × Pi 5** (base, firefighters, hospital, relative) + **1
phone** (journalist hotspot) + **1 DO droplet** (already running the cloud
mailbox; gains the journalist bot).

### Game setup (at the base station)

The game begins at the base station — the mayor's office. Players join the
base station's Wi-Fi and the captive portal opens: **the town mayor** explains
the fires, pleads for help, and gives the first instructions —

1. Add the four characters as contacts by scanning their **QR posters on the
   wall** around the base station. Each one becomes a private chat.
2. When a character asks for a message to be passed on, **copy it and paste it
   into the recipient's chat**.
3. Walk to that recipient's station so the phone can deliver it.

The base Pi's mailbox is on that same Wi-Fi, so the contact requests are
seeded into the base mailbox immediately.

The contact requests only *reach* each bot when a player first syncs at that
bot's station — that's fine and thematic: each character "comes online" when
first visited, accepts the contact, greets the player in character, and starts
producing missions. There is no separate facilitator trigger (auto-start on
contact acceptance was the chosen design).

**Game rule that must be enforced socially:** players keep **mobile data off**
(Wi-Fi on) and **forget all other saved Wi-Fi networks**. A phone with LTE
would sync everything through the cloud mailbox from anywhere and
short-circuit the entire sneakernet, and a phone with saved networks in range
keeps auto-switching away from the station APs.

### Play loop

1. A bot, in each direct chat it has with a player, fires a mission message at
   a random interval (default: uniform 3–8 min, configurable), drawn from its
   character's template pool. Every mission names its destination character in
   the prose itself ("…copy this into the firefighters' chat!") — there is
   **no visible machine metadata**; recognition works on the text itself
   (see §3).
2. The player copies that message and pastes it into their chat with the
   named character.
3. Players walking into a station's AP bubble auto-sync (Dash Chat's existing
   mDNS local-mailbox discovery) — they pick up whatever blips that mailbox
   holds and deposit whatever they carry.
4. When the destination character's bot sees its own known mission text in a
   message a *player* wrote, it replies once with that template's in-character
   success message. Pasted at the wrong character, it gets that character's
   "this message is not for me!" line — enough to know it went astray, and
   nothing more.

Nothing gates a character on delivery: the walk is one-way, so a character
never learns whether its last message arrived. What bounds the flow is the
pack — **each template is handed to a given player at most once**, and once a
character has given out everything it has, it goes quiet (bar acks and Anna's
comeback line). Five templates per character × four characters ≈ 20 deliveries
for a solo courier.

Ending: the facilitator calls time; the chats themselves are the score sheet
(count success replies). No formal end state in software.

### The anonymous informant (side plot)

A hidden sixth character, **Anonymous**, has an identity but **no scenario
pack and no cast entry** — no other character knows he exists
(`characters.just` keeps him out of `larp-cast.toml`). His QR poster is hidden
somewhere on the map instead of hanging on the base-station wall.

A player who scans it sends a contact request that travels, like everything
else, through the mailboxes in players' pockets. **Every** station runs the
informant daemon (`larp-bot anonymous`) — every flash recipe copies the same
anonymous identity onto the card. When the request reaches any station, the
bot accepts and whispers into the direct chat: the mayor is lying — he lit
the fires, he shut down the internet, and he is using the emergency to
control the town. Then it tells the **whole secret** (`anonymous.toml`):

- the mayor's head on his portrait in the base-station captive portal is the
  secret trigger — five taps in a row reveal a hidden password prompt;
- the password — `ahawegotyou`.

Five taps on the portrait's head reveal the prompt; entering `ahawegotyou`
replaces the mayor's broadcast with the endgame page: his files are out and
**the mayor flees town**. A single player can finish the whole plot.

All stations run the **same** identity (one printed poster). p2panda logs
are per `(device, topic)`, and a failed op ingest is dropped per-op, so the
instances only ever collide on the announcements topic (all branches carry
the same "Anonymous" profile — first one in wins) and on a direct chat when
several stations accept the *same* player (that player keeps the first
station's chat — and every station tells the full story, so it doesn't
matter which one they keep). Degradation, not breakage.

---

## 2. What already exists (reused unmodified)

- **The `mailbox-image` flake input** (raspberry-pi-mailbox-server): NixOS SD
  image for Pi 5 with `hostapd` AP, dnsmasq, captive portal, and the
  `replicating-local-mailbox-server` from the dash-chat flake input. Per-card
  configuration via env files on the FAT boot partition (`wifi-ap.env`). This
  repo's station image is that image `extendModules`-ed with the bot. The
  image ships deliberately range-limited (minimum tx power, link-quality
  eviction, hostapd distance gates); **this repo overrides all of that back
  to full range** — see §4.
- **mDNS announce/discovery**: stations announce `_dashchat._tcp.local.`, so
  players' apps auto-discover the mailbox when they join a station's Wi-Fi.
- **Mailbox replication** (`replicating-local-mailbox-server`): bidirectional
  `/blips/get` sync of known topics between mDNS-discovered mailboxes. On
  this map stations are out of each other's range, so LAN replication is
  idle.
- **`dashchat-node`** (dash-chat repo): headless node with everything a bot
  needs — `new_qr_code()` / `add_contact()`, `accept_contact()` (which also
  creates the direct-chat space the whole game runs in), `direct_chat_topic()`,
  `send_message()`, `get_messages()`, and a `Notification` mpsc channel that
  streams every processed operation (header + payload) to the embedding
  application.
- **Cloud mailbox**: already running; the journalist bot and any
  hotspot-connected player sync through it.
- **mAP lite tooling (this repo, currently unused)**: `just
  base-station::map-lite::provision` turns a stock device into the
  base-station AP (ether1 bridged to the Pi, DHCP off; the Pi
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
- the **inbox nonce + expiry** — the printed QR carries an 8-byte nonce, and
  both sides derive the inbox topic as `blake3(device_pubkey ‖ nonce)`. Stream
  processing drops requests whose topic isn't in the local store's
  `active_inboxes` table, so a surviving key with a lost nonce still means a
  dead QR. (Before dash-chat 0.19 the bundle stored a *random* topic id and
  the QR carried it verbatim, along with the agent id and a share intent. Old
  bundles don't load; the whole cast was regenerated for 0.19 and the posters
  reprinted.)

On every boot the bot loads the bundle from `/boot/firmware/`, passes the
reconstructed `NodeKeys` to `Node::init` (which accepts them directly), and
idempotently re-registers the bundle's inbox topic as active
(`node.local_store` is public, so `add_active_inbox_topic` + topic
initialization need no upstream patch). `/var/lib/larp-bot` is thereby demoted
to a cache: after a wipe the bot forgets its contacts and answer-dedup state,
but players can simply re-scan the *same printed QR* to open a fresh chat —
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
  `InboxPayload::ContactRequest` and call `accept_contact(agent_id)`, which
  completes the handshake, replies with the profile *and* creates the direct
  chat. The requester's **device** id (the op author) is what gets persisted:
  the chat topic is derived from it.
- **Direct-chat discovery.** `Node::get_groups()` returns group chats only, so
  the bot derives its chats itself: every contact it accepted (its own state
  file) unioned with every contact in the node's projection (which survives a
  `state.json` wipe), mapped through `direct_chat_topic()`, keeping only
  topics the node is actually subscribed to (the node's own record that the
  space exists).
- **Greeting.** The first time a player's chat appears, send the character's
  in-character intro line, which also teaches the mechanic ("copy my message
  and paste it into that person's chat").
- **Scenario engine.** Per direct chat: a timer loop firing at
  `rand_range(min_interval, max_interval)`, drawing a template the player has
  not been given yet. No pool reshuffle and no ack gate — when the pack runs
  out for that player, the character stops handing out missions.
- **Delivery recognition — no visible metadata.** Messages are pure
  in-character prose; the machine layer rides on the text itself, since we
  author every pack:

  - All bots ship with **all four template packs** (they live in this repo).
  - *Recipient side:* a message a **player** wrote is a delivery for me iff it
    **contains** a template with `to = <my character>`. Matching normalizes
    whitespace and case and tolerates text around the paste (a quote header, a
    "look at this:" prefix) — a phone clipboard is not careful, and a missed
    delivery is a dead end for the player. There is no author check to hide
    behind any more, so a player who retypes a mission by hand gets the ack:
    fine, that is not a threat model, it's a LARP.
  - *Wrong recipient:* if the pasted template's `to` is somebody else, reply
    with the pack's `misdelivered` line — a plain "this message is not for
    me!". It gives nothing else away: working out where the message belongs
    is the players' job.
  - *Dedup:* the bot persists the **header hash** of every player message it
    has answered, so restarts and re-syncs don't double-answer. Hashes exist
    only in the protocol layer — nothing machine-ish ever appears on screen.
  - *Lint (unit-tested):* mission texts and success lines are unique across
    all packs, and **no mission text is contained in any other line a player
    might paste** — that is what keeps containment matching unambiguous.
  - The **cast file** (each character's public device/agent id) is no longer
    what recognition rests on. It is still shipped and loaded, used only to
    ignore anything authored by another character bot, so a stray cast message
    in a chat can never be mistaken for a player's delivery.
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
entries plus `greeting` and `misdelivered` (the "this message is not for me!"
line, sent verbatim when a player pastes in somebody else's message). Authored
in Spanish/Catalan/English as needed — pure content, no code. All four
character packs live in this repo under `scenarios/`, and every bot loads all
of them (recognition depends on it — see above). A unit test lints the packs:
`text` and `success` unique across all packs and never nested inside another
pasteable line, `to` values valid.

A pack may also carry an optional `[comeback]` (`after_secs` + `text`): after
that long without any *player* message in a chat, the character answers the
next player message with `text`, once per quiet spell. Only Aunt Anna uses it
("Hey! How is everything over there?") — a sign of life from Riverside when
players resurface. Tracking is in-memory and baselined on the first scan, so
bot restarts never trigger it.

## 4. NixOS & deployment changes

### One image, per-card station selection

Keep the single-SD-image philosophy. **Implemented:** the bot service
(`nix/larp-bot.nix`, baked into every image with `services.larp-bot`) is gated
at runtime with `ConditionPathExists` on files the FAT boot partition may
carry, next to `wifi-ap.env`:

- `larp-identity.toml` — the character's flashed identity bundle
- `larp-cast.toml` — the public cast file (flashed too, **not** baked into the
  image: it changes per game, the image doesn't)
- `larp-anonymous.toml` — the anonymous informant's identity (gates the
  informant service the same way; every flash recipe copies it)

No file → no bot: the card is a plain mailbox appliance. The station variants
are just combinations of flashed files:

| Station | mailbox | AP (hostapd) | larp-bot | informant |
|---|---|---|---|---|
| base | ✓ | ✓ (`base-station` image) | – | ✓ |
| firefighters / hospital / relative | ✓ | ✓ | ✓ (identity flashed) | ✓ |

The base station Pi runs the `base-station` image (`just base-station::build`):
the station image with the captive portal re-enabled and the mayor page in
place of the generic captive-portal SPA — it is the only station with a
portal at all. It hosts its own wifi like every other station —
`just base-station::flash` writes the `wifi-ap.env` (SSID
`internet-shutdown-larp` by default).

### Full-range Wi-Fi (no distance reduction)

The plain mailbox image deliberately shrinks each station's radio footprint
(1 dBm tx power, a link-quality eviction daemon, hostapd's RSSI join gate /
client power constraint / rate floor / low-ack kick). The game doesn't want
tiny bubbles any more, so the station image overrides all of it in
`flake.nix`:

- `dashchat.wifi.apTxPowerDbm = 20` (the ES regulatory max on 2.4 GHz);
- the `dashchat-ap-guard` eviction service is disabled;
- an `ExecStartPre` strips the remaining distance limiters from the generated
  `hostapd.conf` before hostapd starts;
- an extra `ExecStartPost` keeps `power_save off` applied regardless of the
  txpower clamp outcome (power save on a brcmfmac AP kills beaconing minutes
  in — the validated stability fix).

### Base station: mayor portal

- **Mayor page** *(implemented)*: `portal/index.html` in this repo — a single
  static page (a couple of lines from the mayor + portrait + the minimal
  step-by-step instructions in big type + a mailbox health check via the
  module's `/api/` proxy), no build step. The `base-station` config overrides
  `dashchat.captivePortal.package` with it. The portrait hides the informant
  side plot's endgame: tapping the mayor's head five times in a row reveals a
  hidden password prompt, and entering `ahawegotyou` swaps the broadcast for
  the mayor-flees-town page (per device, remembered in `localStorage`).
- **Nothing is gated**: the portal is onboarding UX, and every client
  (phones, headless Pis) reaches the mailbox without logging in to anything.

*(Currently unused alternative)* If the Pi's brcmfmac AP can't carry the
30-40 concurrent base-station clients, the mAP-lite variant is kept:
`nix/base-station.nix` (re-add it to the `base-station` modules in
`flake.nix`) makes the Pi host no wifi and instead own DHCP + wildcard DNS
on the cable to a MikroTik mAP lite, provisioned as a plain AP with
`just base-station::map-lite::provision` (ether1 bridged to the Pi, DHCP
off). mDNS passes the mAP's L2 bridge, and no RouterOS hotspot is involved.

Also per-station: the AP SSID defaults to the station name plus a game
suffix (`SSID=firefighters-larp` etc. via `wifi-ap.env`),
so the facilitator can see at a glance which bubble they're in. The one
exception is the relative's station, which advertises `riverside-radio-larp`
— in the fiction it's the town's radio link to Riverside, not Aunt Anna's
house. Character
stations run **no captive portal** (`dashchat.captivePortal.enable = false`
in the station image): joining one looks like a dead network, and the app
still finds the mailbox via mDNS + its own port. Only the base station pops
a portal (the mayor).

`larp-bot` builds with `rustPlatform.buildRustPackage` from this repo's
workspace (git deps via `cargoLock.allowBuiltinFetchGit`, so no outputHashes
to maintain), exposed as flake packages for x86_64 (dev/DO) and aarch64 (Pi).
Scenario packs (`scenarios/`) are pure repo content baked into the image at
`services.larp-bot.scenariosDir`.

Provisioning flow (all offline, on the laptop — implemented as `just` recipes):

1. `just characters::generate` — one identity bundle per scenario pack into
   `secrets/` (gitignored; existing bundles are kept, since re-generating
   would invalidate the printed posters), plus the anonymous identity and the
   public `secrets/larp-cast.toml` assembled from all of them (minus
   anonymous). Idempotent, and the cast is complete by construction: the
   character list *is* `scenarios/*.toml`.
2. `just characters::posters` — renders the QR wall-poster PNGs for printing.
4. `just characters::flash <character> /dev/sdX` — flashes the station image and
   puts the character's files (`wifi-ap.env` with `SSID=<character><ssid_suffix>`,
   **open network** unless a password argument is given,
   `larp-identity.toml`, `larp-cast.toml`, `larp-anonymous.toml` — assembled
   on the fly from `secrets/`) on the card's boot partition.

The base station's portal can additionally serve the mayor's QR as a
fallback onboarding path (the character stations run no portal).

**Seed the base mailbox with the cast's profiles** (once, after the bots have
booted): each character's profile lives on its bot's announcements topic,
seeded only at its own station (Marta: only in the cloud) — and replication
never introduces a mailbox to topics it doesn't know. Without seeding,
contacts added from the wall posters appear *nameless* at the base station,
right when players are learning which chat belongs to which character.
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
to a different cloud mailbox than the players' apps never sees their chats.

## 5. End-to-end message walk-through (sanity check)

Hospital bot fires: *"Injured people trapped on Elm St — copy this into the
firefighters' chat!"* into its direct chat with the player, via the hospital
Pi's localhost mailbox — plain prose, no visible metadata.

1. A player visits the hospital bubble → phone syncs the blip and shows the
   message in James's chat.
2. The player long-presses it, copies it, and pastes it into the
   *firefighters'* chat. That paste is authored by the player, and sits in
   their pocket until they reach a station.
3. The player walks to the firefighters bubble → the paste is deposited into
   the firefighters mailbox.
4. The firefighters bot's node polls its localhost mailbox and sees a message
   from the player containing a template with `to = "firefighters"` — it
   replies *"Okey! Crews dispatched to Elm St, thanks!"* (that template's
   success line) into the same chat, where the player reads it on the spot.

Had the player pasted it into Aunt Anna's chat instead, Anna would have
answered *"Oh dear, this message is not for me!"* — enough to send the player
looking again, without telling them where. The hospital never finds out
either way.

For Aunt Anna, the destination is her radio-link station — same mechanics.
For the journalist, the deposit step is "join the hotspot": it goes to the
cloud mailbox, and the DO bot answers usually within seconds.

## 6. Risks & open questions

- **Nameless contacts at the base station** — profiles ride each bot's
  announcements topic, which the base mailbox doesn't know until seeded (see
  the seeding step in §4). Re-seed if a profile ever changes.
- **QR encoding fidelity** — the printed QR must decode in the real app.
  Verify with a phone in week 1; this gates the whole onboarding flow.
- **QR/inbox expiry semantics** — besides the bundle's inbox expiry, check
  that nothing else garbage-collects the bot's inbox topic before game day.
- **Post-wipe state loss** — a wipe preserves identity (flashed bundle) but
  loses contacts and answer-dedup, so a mid-game re-flash means players
  re-scan the character's poster (same printed QR) to get a chat back, and
  already-answered deliveries may be answered twice. Acceptable; don't wipe
  mid-game.
- **dashchat-node offline behaviour** — the node embeds iroh/p2panda
  networking that may want internet (relays, DNS). Must verify a node on an
  offline LAN talking only to a localhost mailbox is healthy. (The mailbox
  side is already proven offline; the *node* side is not.)
- **The copy-paste step is the whole mechanic** — if players don't discover
  that they must long-press a message and paste it into another chat, nothing
  happens at all. It is taught in three places (the mayor's portal, every
  character's greeting, every mission text) and the misdelivery line catches
  the near-miss; still, watch the first players and be ready to say it out
  loud.
- **Messages pile up while nobody is around** — a character keeps firing on
  its timer whether or not a player is in its bubble, so a long absence means
  several missions waiting at once. Bounded by the pack (each template is
  handed out once per player), and a queue of work is playable, but it is why
  the pool is five templates deep, not fifty.
- **Station Wi-Fi ranges overlapping** — with the range limiting removed the
  bubbles are full-size; on a small play area two stations may cover the same
  spot and phones will pick one arbitrarily. Space the stations, or shrink
  the map's density.
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
  before game day (milestone 2). *(Only relevant if the unused mAP-lite
  variant is ever revived.)*

## 7. Implementation milestones

1. **`larp-bot` core** — workspace scaffolding, config, `keygen`/`qr`
   (offline identity bundles), bundle loading + inbox re-registration,
   auto-accept, direct-chat discovery, greeting, scenario engine, delivery
   recognition (pasted-text matching). E2E test on a laptop:
   `dashchat-node` test instances + one in-memory mailbox + two bots; assert a
   mission → copy-paste(simulated, sloppy) → ack round-trip and a misdelivery
   nudge, then wipe a bot's data dir, restart it, and assert the same
   identity/QR still onboards.
2. **Nix integration + base station** — `nix/larp.nix`, per-station env dirs
   with flashed identity bundles, packages, image build for a bot station;
   the base-station image; live tests: phone joins a bot station's AP, scans
   the printed QR poster, gets greeted in a direct chat, receives a mission —
   and at the base, portal opens, phone syncs with the base mailbox.
3. **Journalist droplet** — NixOS config on DO against the cloud mailbox;
   test through a real phone hotspot.
4. **Scenario content + dress rehearsal** — write the four template packs,
   full field test (4 Pis), print the QR wall posters and finalize
   the mayor's portal content, tune intervals/caps.
