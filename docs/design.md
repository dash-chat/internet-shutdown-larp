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
player would actually be frantic about in a fire. Five of them live at the
stations (Nadia at the base station itself), and each talks to the player in
a **private one-to-one chat** —
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
   (AP + Pi: mailbox + bot)             (AP + Pi: mailbox + bot)
        ┌─────────────────────────────────────────┐
        │                                         │
        │                BASE STATION             │
        │      (an AP broadcasting OfflineWifi    │
        │       + a Pi 5 with the mailbox and     │
        │       TWO bots: the mayor, and NADIA    │
        │       the tinkerer — her greeting is    │
        │       the tutorial and her first        │
        │       mission opens the game; all six   │
        │       QR posters on the wall)           │
        │                                         │
        └─────────────────────────────────────────┘
   RAFA (the fire line, east)           MIRA (the school shelter)
   (AP + Pi: mailbox + bot)             (AP + Pi: mailbox + bot; the desk
                                         with the list of who has arrived —
                                         and the only door to the informant)
```

Corner assignment is arbitrary — the only requirement is that the stations
are far enough apart that carrying a message means actually walking.

### The cast

| Character | Persona | Infrastructure |
|---|---|---|
| **mum** | **Mama**, at the family house, packing for the shelter and holding everyone together | Own AP (`OfflineWifi`) + Pi 5: mailbox + bot |
| **grandpa** | **Grandpa Amir**, alone at the top of the hill, refusing to be evacuated | Own AP (`OfflineWifi`) + Pi 5: mailbox + bot |
| **neighbour** *(base station)* | **Nadia the neighbour**, the town's tinkerer: the offline boxes that keep these chats alive are hers, up on the lampposts for years before the blackout. Her greeting is the game's real tutorial — the courier job, the phone rules — and her `first` mission is the opening delivery. She minds the street's pets and keys too | Rides the base-station Pi (no card of her own — `characters::flash neighbour` refuses) |
| **cousin** | **Rafa, the player's cousin**, digging firebreaks with the volunteers at the east edge — he can't leave the line, so everything he needs travels by courier | Own AP (`OfflineWifi`) + Pi 5: mailbox + bot |
| **sister** | **Mira, the player's sister**, on the desk at the school shelter: she holds the list of who has arrived, so everyone's names pass through her. She is also **the only door to the side plot** — the informant wrote to her desk, and she is the one character who hands his contact on (§The mayor and the informant) | Own AP (`OfflineWifi`) + Pi 5: mailbox + bot |
| **mayor** *(base station)* | **The Mayor** — a chat contact like everyone else, but with no scenario pack: his greeting is the emergency notice players read first (situation only — he never encourages the comms he secretly cut), and one trigger phrase is the endgame (§The mayor and the informant) | Shares the base-station Pi with Nadia's bot: same station image, flashed with his identity plus her character files. *(The base AP carries the 30-40 concurrent clients, so it wants to be the best one available — a MikroTik mAP lite, say, not a spare phone)* |

Total hardware: **5 × Pi 5** (base [mayor + Nadia], mum, grandpa, cousin,
sister) + **an AP per station** (§Wi-Fi). Nothing runs off-map any more: no
hotspot corner, no droplet, no cloud mailbox in the game loop.

### Game setup (at the base station)

The game begins at the base station — the town hall. There is **no captive
portal** anywhere on the map: the mayor is a chat contact, so onboarding
happens in Dash Chat like everything else.

Players join the base station's Wi-Fi and scan **all six QR posters,
the mayor's first** — a printed sign at the base station says so, and it is
the only instruction that has to exist on paper. Onboarding is then a
two-voice scene, both bots living on that same Pi:

1. **The mayor** answers with the emergency notice: fires everywhere,
   phones and internet down, be careful. Situation only — he never mentions
   the chats still working, and he never teaches the courier job.
   Encouraging communications is the last thing he wants (§The mayor and
   the informant); replayed after the endgame, his blandness reads very
   differently.
2. **Nadia** answers with the real tutorial: the boxes on the lampposts are
   hers — up for years, from long before the blackout — which is why the
   chats still work; the boxes can't reach each other, so the player is the
   wire (**copy, paste into the right chat, walk to that station**); phone
   rules (mobile data off, forget other wifi); and "never mind what the
   mayor told you".
3. Seconds later her bot fires the **opening delivery** (`first = true` on
   the mission — deterministic, not a lucky draw): tell your mum the chats
   still work. The first thing the player carries is the news that carrying
   works, and it kills the cold start — no standing around waiting for a
   mission timer somewhere else.

The base Pi's mailbox is on that same Wi-Fi, so all the contact requests are
seeded into the base mailbox immediately — and the mayor's and Nadia's bots
answer on the spot, since they live there.

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
5. The landed delivery immediately earns the courier their next job: the
   receiving character fires a fresh mission of its own into the chat on the
   spot, drawn **preferring a destination other than the delivered mission's
   originator** (falling back to the full pool when that's all that's left),
   so the walk keeps moving forward instead of ping-ponging between two
   stations. The chat's random timer resets, so the two paths never pile
   missions on top of each other. One exception: when the delivery earns the
   informant tip (Mira, once per player), the tip **is** the follow-up — no
   regular mission goes out with it, so the side plot isn't buried the
   moment it opens.

The random timer never gates on delivery: the walk is one-way, so a character
never learns whether its *own* last message arrived (the follow-up above is
fired by the *receiving* character, the one station that does witness a
success). What bounds the flow is the
pack — **each template is handed to a given player at most once**, and once a
character has given out everything it has, it goes quiet (bar acks, Mira's
comeback line and her informant tip). Five or six templates per character
(28 across the five packs, Nadia's opener included) — ≈ 28 deliveries for a
solo courier.

Ending: the facilitator calls time; the chats themselves are the score sheet
(count success replies). No formal end state in software.

### The mayor and the informant (side plot)

Two characters have an identity but **no scenario pack and no cast entry**:
the **mayor** and **Anonymous**. They trade no missions — nothing is ever
addressed to either of them (a spec bot never acks a delivery) — and
`characters.just` keeps both out of `larp-cast.toml`: the family doesn't
know the informant exists. Both are driven by the same **spec bot**
(`larp-bot spec`, `crates/larp-bot/src/spec.rs`): a small script of
`name` + `greeting` (sent in order when a contact request is accepted) +
optional `triggers` (a phrase to listen for in player messages, and what to
answer). The whole side plot is those two files, `mayor.toml` and
`anonymous.toml`.

**The mayor** runs on the base-station Pi only, next to Nadia's bot. His
greeting is the emergency notice (above) — deliberately empty of anything
that would help messages move: he cut the networks himself, so he never
mentions the chats still working and never teaches the courier job (a unit
test pins that: no "Nadia", no "copy" in his greeting). He listens for one
phrase.

**Anonymous has no QR poster at all.** The only way to meet him is **Mira**:
he wrote to the shelter desk, because that is the desk every name in town
passes through. Once a player has actually carried a message *to* her, her
bot follows the success line with her `informant_tip` — her own words plus
his **add-contact deep link**, `https://dashchat.org/add-contact/<code>`.
Tapping it opens Dash Chat and sends the contact request; a phone that fails
to route the tap can paste the same line into Add contact, which accepts the
link and the bare code alike. This is deterministic, not a chance: one door,
and it is behind a real delivery. Sent at most once per player (recorded in
the bot's `state.json`).

Every character bot could carry a tip line — the mechanism is a per-pack
`informant_tip` field with a `{link}` placeholder — but as shipped exactly
one does, and a unit test enforces that. More doors would make the side plot
a lottery; none would make it unreachable.

The side plot is gated on a delivery reaching Mira, so three missions are
addressed to her (one each from Mama, Grandpa and Nadia) and a courier doing
the rounds hits one early. Strip them all and the informant would be
unreachable — the pack linter fails for exactly that: a character with an
`informant_tip` that nobody is ever sent to.

The informant runs on **Mira's card alone** (`characters.just`
`informant_character`), where his identity does double duty: it arms his
service, and her character bot reads its public half to build the link in
her tip. One identity, one card — like the mayor. The consequence: his
contact link is only answered **inside Mira's station wifi**, which is why
her tip says to tap it right there before walking off (a link tapped
elsewhere just sits in the player's pocket until they're back). When the
request lands, the bot accepts and whispers into the direct chat: the mayor
is lying — he lit the fires, he cut the phones, and he is using the
emergency to control the town. And then the evidence: one line the informant
copied **word for word** out of the mayor's own written order —

