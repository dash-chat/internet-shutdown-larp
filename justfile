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

# Set a station Pi's clock from this laptop over a direct ethernet cable:
# discovers the Pi on the link (IPv4 neighbors first — e.g. a 10.55.0.x
# lease when the laptop-side DHCP-server trick is running — then IPv6
# link-local all-nodes ping), pushes the laptop's time over ssh and writes
# it to the RTC when one is present (battery on J5 — then the time survives
# power-off and reflashing; without it, only until shutdown). Caveat: with
# no DHCP server on the cable, the Pi's link only stays up ~2 min after
# boot (NetworkManager thrashes on the leaseless DHCP client) — power-cycle
# the Pi and run this shortly after it boots.
[doc("Push the laptop's time to the Pi on the direct ethernet link (writes the RTC if present)")]
set-time iface="":
    #!/usr/bin/env bash
    set -euo pipefail
    iface={{ quote(iface) }}
    ssh_opts=(-o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o BatchMode=yes -o ConnectTimeout=4 -o LogLevel=ERROR)

    if [ -z "$iface" ]; then
      for d in /sys/class/net/*; do
        dev=$(basename "$d")
        [ "$dev" = lo ] && continue
        [ -e "$d/device" ] || continue    # physical devices only
        [ -d "$d/wireless" ] && continue  # skip wifi
        [ "$(cat "$d/carrier" 2>/dev/null || echo 0)" = 1 ] || continue
        iface="$dev"; break
      done
      [ -n "$iface" ] || { echo "error: no wired interface with a cable plugged in — is the Pi connected and powered?" >&2; exit 1; }
    fi
    echo ">> looking for the Pi on $iface"

    candidates=()
    # IPv4 neighbors (e.g. the 10.55.0.x lease from the DHCP-server trick).
    while read -r addr; do candidates+=("$addr"); done \
      < <(ip -4 neigh show dev "$iface" | awk '$NF != "FAILED" { print $1 }')
    # IPv6 link-local: ping all-nodes, responders minus ourselves.
    own=$(ip -6 addr show dev "$iface" scope link 2>/dev/null \
      | awk '/inet6/ { sub(/\/.*/, "", $2); print $2; exit }')
    if [ -n "$own" ]; then
      while read -r addr; do
        [ "$addr" = "$own" ] || candidates+=("$addr%$iface")
      done < <(ping -6 -c 3 -W 1 -I "$iface" ff02::1 2>/dev/null | grep -oE 'fe80:[0-9a-f:]+' | sort -u)
    elif [ ${#candidates[@]} -eq 0 ]; then
      echo "error: no IPv6 link-local address on $iface — enable it and retry:" >&2
      echo "  sudo sysctl -w net.ipv6.conf.$iface.addr_gen_mode=0 && sudo ip link set $iface down && sudo ip link set $iface up" >&2
      exit 1
    fi
    [ ${#candidates[@]} -gt 0 ] || { echo "error: no Pi found on $iface — power-cycle the Pi and re-run within ~2 min of its boot" >&2; exit 1; }

    pi=""
    for c in "${candidates[@]}"; do
      echo ">> trying $c"
      if ssh "${ssh_opts[@]}" "admin@$c" true 2>/dev/null; then pi="$c"; break; fi
    done
    [ -n "$pi" ] || { echo "error: ${#candidates[@]} neighbor(s) on $iface but none accepted ssh as admin@ — is this a station Pi? Power-cycle it and re-run within ~2 min of its boot." >&2; exit 1; }

    # Stamp the epoch as late as possible ($(date +%s) expands HERE, on the
    # laptop); date -s @<epoch> is timezone-proof.
    out=$(ssh "${ssh_opts[@]}" "admin@$pi" "sudo /run/current-system/sw/bin/date -u -s @$(date +%s) > /dev/null && { sudo /run/current-system/sw/bin/hwclock -w 2>/dev/null && echo RTC_OK || echo RTC_MISSING; } && date")
    rtc=$(head -n1 <<< "$out")
    echo ">> Pi clock set: $(tail -n1 <<< "$out")"
    case "$rtc" in
      RTC_OK)      echo ">> RTC written — the time now survives power-off and reflashing" ;;
      RTC_MISSING) echo ">> no RTC found (no battery on J5, or /dev/rtc0 missing) — the time holds until the Pi powers off" ;;
    esac
