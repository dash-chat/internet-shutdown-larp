# The relative's LoRa link (docs/rns-gateway.md): two Heltec dev kits
# flashed with RNode firmware, one per relative-link Pi, carrying mailbox
# sync over Reticulum. The Pi-side gateway is baked into the station image
# and gated on /boot/firmware/lora.env — these recipes flash the radios and
# assemble the relative-link cards.

# Flash RNode firmware onto a Heltec over USB (interactive: rnodeconf asks
# for the board model). Do this once per radio; the gateway drives radio
# parameters from lora.env on every boot, so no further radio provisioning.
[doc("Flash RNode firmware onto a Heltec (interactive, once per radio)")]
rnode-install serial="/dev/ttyACM0":
    nix shell .#rns-gateway.py --command rnodeconf {{ serial }} --autoinstall

# Both relative-link cards get the same lora.env: the radios discover each
# other over RNS announces, so there is no per-side peer configuration.
[private]
lora-env dir serial freq sf:
    #!/usr/bin/env bash
    set -euo pipefail
    {
      printf 'RNODE_PORT=%s\n' {{ quote(serial) }}
      printf 'RNODE_FREQ=%s\n' {{ quote(freq) }}
      printf 'RNODE_BANDWIDTH=125000\n'
      printf 'RNODE_SF=%s\n' {{ quote(sf) }}
      printf 'RNODE_CR=5\n'
      printf 'RNODE_TXPOWER=7\n'
    } > {{ quote(dir) }}/lora.env

# The on-map end of the link: a plain station (AP + mailbox, no character)
# plus the gateway. Players deposit Anna's messages here; the gateway
# carries them to Riverside.
[doc("Flash the relative-near card: AP + mailbox + LoRa gateway (no bot)")]
flash-near device="" ssid="larp-relative" password="" serial="/dev/ttyACM0" freq="869525000" sf="8":
    #!/usr/bin/env bash
    set -euo pipefail
    just={{ quote(just_executable()) }}
    envdir="$(mktemp -d)"
    trap 'rm -rf "$envdir"' EXIT
    printf 'SSID=%s\nPASSWORD=%s\n' {{ quote(ssid) }} {{ quote(password) }} > "$envdir/wifi-ap.env"
    "$just" lora::lora-env "$envdir" {{ quote(serial) }} {{ quote(freq) }} {{ quote(sf) }}
    "$just" image::flash {{ quote(device) }} "$envdir"
    echo ">> relative-near card ready ({{ ssid }} + LoRa gateway)"

# The Riverside end: Aunt Anna's bot + mailbox + gateway. It also broadcasts
# its own (out-of-play) AP: mDNS between the gateway and the mailbox needs a
# live multicast interface, and the bubble doubles as facilitator debug
# access in the far location.
[doc("Flash the relative-far card: bot + mailbox + LoRa gateway (+ debug AP)")]
flash-far character="relative" device="" ssid="larp-riverside" password="" serial="/dev/ttyACM0" freq="869525000" sf="8":
    #!/usr/bin/env bash
    set -euo pipefail
    just={{ quote(just_executable()) }}
    [ -f "secrets/{{ character }}-identity.toml" ] || { echo "no secrets/{{ character }}-identity.toml — run 'just characters::generate' first"; exit 1; }
    [ -f secrets/larp-cast.toml ] || { echo "no secrets/larp-cast.toml — run 'just characters::generate' first"; exit 1; }
    envdir="$(mktemp -d)"
    trap 'rm -rf "$envdir"' EXIT
    printf 'SSID=%s\nPASSWORD=%s\n' {{ quote(ssid) }} {{ quote(password) }} > "$envdir/wifi-ap.env"
    cp "secrets/{{ character }}-identity.toml" "$envdir/larp-identity.toml"
    cp secrets/larp-cast.toml "$envdir/larp-cast.toml"
    "$just" lora::lora-env "$envdir" {{ quote(serial) }} {{ quote(freq) }} {{ quote(sf) }}
    "$just" image::flash {{ quote(device) }} "$envdir"
    echo ">> relative-far card ready ({{ character }} bot + LoRa gateway)"

# Run the gateway on the laptop against a local mailbox (bench test with
# two radios on one desk: run it twice with different --data-dir/--port-base
# and two mailboxes). State in .run/gateway*/.
[doc("Run the gateway locally against a mailbox (bench testing)")]
run serial="/dev/ttyACM0" mailbox_url="http://127.0.0.1:3000" data_dir=".run/gateway":
    #!/usr/bin/env bash
    set -euo pipefail
    mkdir -p {{ quote(data_dir) }}
    RNODE_PORT={{ quote(serial) }} GATEWAY_MAILBOX_URL={{ quote(mailbox_url) }} \
      GATEWAY_DATA_DIR={{ quote(data_dir) }} \
      nix run .#rns-gateway