> Let the north side burn until they stop asking questions.

Then the payoff, and the reason this plot lives in the chat app: the
informant tells the player to do the one thing this game has been teaching
them all along — **copy that line and paste it into the mayor's own chat**.
Not a password, not a magic word: his own sentence, handed back to him. The
mayor's bot matches it the same forgiving way a delivery is matched
(whitespace, case and surrounding prose forgiven, so pasting the informant's
whole message works), and answers with the collapse: those are his words,
that is his handwriting, the orders to light the fires and cut the network
are all in the file, and **the mayor flees town**. His last message — *"I'm
leaving this town tonight and I'm never coming back."* — closes his chat.
Answered once per message (the op hash is persisted), so a re-sync never
replays it.

Then the win actually *lands*: seconds later **Nadia erupts, unprompted**, in
her own chat. The player is standing at the base station — her bot shares
that Pi with the mayor's, which is exactly what makes this possible. When his
trigger fires, his bot appends the triggering player's device id to a flag
file (`/var/lib/larp/mayor-triggered` — a shared tmpfiles dir, NOT his state
dir: a DynamicUser StateDirectory lives under `/var/lib/private`, 0700, and
would be invisible to her service); her bot polls the path
(`BotConfig::mayor_fallen_flag`, wired in `nix/larp-bot.nix`) and, the tick
it appears, sends her `mayor_fallen` line — **only into that player's chat**,
once: he's gone, the fires are going out, the internet is coming back, you
can relax now — thank you. The collapse is the courier's payoff, not
broadcast news; other players keep playing until their own endgame (each can
fell him with their own message — the flag collects one device id per line).
An *empty* flag (a facilitator's manual `touch`) means everyone hears. The
resolution comes from the character the player has been working with all
game, not from the villain, and it arrives without the player having to do
anything but stand there. Only Nadia has the line (linted): everywhere else
the flag never appears, so a `mayor_fallen` in any other pack would be dead
content. The game still closes entirely in-fiction — no out-of-character
congratulations.

(Resetting for a new game day = wiping `/var/lib` on the base Pi: the flag
and the mayor's state both live under it, so they clear together.)

Because the mayor's bot sits on the base station, the last delivery of the
game is a walk back to where it started. A single player can finish the whole
plot. A unit test asserts that what the informant hands out is exactly what
the mayor listens for — a drifted line would make the endgame unreachable.

Nothing has to be hidden on the map any more, and nothing is found by
accident: the side plot's shape is walk to Mira → get the link → meet the
informant on the spot → walk his evidence back to the town hall. (There is no
multi-instance identity anywhere now — informant and mayor are both one
identity on one card, so the old same-identity-on-every-station collision
caveats are gone with the poster.)

---

## 2. What already exists (reused unmodified)

- **The `mailbox-image` flake input** (raspberry-pi-mailbox-server): NixOS SD
  image for Pi 5 running the `replicating-local-mailbox-server` from the
  dash-chat flake input. Per-card configuration via env files on the FAT boot
  partition (`wifi.env`). This repo's station image is that image
  `extendModules`-ed with the bot. The image **hosts no AP**: the Pi's
  brcmfmac AP mode was the main source of field failures and was dropped
  upstream, along with all the range limiting that came with it. The radio is
  a **client** only (`nix/wifi-client.nix` upstream) — see §4.
- **mDNS announce/discovery**: stations announce `_dashchat._tcp.local.`, so
  players' apps auto-discover the mailbox once they and the station are on
  the same station AP.
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
- **Cloud mailbox**: still running, but **out of the game** since Mira moved
  into town — nothing on the map syncs through it any more. It stays useful
  for testing a bot from a laptop (`just characters::run`).
- **mAP lite tooling (this repo)**: `just base-station::map-lite::provision`
  turns a stock device into a station AP. Since no Pi hosts wifi any more,
  every station needs an AP of some kind next to it; the mAP lite is the
  known-good one (it comfortably carries the 30-40 concurrent clients the
  base station sees). Its `provision` recipe also disables the device's own
  DHCP server, which assumed the Pi served DHCP on the cable
  (`nix/base-station.nix`) — with the Pi now a plain wifi client, leave the
  AP's DHCP server ON instead.

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
FAT boot partition alongside `wifi.env`/`larp.env`. Re-flashing the image
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
  not been given yet — plus an immediate follow-up fire every time a delivery
  lands here, preferring templates not addressed back to the delivered
  mission's originator (the timer resets after it, so the two never
  double-fire). No pool reshuffle and no ack gate on a character's *own*
  missions — when the pack runs out for that player, the character stops
  handing out missions.
- **Delivery recognition — no visible metadata.** Messages are pure
  in-character prose; the machine layer rides on the text itself, since we
  author every pack:

  - All bots ship with **all five template packs** (they live in this repo).
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
  mailbox URL: `http://127.0.0.1:<port>` on the Pis (the cloud mailbox URL on
  the unused droplet). No iroh internet connectivity is assumed on the Pis (offline
  LAN blob sync already works per the mailbox-image repo's README; missions
  are text-only anyway).

### Configuration (`config.toml`)

```toml
# The character is NOT configured here: it comes from the identity bundle.
mailbox_url   = "http://127.0.0.1:3000"
identity      = "/boot/firmware/larp-identity.toml"  # flashed bundle (see above)
cast          = "/boot/firmware/larp-cast.toml"      # all characters' public ids
scenarios_dir = "/nix/store/…-scenarios"             # all packs, baked into the image
data_dir      = "/var/lib/larp-bot"                  # cache only — safe to wipe
# Optional: the flashed informant bundle, read for its public half only, to
# build the deep link in a pack's informant_tip. Absent → no tips.
anonymous_identity = "/boot/firmware/larp-anonymous.toml"

[timing]
min_interval_secs = 180
max_interval_secs = 480
first_mission_delay_secs = 5
poll_interval_secs = 3
```

Scenario pack (`scenarios/<character>.toml`): a list of
`{ to = "grandpa", text = "…", success = "…" }`
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
("Hey!! Anything? I'm losing my mind over here.") — she is stuck behind a
desk waiting for names. Tracking is in-memory and baselined on the first
scan, so bot restarts never trigger it.

A pack may also carry an optional `informant_tip` (a line containing
`{link}`): after a delivery lands, that character passes the player the
informant's add-contact deep link, once per player. Only Mira uses this one
too, and it is the only way into the side plot (§The mayor and the
informant). The link is built at startup from the flashed
`larp-anonymous.toml`, so a card without the informant simply never tips.

## 4. NixOS & deployment changes

### One image, per-card station selection

Keep the single-SD-image philosophy. **Implemented:** the bot service
(`nix/larp-bot.nix`, baked into every image with `services.larp-bot`) is gated
at runtime with `ConditionPathExists` on files the FAT boot partition may
carry, next to `wifi.env`:

- `larp-identity.toml` — the character's flashed identity bundle
- `larp-cast.toml` — the public cast file (flashed too, **not** baked into the
  image: it changes per game, the image doesn't)
- `larp-anonymous.toml` — the anonymous informant's identity (gates the
  informant service the same way; **only** the sister's card gets it —
  `characters.just` `informant_character` — since Mira is the one who hands
  out his contact)
- `larp-mayor.toml` — the mayor's identity (gates his service; **only**
  `base-station::flash` writes it, so he exists on exactly one card)

