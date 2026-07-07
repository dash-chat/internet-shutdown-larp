# The base station: a MikroTik mAP lite broadcasts the wifi (see the
# map-lite submodule), wired to a Pi running the base-station image
# (nix/base-station.nix) that serves DHCP/DNS, the mayor captive portal and
# the mailbox.

# The mAP lite AP: connect / cli / provision.
mod map-lite

# Flash the base-station Pi card: base-station.img, no env files — the base
# station hosts no Pi wifi. Build first with `just image::build-base-station`.
# With no device, the SD card is auto-detected (see image::flash).
[doc("Flash the base-station Pi card (base-station.img, no env files); auto-detects the SD card")]
flash device="":
    #!/usr/bin/env bash
    set -euo pipefail
    just={{ quote(just_executable()) }}
    "$just" image::flash {{ quote(device) }} "" base-station.img
