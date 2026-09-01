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

# Writes the RTC when one is present (battery on J5 — then the time
# survives power-off and reflashing; without it, only until shutdown).
[doc("Push the laptop's time to the Pi on the direct ethernet link (writes the RTC if present)")]
set-time iface="":
    nix run .#ethernet-set-time --accept-flake-config -- {{ iface }}