No file → no bot: the card is a plain mailbox appliance. There is now
**literally one image** — the base station stopped being a variant when the
captive portal went away — and the station types are nothing but combinations
of flashed files:

| Station | mailbox | Wi-Fi client | character bot | mayor | informant |
|---|---|---|---|---|---|
| base | ✓ | ✓ | ✓ (neighbour) | ✓ | – |
| mum / grandpa / cousin | ✓ | ✓ | ✓ (identity flashed) | – | – |
| sister | ✓ | ✓ | ✓ (identity flashed) | – | ✓ |

`just base-station::flash` writes the base card's `wifi.env` (SSID
`OfflineWifi`, open, by default) plus the mayor's identity AND the
neighbour's character files (her identity + the cast): the base Pi runs two
bots, the mayor and Nadia. `characters::flash neighbour` refuses to run — a
second card with her identity would be a duplicate instance of her. No
informant here: he is Mira's alone.

### Wi-Fi: every Pi is a client of `OfflineWifi`

No Pi hosts an AP. The mailbox image dropped AP mode entirely — the Pi 5's
brcmfmac AP was the main source of field failures — and with it went the
range limiting this repo used to override (tx clamp, rate floor, RSSI gate,
`dashchat-ap-guard`). What remains upstream is `nix/wifi-client.nix`: a boot
service that reads `/boot/firmware/wifi.env` and runs wpa_supplicant.

