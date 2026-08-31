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

The cast is **the player's own family and the woman next door**: everyone the
player would actually be frantic about in a fire. Four of them live at the
stations, and each talks to the player in a **private one-to-one chat** —
there is no group chat in the game. Every character keeps producing urgent
messages with a clear recipient ("The hens are still shut in and my knee
won't get me down that path. Copy this into Nadia's chat."). The
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
   MAMA (home)                            GRANDPA AMIR (top of the hill)
   (Pi: AP + mailbox + bot)             (Pi: AP + mailbox + bot)
        ┌─────────────────────────────────────────┐
        │                                         │
        │                BASE STATION             │
        │      (Pi 5 hosting its own Wi-Fi AP,    │
        │       the mayor's bot and the mailbox;  │
        │       all five QR posters on the wall)  │
        │                                         │
        └─────────────────────────────────────────┘
   NADIA (next door)                    SIGNAL to the CITY
   (Pi: AP + mailbox + bot)             (phone hotspot with internet; the
                                         sister is OUTSIDE the town — her
                                         bot runs on Digital Ocean via the
                                         existing cloud mailbox)
```

Corner assignment is arbitrary — the only requirement is that the stations
are far enough apart that carrying a message means actually walking.

### The cast

| Character | Persona | Infrastructure |
|---|---|---|
| **mum** | **Mama**, at the family house, packing for the shelter and holding everyone together | Pi 5: Wi-Fi AP + mailbox + bot |
| **grandpa** | **Grandpa Amir**, alone at the top of the hill, refusing to be evacuated | Pi 5: Wi-Fi AP + mailbox + bot |
| **neighbour** | **Nadia the neighbour**, next door, who stayed behind with everyone's pets and keys | Pi 5: Wi-Fi AP + mailbox + bot |
| **sister** | **Mira, the player's sister** — studying in the city, **outside the town** and the only one who still has internet: she sees the news, looks things up, and can do nothing with her own hands. The hotspot corner is the family's only line to her | Phone hotspot (internet); bot on a Digital Ocean droplet syncing through the **existing cloud mailbox** |
| **mayor** *(base station)* | **The Mayor** — a chat contact like everyone else, but with no scenario pack: his greeting *is* the game's onboarding, and one trigger phrase is the endgame (§The mayor and the informant) | Pi 5 running the same station image, flashed with his identity and no character identity: Wi-Fi AP + mailbox + his bot. *(The mAP-lite-as-AP variant — a MikroTik mAP lite broadcasting the wifi with the Pi wired behind it, `nix/base-station.nix` — is kept but currently unused, in case the Pi's radio can't carry the 30-40 concurrent base-station clients)* |

Total hardware: **4 × Pi 5** (base, mum, grandpa, neighbour) + **1
phone** (the hotspot that reaches the sister) + **1 DO droplet** (already
running the cloud mailbox; gains the sister's bot).

### Game setup (at the base station)

The game begins at the base station — the town hall. There is **no captive
portal** anywhere on the map: the mayor is a chat contact, so onboarding
happens in Dash Chat like everything else.

Players join the base station's Wi-Fi and scan **the mayor's QR poster
first** — a printed sign at the base station says so, and it is the only
instruction that has to exist on paper. He accepts immediately (his bot is on
that same Pi, on that same Wi-Fi) and his greeting is the briefing: the fires,
the dead network, and then the three rules —

1. Scan the other four **QR posters on the wall**. Each becomes a private chat
   with one of the player's family or their neighbour.
2. When one of them asks for a message to be passed on, **copy it and paste it
   into the recipient's chat**.
3. Walk to that recipient's station so the phone can deliver it.

(Plus: mobile data off, forget other networks.)

The base Pi's mailbox is on that same Wi-Fi, so the four contact requests are
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
   the prose itself ("…copy this into Grandpa Amir's chat.") — there is
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
character has given out everything it has, it goes quiet (bar acks and Mira's
comeback line). Five templates per character × four characters ≈ 20 deliveries
for a solo courier.

Ending: the facilitator calls time; the chats themselves are the score sheet
(count success replies). No formal end state in software.

### The mayor and the informant (side plot)

Two characters have an identity but **no scenario pack and no cast entry**:
the **mayor** and **Anonymous**. They trade no missions, and
`characters.just` keeps both out of `larp-cast.toml` — the family doesn't
know the informant exists. Both are driven by the same **spec bot**
(`larp-bot spec`, `crates/larp-bot/src/spec.rs`): a small script of
`name` + `greeting` (sent in order when a contact request is accepted) +
optional `triggers` (a phrase to listen for in player messages, and what to
answer). The whole side plot is those two files, `mayor.toml` and
`anonymous.toml`.

**The mayor** runs on the base-station Pi only. His greeting is the
onboarding (above). He listens for one phrase.

**Anonymous**'s QR poster is hidden somewhere on the map instead of hanging
on the base-station wall. A player who scans it sends a contact request that
travels, like everything else, through the mailboxes in players' pockets.
**Every** station runs the informant — every flash recipe copies the same
anonymous identity onto the card. When the request reaches any station, the
bot accepts and whispers into the direct chat: the mayor is lying — he lit
the fires, he shut down the internet, and he is using the emergency to
control the town. He keeps it all in files behind one password, and the
informant hands that password over: **`ahawegotyou`**.

Then the payoff, and the reason this plot lives in the chat app: the
informant tells the player to do the one thing this game has been teaching
them all along — **copy the password and paste it into the mayor's own
chat**. The mayor's bot matches it the same forgiving way a delivery is
matched (whitespace, case and surrounding prose forgiven), and answers with
the collapse: the files are open, the orders to light the fires and cut the
network are in them, and **the mayor flees town** — ending with the
congratulations line. Answered once per message (the op hash is persisted),
so a re-sync never replays it.

Because the mayor's bot sits on the base station, the last delivery of the
game is a walk back to where it started. A single player can finish the whole
plot. A unit test asserts that what the informant hands out is exactly what
the mayor listens for — a drifted password would make the endgame
unreachable.

The informant's stations all run the **same** identity (one printed poster).
p2panda logs are per `(device, topic)`, and a failed op ingest is dropped
per-op, so the instances only ever collide on the announcements topic (all
branches carry the same "Anonymous" profile — first one in wins) and on a
direct chat when several stations accept the *same* player (that player keeps
the first station's chat — and every station tells the full story, so it
doesn't matter which one they keep). Degradation, not breakage. The mayor has
no such caveat: one identity, one card.

---

## 2. What already exists (reused unmodified)

- **The `mailbox-image` flake input** (raspberry-pi-mailbox-server): NixOS SD
  image for Pi 5 with `hostapd` AP, dnsmasq and the
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
- **Cloud mailbox**: already running; the sister's bot and any
  hotspot-connected player sync through it.
- **mAP lite tooling (this repo, currently unused)**: `just
  base-station::map-lite::provision` turns a stock device into the
  base-station AP (ether1 bridged to the Pi, DHCP off — see
  `nix/base-station.nix`). Kept in case the Pi's own AP can't carry the
  base-station load; for now the base station hosts its own Pi wifi like
  every other station.

## 3. New component: `larp-bot` crate

A new Rust crate **in this repo** (new `crates/` workspace), depending on
`dashchat-node` as a **git dependency pinned to the same rev as the flake's
`dash-chat` input** (the message/payload format must match the app version
players run — version skew here is the #1 way to break the game silently).

One binary, one character per process:

```
larp-bot keygen --out larp-identity.toml           # provision an identity bundle (run on the laptop)
larp-bot qr     --identity larp-identity.toml --out qr.png   # derive the printed QR (offline, no Pi needed)
larp-bot cast   --identity … --out cast.toml       # assemble the public cast file
larp-bot run    --config /etc/larp-bot/config.toml # a character daemon (loads the flashed bundle)
larp-bot spec   --config /etc/larp-bot/spec.toml   # a spec-bot daemon: the mayor, or the informant
```

`run` plays a character out of `scenarios/`; `spec` plays one of the two
pack-less characters out of its own script file (`mayor.toml`,
`anonymous.toml` — see §The mayor and the informant). `anonymous` is kept as
an alias of `spec` for the old invocation.

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
character   = "mum"                   # persona selection
mailbox_url = "http://127.0.0.1:8080"
identity    = "/boot/firmware/larp-identity.toml"  # flashed bundle (see above)
cast        = "/etc/larp-bot/cast.toml"            # all characters' public agent ids
data_dir    = "/var/lib/larp-bot"                  # cache only — safe to wipe

[timing]
min_interval_secs = 180
max_interval_secs = 480

[templates]                            # per-character scenario file
path = "/etc/larp-bot/mum.toml"
```

Template file: a list of `{ to = "grandpa", text = "…", success = "…" }`
entries plus `greeting` and `misdelivered` (the "this message is not for me!"
line, sent verbatim when a player pastes in somebody else's message). Authored
in Spanish/Catalan/English as needed — pure content, no code. All four
character packs live in this repo under `scenarios/`, and every bot loads all
of them (recognition depends on it — see above). A unit test lints the packs:
`text` and `success` unique across all packs and never nested inside another
pasteable line, `to` values valid.

A pack may also carry an optional `[comeback]` (`after_secs` + `text`): after
that long without any *player* message in a chat, the character answers the
next player message with `text`, once per quiet spell. Only Mira uses it
("Hey!! Anything? I'm losing my mind over here.") — a sign of life from the
city when players resurface. Tracking is in-memory and baselined on the first scan, so
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
- `larp-mayor.toml` — the mayor's identity (gates his service; **only**
  `base-station::flash` writes it, so he exists on exactly one card)

No file → no bot: the card is a plain mailbox appliance. There is now
**literally one image** — the base station stopped being a variant when the
captive portal went away — and the station types are nothing but combinations
of flashed files:

| Station | mailbox | AP (hostapd) | character bot | mayor | informant |
|---|---|---|---|---|---|
| base | ✓ | ✓ | – | ✓ | ✓ |
| mum / grandpa / neighbour | ✓ | ✓ | ✓ (identity flashed) | – | ✓ |

`just base-station::flash` writes the base card's `wifi-ap.env` (SSID
`internet-shutdown-larp` by default) plus the mayor's and the informant's
identities — and deliberately no character identity and no cast file, so the
character-bot service stays dormant there.

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

