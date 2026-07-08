# Recipes for the internet-shutdown LARP, organized in modules — run `just`
# to list everything. Run inside `nix develop` (or prefix with
# `nix run nixpkgs#just --`).

# Character provisioning: identity bundles, cast file, QR posters,
# per-station env dirs, local bot runs.
mod characters

# Station SD images: build + flash (station and base-station variants).
mod image

# The base station: flash the Pi card (hosts its own wifi, like the other
# stations); the map-lite submodule is the currently-unused mAP-lite tooling.
mod base-station

# The journalist's cloud host: deploy the bot to a Digital Ocean droplet
# (doctl + nixos-infect), plus ssh/logs/destroy.
mod journalist

# Show available recipes.
_default:
    @just --list --list-submodules

# Run all tests (unit + e2e).
test:
    cargo test --workspace