Both flash recipes write that file, defaulting to the same network everywhere:

```
WIFI_SSID=OfflineWifi
WIFI_PASSWORD=
```

An empty password means an open network (`key_mgmt=NONE`). Pass `ssid` /
`password` arguments to either recipe to point a card elsewhere; the file is
read at every boot, so it can also be edited on the card or over SSH with no
reflash.

What this implies on game day:

- **Each station needs its own AP** next to its Pi — a mAP lite (see the
  `map-lite` recipes, but read the staleness warning there), a home router, a
  spare phone. The Pi and the players' phones just have to land on the same
  L2 segment for mDNS.
- **Every AP broadcasts the same open SSID `OfflineWifi`**, so a phone that
  joined once re-joins by itself at every stop. The cost: the SSID no longer
  tells a facilitator which bubble they are in (it used to be `mum-larp`,
  `grandpa-larp`, …).
- **Keep the APs on separate LANs** (each with its own DHCP server, don't
  bridge them together). Same SSID, separate islands — that is what keeps the
  sneakernet real. Two stations on one LAN would replicate mailboxes directly
  and hand players messages they were supposed to carry.
- **Station range is now the AP's problem**, not a nix option: tune it on the
  AP if a bubble is too big or too small.

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
  Dash Chat, scan all six posters, the mayor's first.* It is the only instruction that does
  not live in a chat.

