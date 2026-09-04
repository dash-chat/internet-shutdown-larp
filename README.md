# Internet-shutdown LARP

A live-action game about carrying information when the network is gone:
players are couriers in a town ravaged by fires, Raspberry Pi stations
running [Dash Chat](https://github.com/dash-chat/dash-chat) mailboxes are the
only communication infrastructure left, and bots impersonating the player's
own family produce messages that must be physically carried to their
destinations.

**Read [docs/design.md](docs/design.md)** — the full design: cast, physical
layout, message mechanics, identity bundles, and the milestone plan.

## The cast

Five characters trade missions (`scenarios/`): **Mama** at home, **Grandpa
Amir** up the hill, **Rafa** the cousin at the fire line, **Mira**, the
sister on the desk at the school shelter with the list of who has arrived —
each on their own Pi — and **Nadia the neighbour**, the town's tinkerer,
whose bot rides the base-station Pi. The offline boxes that keep the chats
alive are hers (up for years before the blackout), her greeting is the real
tutorial, and her first mission opens the game.

One more has no missions and no cast entry, and he is the side plot
(`mayor.toml`): **the Mayor**, whose QR poster players scan first — an
emergency notice that deliberately never encourages the comms he secretly
cut. The way in is Nadia: the first time a player carries a delivery to
her, she tells them what she saw in the town hall — ending with one line
she read word for word off the mayor's own written order. Pasting that line
into the mayor's chat, like every other message in this game, ends it.

## What's here

- `crates/larp-bot/` — the bot: a headless `dashchat-node` that auto-accepts
  contacts, greets each player in their own direct chat, fires scripted
  missions and acknowledges the deliveries players paste in (`run`), or plays
  a scripted pack-less character with an optional trigger phrase (`spec`).
  Also the provisioning tool (`keygen` / `qr` / `cast`).
- `scenarios/` — the four family characters' mission packs (pure content).
- `mayor.toml` — the spec-bot script (the side plot's endgame).
- `nix/` — the NixOS modules and packages.
- `flake.nix` — extends the plain
  [raspberry-pi-mailbox-server](https://github.com/dash-chat/raspberry-pi-mailbox-server)
  image (a flake input) with the bot: **one station image for every card**,
  base station included. Which bots run is decided entirely by the identity
  files a flash recipe puts on the card. No captive portal anywhere.

The Pis host no Wi-Fi: the image dropped AP mode, so each station sits behind
its own AP and joins it as a client (`wifi.env`, `WIFI_SSID=OfflineWifi`, open
network by default). Same SSID everywhere so a phone re-joins by itself at
every stop — but keep the APs on **separate LANs**, or the stations replicate
to each other and there is nothing left to carry.

## Quick start

```sh
nix develop                 # rust toolchain + just
just test                   # unit + e2e tests

just characters::generate              # all identities + cast file → secrets/
just characters::posters               # printable QR posters (six: five characters + mayor)
just image::build                      # the station SD image (one for every card)
just characters::flash mum             # flash + station files (auto-detects the SD card)
just base-station::flash               # the base card: the mayor + Nadia's bot
```
