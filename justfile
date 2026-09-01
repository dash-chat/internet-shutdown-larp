# Recipes for the internet-shutdown LARP, organized in modules — run `just`
# to list everything. Run inside `nix develop` (or prefix with
# `nix run nixpkgs#just --`).

# Character provisioning: identity bundles, cast file, QR posters,
# per-station env dirs, local bot runs.
mod characters

# Station SD images: build + flash (station and base-station variants).
mod image

# The base station: flash the Pi card (joins the game's wifi, like the other
# stations); the map-lite submodule provisions a MikroTik mAP lite as the AP
# that broadcasts it.
mod base-station

# The sister's cloud host: CURRENTLY UNUSED — Mira runs on a Pi station like
# everyone else now. Kept for a future off-map character (doctl +
# nixos-infect deploy, ssh/logs/destroy).
mod sister

# Show available recipes.
_default:
    @just --list --list-submodules

# Run all tests (unit + e2e).
test:
    cargo test --workspace

# The direct-ethernet-cable helpers live in the mailbox image repo
# (scripts/ there, exported as nix packages: find-pi, ethernet-ssh,
# ethernet-set-time) and are re-exported by this flake. Caveat: with no
# DHCP server on the cable, the Pi's link only stays up ~2 min after boot
# (NetworkManager thrashes on the leaseless DHCP client) — power-cycle the
# Pi and run these shortly after it boots.

# Extra arguments become the remote command; with none you get an
# interactive shell.
[doc("SSH into the Pi on the direct ethernet link (optional remote command)")]
ssh *cmd:
    nix run .#ethernet-ssh --accept-flake-config -- {{ cmd }}

# Update a running, already-flashed station over the direct ethernet cable,
# without reflashing. Two halves:
#   1. the SYSTEM (bot binary, scenarios, services) — the mailbox image's
#      ethernet-deploy switches the Pi to this tree's larp-station closure.
#      On an x86_64 host without aarch64 emulation the exact working tree
#      must already be in the binary cache: commit, push, wait for CI.
#   2. the CARD FILES (identity, cast, extra bots) — refreshed on
#      /boot/firmware from secrets/ and the bots restarted. WHICH files is
#      read off the card itself: larp-identity.toml names the character, and
#      the presence of larp-mayor.toml / larp-anonymous.toml marks the base
#      station / Mira's card. wifi.env is left alone.
# Reflash only for partition-layout or boot-breaking changes.
[doc("Deploy code + character files to the running Pi on the ethernet cable (no reflash)")]
deploy:
    #!/usr/bin/env bash
    set -euo pipefail
    pi="$(nix run .#find-pi --accept-flake-config)"
    ssh_opts=(-o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o LogLevel=ERROR)
    on_pi() { ssh "${ssh_opts[@]}" "admin@$pi" "$@"; }
    char="$(on_pi "sed -n 's/^character = \"\(.*\)\"/\1/p' /boot/firmware/larp-identity.toml")"
    [ -n "$char" ] || { echo "no character on the card (/boot/firmware/larp-identity.toml) — is this a flashed station?"; exit 1; }
    [ -f "secrets/$char-identity.toml" ] || { echo "the card runs '$char' but there is no secrets/$char-identity.toml"; exit 1; }
    [ -f secrets/larp-cast.toml ] || { echo "no secrets/larp-cast.toml — run 'just characters::generate' first"; exit 1; }
    echo ">> station runs '$char' — switching the system"
    nix run .#ethernet-deploy --accept-flake-config -- . larp-station
    echo ">> refreshing character files on the boot partition"
    on_pi "sudo tee /boot/firmware/larp-identity.toml >/dev/null" < "secrets/$char-identity.toml"
    on_pi "sudo tee /boot/firmware/larp-cast.toml >/dev/null" < secrets/larp-cast.toml
    units=(larp-bot)
    if on_pi "test -f /boot/firmware/larp-mayor.toml"; then
      on_pi "sudo tee /boot/firmware/larp-mayor.toml >/dev/null" < secrets/mayor-identity.toml
      units+=(larp-bot-mayor)
    fi
    if on_pi "test -f /boot/firmware/larp-anonymous.toml"; then
      on_pi "sudo tee /boot/firmware/larp-anonymous.toml >/dev/null" < secrets/anonymous-identity.toml
      units+=(larp-bot-anonymous)
    fi
    echo ">> restarting: ${units[*]}"
    on_pi "sudo systemctl restart ${units[*]}"
    echo ">> deployed: '$char' station is on the new system with fresh card files"

# Writes the RTC when one is present (battery on J5 — then the time
# survives power-off and reflashing; without it, only until shutdown).
[doc("Push the laptop's time to the Pi on the direct ethernet link (writes the RTC if present)")]
set-time iface="":
    nix run .#ethernet-set-time --accept-flake-config -- {{ iface }}