*(Unused module)* `nix/base-station.nix` — the Pi owning DHCP + wildcard DNS
on a cable to a MikroTik mAP lite — stays out of `flake.nix`. It predates
the client-mode switch and no longer evaluates as-is (it references the
deleted captive-portal module). The mAP lite itself is still a fine station
AP; just let it serve its own DHCP.

Every station is now the same open `OfflineWifi` network (see §Wi-Fi), and
joining any of them (the base station included) looks like a dead network:
nothing pops, and the app finds that station's mailbox via mDNS on its own
port. The facilitator can no longer tell stations apart by SSID — label the
hardware instead.

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
   **Six** of them: the five characters and the mayor, all on the
   base-station wall (his is the one players scan *first*). The informant is
   skipped on purpose — he is reached through Mira's link, never off a wall.
3. `just characters::flash <character> /dev/sdX` — flashes the station image and
   puts the character's files (`wifi.env` with `WIFI_SSID=OfflineWifi` and an
   empty `WIFI_PASSWORD=` — an **open network** — unless `ssid` / `password`
   arguments say otherwise, plus `larp-identity.toml` and `larp-cast.toml`,
   assembled on the fly from `secrets/`) on the card's boot partition. The
   sister's card additionally gets `larp-anonymous.toml` — the informant runs
   there and nowhere else. The neighbour is refused: her bot lives on the
   base card.