### No captive portal anywhere

The base station used to serve a captive portal — the mayor's broadcast page,
with the onboarding steps and a hidden five-taps-on-the-portrait endgame.
**That is gone** (`portal/index.html` and `nix/captive-portal.nix` deleted,
`dashchat.captivePortal` no longer defined by this repo): the mayor is a chat
contact now, so his briefing and his downfall both happen in Dash Chat, where
every other word of the game already happens.

What this buys: one medium instead of two, one image instead of two, and a
finale that uses the exact copy-paste-and-walk verb the whole game teaches,
rather than an unrelated tap-the-picture easter egg.

What it costs, and what to watch on game day:

- **Phones drop Wi-Fi networks with no internet.** The portal handshake was
  the mitigation at the base station; now no station has one. This risk is
  live everywhere — test with the actual target phones (§6).
- **Nothing on the network greets a player who has not installed the app.**
  A printed sign at the base station has to carry that first step: *install
  Dash Chat, scan the mayor's poster.* It is the only instruction that does
  not live in a chat.

*(Currently unused alternative)* If the Pi's brcmfmac AP can't carry the
30-40 concurrent base-station clients, the mAP-lite variant is kept:
`nix/base-station.nix` (re-add it to the `base-station` modules in
`flake.nix`) makes the Pi host no wifi and instead own DHCP + wildcard DNS
on the cable to a MikroTik mAP lite, provisioned as a plain AP with
`just base-station::map-lite::provision` (ether1 bridged to the Pi, DHCP
off). mDNS passes the mAP's L2 bridge, and no RouterOS hotspot is involved.

