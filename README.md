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

Four family characters trade missions (`scenarios/`): **Mama** at home,
**Grandpa Amir** up the hill, **Nadia the neighbour** next door, and
**Mira**, the sister stuck in the city with the only working internet — she
runs on a cloud droplet, the rest on their own Pi.

Two more have no missions and no cast entry, and between them they are the
side plot (`mayor.toml`, `anonymous.toml`): **the Mayor**, whose QR poster
players scan first and whose greeting *is* the tutorial, and **Anonymous**,
whose poster is hidden on the map and who hands out the password that brings
the mayor down. Sending that password to the mayor — copied and pasted into
his chat, like every other message in this game — ends it.

## What's here

- `crates/larp-bot/` — the bot: a headless `dashchat-node` that auto-accepts
  contacts, greets each player in their own direct chat, fires scripted
  missions and acknowledges the deliveries players paste in (`run`), or plays
  a scripted pack-less character with an optional trigger phrase (`spec`).
  Also the provisioning tool (`keygen` / `qr` / `cast`).
- `scenarios/` — the four family characters' mission packs (pure content).
- `mayor.toml` / `anonymous.toml` — the two spec-bot scripts (the side plot).
- `nix/` — the NixOS modules and packages.
- `flake.nix` — extends the plain
  [raspberry-pi-mailbox-server](https://github.com/dash-chat/raspberry-pi-mailbox-server)
  image (a flake input) with the bot: **one station image for every card**,
  base station included. Which bots run is decided entirely by the identity
  files a flash recipe puts on the card. No captive portal anywhere.

## Quick start

```sh
nix develop                 # rust toolchain + just
just test                   # unit + e2e tests

just characters::generate              # all identities + cast file → secrets/
just characters::posters               # printable QR posters (six of them)
just image::build                      # the station SD image (one for every card)
just characters::flash mum             # flash + station files (auto-detects the SD card)
just base-station::flash               # the base card: mayor + informant, no character
just sister::deploy                    # the sister's bot → Digital Ocean droplet (doctl)
```
