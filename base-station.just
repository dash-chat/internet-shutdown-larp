# The base station: a Pi hosting its own wifi network like every other
# station (wifi-ap.env on the boot partition), serving the mayor captive
# portal and the mailbox. The mAP-lite-as-AP setup (map-lite submodule +
# nix/base-station.nix) is kept but currently unused.

# The mAP lite AP tooling (currently unused): connect / cli / provision.
mod map-lite

# Flash the base-station Pi card: base-station.img plus wifi-ap.env — the
# base station broadcasts its own wifi, like the character stations. The AP
# is OPEN by default; pass a password (8-63 chars, WPA2) to lock it. Build
# first with `just image::build-base-station`. With no device, the SD card
# is auto-detected (see image::flash).
[doc("Flash the base-station Pi card; AP is <ssid> (open unless password given); auto-detects the SD card")]
flash device="" ssid="larp-base" password="":
    #!/usr/bin/env bash
    set -euo pipefail
    just={{ quote(just_executable()) }}
    ssid={{ quote(ssid) }}
    password={{ quote(password) }}
    if [ -n "$password" ] && { [ "${#password}" -lt 8 ] || [ "${#password}" -gt 63 ]; }; then
      echo "wifi password must be 8-63 characters (WPA2), got ${#password}"; exit 1
    fi
    envdir="$(mktemp -d)"
    trap 'rm -rf "$envdir"' EXIT
    # An empty PASSWORD= line means an open network (see the mailbox image's
    # wifi-ap.env semantics).
    printf 'SSID=%s\nPASSWORD=%s\n' "$ssid" "$password" > "$envdir/wifi-ap.env"
    "$just" image::flash {{ quote(device) }} "$envdir" base-station.img
    echo ">> card is ready to boot as $ssid$([ -z "$password" ] && echo ' (open network)' || true)"