4. `just base-station::flash /dev/sdX` — the same image, with
   `larp-mayor.toml` plus the neighbour's `larp-identity.toml` and
   `larp-cast.toml`: two bots on one Pi. No informant.

**Seed the base mailbox with the cast's profiles** (once, after the bots have
booted): each character's profile lives on its bot's announcements topic,
seeded only at its own station — and replication never introduces a mailbox
to topics it doesn't know. Without seeding, contacts added from the wall
posters appear *nameless* at the base station, right when players are
learning which chat belongs to which character. The fix uses the client push
path: on one phone, add all five characters (walk their stations — Nadia's
profile is already at the base — or plug all the Pis into one ethernet
switch, where the mailboxes discover and push to each other), then stand on
the base station's Wi-Fi for a minute. The phone pushes the announcements
topics into the base mailbox, permanently. Do
NOT seed by running a character bot against the base mailbox with a fresh
data dir — same identity, second op log, forked history.

### The cloud host (kept, unused)

Mira used to be the one character outside the town, running the same
`larp-bot` service against the cloud mailbox with a phone hotspot as her
corner of the map. **She is a Pi station now** (the school shelter desk), so
nothing below is part of a game day: flash her card with
`just characters::flash sister` like anybody else's. It is kept — recipes and
the `sister-droplet` flake config both marked unused — for a future character
on the far side of a hotspot. Note that whoever plays that role cannot be the
one who hands out the informant: the droplet carries no informant identity,
so there is no link for the tip to contain.

*How it worked:* `just sister::deploy` provisions the whole thing with
doctl — first run creates an Ubuntu droplet whose cloud-init converts it to
NixOS in place (nixos-infect), then pushes the flake's `sister-droplet`
config plus the secrets (same `keygen` artifacts, delivered over SSH to
`/var/lib/larp-secrets/` instead of a FAT partition); later runs skip
straight to the push. `sister::logs` follows the bot's journal,
`sister::destroy` tears the droplet down (the identity survives in
`secrets/`). The flake also still exports `nixosModules.larp-bot` with a
usage example (see flake.nix) for wiring the bot into an existing NixOS
host instead — e.g. the droplet already running the cloud mailbox.

`just characters::run <character> [mailbox_url]` still runs any bot on the
laptop against a mailbox, which is the quickest way to try a pack (or Mira's
informant tip — the recipe passes `secrets/anonymous-identity.toml` along
when it exists, so the link in her tip is the real one). State lives in
`.run/<character>/` (wipe it to simulate a reset; identity survives, it's in
`secrets/`).

**Pick the mailbox URL to match the players' app build**: release builds use
the production mailbox, dev builds may point at staging. A bot synced to a
different cloud mailbox than the players' apps never sees their chats.

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

Mira's station works exactly the same way — she is a Pi like the rest now.
The one extra beat there: a delivery *to her* is also answered with the
informant's contact link, which is what opens the side plot.

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
  at the base station must carry "install Dash Chat, scan all six posters
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
3. ~~**The sister's droplet**~~ — done, then retired: Mira is a Pi station
   now. The droplet config and recipes are kept but unused (§The cloud host).
4. **Scenario content + dress rehearsal** — write the four template packs and
   the two spec-bot scripts, full field test (5 Pis), print the five QR wall
   posters and the base station's paper sign, tune intervals/caps.