Also per-station: the AP SSID is the character name plus a game suffix
(`SSID=mum-larp` etc. via `wifi-ap.env`), so the facilitator can see at a
glance which bubble they're in — there is no exception any more, every
in-town character's station carries their own name. Joining any of them
(the base station included) looks like a dead network: nothing pops, and the
app finds the mailbox via mDNS on its own port.

`larp-bot` builds with `rustPlatform.buildRustPackage` from this repo's
workspace (git deps via `cargoLock.allowBuiltinFetchGit`, so no outputHashes
to maintain), exposed as flake packages for x86_64 (dev/DO) and aarch64 (Pi).
Scenario packs (`scenarios/`) are pure repo content baked into the image at
`services.larp-bot.scenariosDir`.

Provisioning flow (all offline, on the laptop — implemented as `just` recipes):

1. `just characters::generate` — one identity bundle per scenario pack into
   `secrets/` (gitignored; existing bundles are kept, since re-generating
   would invalidate the printed posters), plus the mayor's and the anonymous
   informant's, plus the public `secrets/larp-cast.toml`. Idempotent, and the
   cast is built from `scenarios/*.toml` alone — so the two spec-bot
   characters are excluded by construction, and an identity left over from a
   retired character is ignored rather than resurrected.
2. `just characters::posters` — renders the QR wall-poster PNGs for printing.
   **Six** of them: the four family posters and the mayor's go on the
   base-station wall (his is the one players scan *first*), and the
   informant's is hidden somewhere on the map.
3. `just characters::flash <character> /dev/sdX` — flashes the station image and
   puts the character's files (`wifi-ap.env` with `SSID=<character><ssid_suffix>`,
   **open network** unless a password argument is given,
   `larp-identity.toml`, `larp-cast.toml`, `larp-anonymous.toml` — assembled
   on the fly from `secrets/`) on the card's boot partition.
4. `just base-station::flash /dev/sdX` — the same image, but with
   `larp-mayor.toml` + `larp-anonymous.toml` and no character identity.

