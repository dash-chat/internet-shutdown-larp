# The base station: a Pi hosting its own wifi network like every other
# station (wifi-ap.env on the boot partition), serving the mailbox and
# running the MAYOR's bot — the contact players scan first, whose greeting
# explains the whole game and whose trigger phrase ends it (docs/design.md).
#
# There is no separate base-station image any more: the mayor is a chat
# character, not a captive portal, so this card runs the plain station image
# and differs only in the files flashed onto it. The mAP-lite-as-AP setup
# (map-lite submodule + nix/base-station.nix) is kept but currently unused.

# The mAP lite AP tooling (currently unused): connect / cli / provision.
mod map-lite

# Flash the base-station Pi card: the station image plus wifi-ap.env, the
# mayor's identity and the anonymous informant's. No character identity and
# no cast file, so the character-bot service stays dormant here. The AP is
# OPEN by default; pass a password (8-63 chars, WPA2) to lock it. Build the
# image first with `just image::build`. With no device, the SD card is
# auto-detected (see image::flash).
[doc("Flash the base-station Pi card (mayor + informant, no character bot); AP is <ssid>, open unless a password is given; auto-detects the SD card")]
flash device="" ssid="internet-shutdown-larp" password="":
    #!/usr/bin/env bash
    set -euo pipefail
    just={{ quote(just_executable()) }}
    ssid={{ quote(ssid) }}
    password={{ quote(password) }}
    [ -f secrets/mayor-identity.toml ] || { echo "no secrets/mayor-identity.toml — run 'just characters::generate' first"; exit 1; }
    [ -f secrets/anonymous-identity.toml ] || { echo "no secrets/anonymous-identity.toml — run 'just characters::generate' first"; exit 1; }
    if [ -n "$password" ] && { [ "${#password}" -lt 8 ] || [ "${#password}" -gt 63 ]; }; then
      echo "wifi password must be 8-63 characters (WPA2), got ${#password}"; exit 1
    fi
    [ "${#ssid}" -le 32 ] || { echo "SSID must be at most 32 bytes (802.11 limit), got ${#ssid}: $ssid"; exit 1; }
    envdir="$(mktemp -d)"
    trap 'rm -rf "$envdir"' EXIT
    # An empty PASSWORD= line means an open network (see the mailbox image's
    # wifi-ap.env semantics).
    printf 'SSID=%s\nPASSWORD=%s\n' "$ssid" "$password" > "$envdir/wifi-ap.env"
    cp secrets/mayor-identity.toml "$envdir/larp-mayor.toml"
    cp secrets/anonymous-identity.toml "$envdir/larp-anonymous.toml"
    "$just" image::flash {{ quote(device) }} "$envdir"
    echo ">> card is ready to boot as $ssid$([ -z "$password" ] && echo ' (open network)' || true) + mayor + anonymous informant"
