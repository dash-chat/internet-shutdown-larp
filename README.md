# Internet-shutdown LARP

A live-action game about carrying information when the network is gone:
players are couriers in a town cut in two by fires, Raspberry Pi stations
running [Dash Chat](https://github.com/dash-chat/dash-chat) mailboxes are the
only communication infrastructure left, and bots impersonating town
characters produce messages that must be physically carried to their
destinations.

**Read [docs/design.md](docs/design.md)** — the full design: cast, physical
layout, message mechanics, identity bundles, and the milestone plan.

## What's here

- `crates/larp-bot/` — the character bot: a headless `dashchat-node` that
  auto-accepts contacts, greets groups, fires scripted missions and
  acknowledges deliveries. Also the provisioning tool (`keygen` / `qr` /
  `cast`).
- `scenarios/` — the four characters' mission packs (pure content).
- `nix/` — the bot's NixOS module and package.
- `flake.nix` — extends the plain
  [raspberry-pi-mailbox-server](https://github.com/dash-chat/raspberry-pi-mailbox-server)
  image (a flake input) with the bot: **one station image for every card**;
  the bot only starts on cards flashed with an identity bundle.

## Quick start

```sh
nix develop                 # rust toolchain + just
just test                   # unit + e2e tests

just characters::generate              # all identities + cast file → secrets/
just characters::posters               # printable QR posters
just image::build                      # station SD image
just characters::flash firefighters    # flash + station files (auto-detects the SD card)
just journalist::deploy                # journalist bot → Digital Ocean droplet (doctl)
```