**Seed the base mailbox with the cast's profiles** (once, after the bots have
booted): each character's profile lives on its bot's announcements topic,
seeded only at its own station (Mira: only in the cloud) — and replication
never introduces a mailbox to topics it doesn't know. Without seeding,
contacts added from the wall posters appear *nameless* at the base station,
right when players are learning which chat belongs to which character.
The fix uses the client push path: on a phone with internet, add all four
characters (Mira's profile arrives via the cloud; the others via their
stations — or plug all the Pis into one ethernet switch, where the mailboxes
discover and push to each other), then stand on the base station's Wi-Fi for
a minute. The phone pushes all four announcements topics into the base
mailbox, permanently. Do NOT seed by running a character bot against the
base mailbox with a fresh data dir — same identity, second op log, forked
history.

### The sister: cloud host (or laptop)

Mira is the one character outside the town, and she is just the same
`larp-bot` service pointed at the cloud mailbox — no new mailbox is deployed
(chosen design), and the phone hotspot needs zero config: any internet gets
players to the cloud mailbox, which the app already knows about.

*Implemented:* `just sister::deploy` provisions the whole thing with
doctl — first run creates an Ubuntu droplet whose cloud-init converts it to
NixOS in place (nixos-infect), then pushes the flake's `sister-droplet`
config plus the secrets (same `keygen` artifacts, delivered over SSH to
`/var/lib/larp-secrets/` instead of a FAT partition); later runs skip
straight to the push. `sister::logs` follows the bot's journal,
`sister::destroy` tears the droplet down (the identity survives in
`secrets/`). The flake also still exports `nixosModules.larp-bot` with a
usage example (see flake.nix) for wiring the bot into an existing NixOS
host instead — e.g. the droplet already running the cloud mailbox.

For testing without touching the droplet, `just characters::run sister
[mailbox_url]` runs the bot on the laptop against the cloud mailbox — the
laptop has internet, which is all she needs. State lives in
`.run/sister/` (wipe it to simulate a reset; identity survives, it's in
`secrets/`).

**Pick the mailbox URL to match the players' app build**: release builds use
the production mailbox, dev builds may point at staging. A sister bot synced
to a different cloud mailbox than the players' apps never sees their chats.

## 5. End-to-end message walk-through (sanity check)

Grandpa's bot fires: *"The hens are still shut in and my knee won't get me
down that path. Somebody has to open the coop. Copy this into Nadia's
chat."* into its direct chat with the player, via the grandpa Pi's localhost
mailbox — plain prose, no visible metadata.

1. A player visits the grandpa bubble → phone syncs the blip and shows the
   message in Grandpa Amir's chat.
2. The player long-presses it, copies it, and pastes it into *Nadia's*
   chat. That paste is authored by the player, and sits in their pocket
   until they reach a station.
3. The player walks to the neighbour bubble → the paste is deposited into
   the neighbour mailbox.
4. The neighbour bot's node polls its localhost mailbox and sees a message
   from the player containing a template with `to = "neighbour"` — it
   replies *"Coop's open, water changed, all six accounted for."* (that
   template's success line) into the same chat, where the player reads it on
   the spot.

Had the player pasted it into Mama's chat instead, she would have answered
*"Love, this one isn't for me!"* — enough to send the player looking again,
without telling them where. Grandpa never finds out either way.

For the sister, the deposit step is "join the hotspot": it goes to the
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
  happens at all. It is taught in three places (the mayor's greeting, every
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
- **Player phones auto-leaving the AP** *(raised by dropping the portal)* —
  phones drop Wi-Fi networks with no internet, and the base station's captive
  portal used to be the one mitigation on the map. No station has a portal
  now, so the risk is live everywhere, onboarding included. Test with the
  actual target phones; if it bites, the fallback is re-adding a
  content-free captive portal purely for the handshake (the deleted
  `nix/captive-portal.nix` is in git history).
- **Nothing greets a player who hasn't installed the app** *(same cause)* —
  with no portal, the network says nothing to a fresh phone. A printed sign
  at the base station must carry "install Dash Chat, scan the mayor's poster
  first".
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
2. **Nix integration + base station** — `nix/larp-bot.nix`, per-station env
   dirs with flashed identity bundles, packages, one image for every station;
   live tests: phone joins a bot station's AP, scans the printed QR poster,
   gets greeted in a direct chat, receives a mission — and at the base, the
   mayor's poster onboards the player and the phone syncs with the base
   mailbox.
3. **The sister's droplet** — NixOS config on DO against the cloud mailbox;
   test through a real phone hotspot.
4. **Scenario content + dress rehearsal** — write the four template packs and
   the two spec-bot scripts, full field test (4 Pis), print the six QR wall
   posters and the base station's paper sign, tune intervals/caps.
