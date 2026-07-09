# The relative's LoRa link (docs/rns-gateway.md): two Heltec dev kits
# flashed with RNode firmware, one per relative-link Pi, carrying mailbox
# sync over Reticulum. The Pi-side gateway is baked into the station image
# and gated on /boot/firmware/lora.env — these recipes flash the radios and
# assemble the relative-link cards.

# Flash RNode firmware onto a Heltec over USB (interactive: rnodeconf asks
# for the board model). Do this once per radio; the gateway drives radio
# parameters from lora.env on every boot, so no further radio provisioning.
[doc("Flash RNode firmware onto a Heltec (interactive, once per radio)")]
flash-rnode serial="/dev/ttyACM0":
    nix shell .#rns-gateway.py --command rnodeconf {{ serial }} --autoinstall

# Flash a MeshCore companion build from GitHub onto a Heltec with esptool
# (board: e.g. heltec_v4 / heltec_v3, variant: ble for the phone app, usb
# for meshcore-cli over serial, version: "latest" or e.g. "1.16.0" — note
# heltec_v4 builds only exist from 1.10.0 on). Erases the whole flash in the
# same esptool session — the recommended path when switching from RNode, and
# one session avoids reconnecting to a port that re-enumerates after the
# erase.
[doc("Flash MeshCore companion firmware onto a Heltec")]
flash-meshcore serial="/dev/ttyACM0" board="heltec_v4" variant="ble" version="latest":
    #!/usr/bin/env bash
    set -euo pipefail
    board={{ quote(board) }}
    variant={{ quote(variant) }}
    version={{ quote(version) }}
    asset_re="^${board}_companion_radio_${variant}-.*-merged\.bin$"
    # gh (authenticated, 5000 req/h) when available; anonymous curl gets
    # rate-limited to 60 req/h per IP and 403s.
    if [ "$version" = latest ]; then
      releases='repos/meshcore-dev/MeshCore/releases?per_page=15'
      pick='[ .[] | select((.tag_name | startswith("companion-")) and (.prerelease | not)) ][0]'
    else
      releases="repos/meshcore-dev/MeshCore/releases/tags/companion-v${version}"
      pick=''
    fi
    if gh auth status >/dev/null 2>&1; then
      releases_json="$(gh api "$releases")" \
        || { echo "no companion-v${version} release on meshcore-dev/MeshCore"; exit 1; }
    else
      releases_json="$(curl -fsSL "https://api.github.com/$releases")" \
        || { echo "no companion-v${version} release on meshcore-dev/MeshCore"; exit 1; }
    fi
    url="$(printf '%s' "$releases_json" \
      | nix run nixpkgs#jq -- -r --arg re "$asset_re" \
          "$pick"'.assets[]? | select(.name | test($re; "i")) | .browser_download_url')"
    [ -n "$url" ] || { echo "no ${board} ${variant} merged .bin in MeshCore companion release ${version}"; exit 1; }
    echo ">> downloading $url"
    bindir="$(mktemp -d)"
    trap 'rm -rf "$bindir"' EXIT
    curl -fsSL "$url" -o "$bindir/meshcore.bin"
    if ! nix run nixpkgs#esptool -- --port {{ quote(serial) }} --baud 921600 \
        write-flash --erase-all 0x0 "$bindir/meshcore.bin"; then
      echo ""
      echo ">> esptool could not talk to the board. If it died with a serial/DTR"
      echo ">> error, the running firmware owns the USB port: put the chip in ROM"
      echo ">> download mode first — hold PRG/BOOT, tap RST, release PRG — then"
      echo ">> re-run this recipe. Press RST again after flashing."
      exit 1
    fi
    echo ">> MeshCore companion ($variant) flashed onto $board"

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
flash-near device="" ssid="nearby-town-radio-link-larp" password="" serial="/dev/ttyACM0" freq="869525000" sf="8":
    #!/usr/bin/env bash
    set -euo pipefail
    just={{ quote(just_executable()) }}
    ssid={{ quote(ssid) }}
    [ "${#ssid}" -le 32 ] || { echo "SSID must be at most 32 bytes (802.11 limit), got ${#ssid}: $ssid"; exit 1; }
    envdir="$(mktemp -d)"
    trap 'rm -rf "$envdir"' EXIT
    printf 'SSID=%s\nPASSWORD=%s\n' "$ssid" {{ quote(password) }} > "$envdir/wifi-ap.env"
    "$just" lora::lora-env "$envdir" {{ quote(serial) }} {{ quote(freq) }} {{ quote(sf) }}
    "$just" image::flash {{ quote(device) }} "$envdir"
    echo ">> relative-near card ready ({{ ssid }} + LoRa gateway)"

# The Riverside end: Aunt Anna's bot + mailbox + gateway. It also broadcasts
# its own (out-of-play) AP: mDNS between the gateway and the mailbox needs a
# live multicast interface, and the bubble doubles as facilitator debug
# access in the far location. So the network doesn't read as part of the
# game, the SSID defaults to a random id and the AP is password-protected
# (random unless given) — both are printed at the end; note them down.
[doc("Flash the relative-far card: bot + mailbox + LoRa gateway (+ protected debug AP)")]
flash-far character="relative" device="" ssid="" password="" serial="/dev/ttyACM0" freq="869525000" sf="8":
    #!/usr/bin/env bash
    set -euo pipefail
    just={{ quote(just_executable()) }}
    [ -f "secrets/{{ character }}-identity.toml" ] || { echo "no secrets/{{ character }}-identity.toml — run 'just characters::generate' first"; exit 1; }
    [ -f secrets/larp-cast.toml ] || { echo "no secrets/larp-cast.toml — run 'just characters::generate' first"; exit 1; }
    ssid={{ quote(ssid) }}
    [ -n "$ssid" ] || ssid="net-$(od -An -N4 -tx1 /dev/urandom | tr -d ' \n')"
    [ "${#ssid}" -le 32 ] || { echo "SSID must be at most 32 bytes (802.11 limit), got ${#ssid}: $ssid"; exit 1; }
    password={{ quote(password) }}
    [ -n "$password" ] || password="$(od -An -N8 -tx1 /dev/urandom | tr -d ' \n')"
    envdir="$(mktemp -d)"
    trap 'rm -rf "$envdir"' EXIT
    printf 'SSID=%s\nPASSWORD=%s\n' "$ssid" "$password" > "$envdir/wifi-ap.env"
    cp "secrets/{{ character }}-identity.toml" "$envdir/larp-identity.toml"
    cp secrets/larp-cast.toml "$envdir/larp-cast.toml"
    "$just" lora::lora-env "$envdir" {{ quote(serial) }} {{ quote(freq) }} {{ quote(sf) }}
    "$just" image::flash {{ quote(device) }} "$envdir"
    echo ">> relative-far card ready ({{ character }} bot + LoRa gateway)"
    echo ">> debug AP — SSID: $ssid  password: $password"

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
