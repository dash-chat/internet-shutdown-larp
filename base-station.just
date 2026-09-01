# The base station: a Pi joining the OfflineWifi network like every other
# station (wifi.env on the boot partition), serving the mailbox and running
# the MAYOR's bot — the contact players scan first, whose greeting explains
# the whole game and whose trigger phrase ends it (docs/design.md).
#
# There is no separate base-station image any more: the mayor is a chat
# character, not a captive portal, so this card runs the plain station image
# and differs only in the files flashed onto it. No Pi hosts wifi any more
# either — the mailbox image dropped AP mode, so an AP (the mAP lite below,
# or any router) broadcasts OfflineWifi and the Pi is one of its clients.
# nix/base-station.nix, which put the Pi in charge of DHCP/DNS on the cable,
# stays unused: let the AP keep its own DHCP server instead.

# The mAP lite AP tooling: connect / cli / provision.
mod map-lite

# Flash the base-station Pi card: the station image plus wifi.env, the
# mayor's identity and the anonymous informant's. No character identity and
# no cast file, so the character-bot service stays dormant here. The Pi joins
# <ssid> on boot (open unless a password of 8-63 chars is given). Build the
# image first with `just image::build`. With no device, the SD card is
# auto-detected (see image::flash).
[doc("Flash the base-station Pi card (mayor + informant, no character bot); the Pi joins wifi <ssid>, open unless a password is given; auto-detects the SD card")]
flash device="" ssid="OfflineWifi" password="":
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
    # An empty WIFI_PASSWORD= line means an open network (see the mailbox
    # image's nix/wifi-client.nix).
    printf 'WIFI_SSID=%s\nWIFI_PASSWORD=%s\n' "$ssid" "$password" > "$envdir/wifi.env"
    cp secrets/mayor-identity.toml "$envdir/larp-mayor.toml"
    cp secrets/anonymous-identity.toml "$envdir/larp-anonymous.toml"
    "$just" image::flash {{ quote(device) }} "$envdir"
    echo ">> card is ready: joins wifi '$ssid'$([ -z "$password" ] && echo ' (open network)' || true) on boot, runs the mayor + the anonymous informant"
