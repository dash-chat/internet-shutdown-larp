# Recipes for the internet-shutdown LARP, organized in modules — run `just`
# to list everything. Run inside `nix develop` (or prefix with
# `nix run nixpkgs#just --`).

# Character provisioning: identity bundles, cast file, QR posters,
# per-station env dirs, local bot runs.
mod characters

# Station SD images: build + flash (station and base-station variants).
mod image

# The base station: flash the Pi card; the map-lite submodule provisions
# the mAP lite that broadcasts its wifi.
mod base-station

# The journalist's cloud host: deploy the bot to a Digital Ocean droplet
# (doctl + nixos-infect), plus ssh/logs/destroy.
mod journalist

# The relative's LoRa link: RNode flashing + relative-link station cards
# (RNS mailbox gateway, docs/rns-gateway.md).
mod lora

# Show available recipes.
_default:
    @just --list --list-submodules

# Run all tests (unit + e2e + gateway).
test:
    cargo test --workspace
    python3 -m unittest discover gateway
