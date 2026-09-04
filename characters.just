# Character provisioning (docs/design.md). Identities are private keys:
# secrets/ is gitignored. Public halves go into larp-cast.toml.

# Generate everything: one identity bundle per scenario pack (existing
# bundles are kept — re-generating one would invalidate its printed poster)
# plus the public cast file assembled from all of them. Idempotent; the cast
# is complete by construction, since the character list IS scenarios/*.toml —
# plus the spec-bot character, "mayor", who has no scenario pack and stays
# OUT of the cast (nothing is ever addressed to the mayor — his opening
# notice is a sender-only mission in mayor.toml).
[doc("Generate all missing character identities + the cast file (idempotent)")]
generate: && cast
    #!/usr/bin/env bash
    set -euo pipefail
    just={{ quote(just_executable()) }}
    for f in scenarios/*.toml; do
      char="$(basename "$f" .toml)"
      if [ -f "secrets/$char-identity.toml" ]; then
        echo ">> $char: identity exists, keeping it"
      else
        "$just" characters::keygen "$char"
      fi
    done
    if [ -f secrets/mayor-identity.toml ]; then
      echo ">> mayor: identity exists, keeping it"
    else
      "$just" characters::keygen mayor
    fi

# Generate one character's identity bundle (kept forever — re-generating
# would invalidate the printed QR posters, hence the overwrite guard).
[private]
keygen character:
    #!/usr/bin/env bash
    set -euo pipefail
    mkdir -p secrets
    [ ! -f "secrets/{{ character }}-identity.toml" ] || { echo "secrets/{{ character }}-identity.toml already exists — refusing to overwrite"; exit 1; }
    nix run .#larp-bot --accept-flake-config -- keygen --character {{ character }} --out "secrets/{{ character }}-identity.toml"

# Assemble the public cast file. The character list IS scenarios/*.toml, so
# the cast is built from those and nothing else: the mayor (no scenario pack)
# never appears in the cast the stations receive, and identities left in
# secrets/ from a retired character are ignored rather than resurrected.
[private]
cast:
    #!/usr/bin/env bash
    set -euo pipefail
    args=()
    for f in scenarios/*.toml; do
      char="$(basename "$f" .toml)"
      [ -f "secrets/$char-identity.toml" ] || { echo "no secrets/$char-identity.toml — run 'just characters::generate' first"; exit 1; }
      args+=(--identity "secrets/$char-identity.toml")
    done
    [ "${#args[@]}" -gt 0 ] || { echo "no scenario packs in scenarios/"; exit 1; }
    nix run .#larp-bot --accept-flake-config -- cast "${args[@]}" --out secrets/larp-cast.toml

# Render the QR wall posters (PNG per character) into secrets/. Six of them,
# all on the base-station wall: the five characters (family + Nadia) and the
# mayor (his is the one players scan FIRST — the situation; Nadia's greeting
# is the tutorial that follows).
[doc("Render the QR wall posters (PNG per character) into secrets/ — five characters + the mayor")]
posters:
    #!/usr/bin/env bash
    set -euo pipefail
    ids=(secrets/*-identity.toml)
    [ -f "${ids[0]}" ] || { echo "no identities in secrets/ — run 'just characters::generate' first"; exit 1; }
    for f in "${ids[@]}"; do
      char="$(basename "$f" -identity.toml)"
      nix run .#larp-bot --accept-flake-config -- qr --identity "$f" --out "secrets/$char-qr.png"
    done

# Run a character bot locally against a mailbox, while developing. State lives
# in .run/<character>/ (wipe it to simulate a reset).
[doc("Run a character bot locally against a mailbox; state in .run/<character>/")]
run character mailbox_url="https://mailbox.production.darksoil.studio":
    #!/usr/bin/env bash
    set -euo pipefail
    [ -f "secrets/{{ character }}-identity.toml" ] || { echo "no secrets/{{ character }}-identity.toml — run 'just characters::generate' first"; exit 1; }
    [ -f secrets/larp-cast.toml ] || { echo "no secrets/larp-cast.toml — run 'just characters::generate' first"; exit 1; }
    mkdir -p ".run/{{ character }}"
    {
      printf 'mailbox_url = "%s"\n' "{{ mailbox_url }}"
      printf 'identity = "%s/secrets/{{ character }}-identity.toml"\n' "$PWD"
      printf 'cast = "%s/secrets/larp-cast.toml"\n' "$PWD"
      printf 'scenarios_dir = "%s/scenarios"\n' "$PWD"
      printf 'data_dir = "%s/.run/{{ character }}/data"\n' "$PWD"
      # Where a locally-run mayor (just characters::run-spec mayor) drops his
      # collapse flag — lets Nadia's eruption be tested on a laptop.
      printf 'mayor_fallen_flag = "%s/.run/mayor/data/triggered"\n' "$PWD"
    } > ".run/{{ character }}/config.toml"
    exec nix run .#larp-bot --accept-flake-config -- run --config ".run/{{ character }}/config.toml"

# Run a spec bot locally against a mailbox — the mayor (the official notice
# + the endgame trigger). He has no scenario pack: his whole script is
# mayor.toml in the repo root. State lives in .run/<character>/ (wipe it to
# simulate a reset).
[doc("Run a spec bot (mayor) locally; state in .run/<character>/")]
run-spec character mailbox_url="https://mailbox.production.darksoil.studio":
    #!/usr/bin/env bash
    set -euo pipefail
    case {{ quote(character) }} in
      mayor) ;;
      *) echo "run-spec takes 'mayor' (the character with no scenario pack)"; exit 1 ;;
    esac
    [ -f "secrets/{{ character }}-identity.toml" ] || { echo "no secrets/{{ character }}-identity.toml — run 'just characters::generate' first"; exit 1; }
    mkdir -p ".run/{{ character }}"
    {
      printf 'mailbox_url = "%s"\n' "{{ mailbox_url }}"
      printf 'identity = "%s/secrets/{{ character }}-identity.toml"\n' "$PWD"
      printf 'spec = "%s/{{ character }}.toml"\n' "$PWD"
      printf 'avatar = "%s/{{ character }}.png"\n' "$PWD"
      printf 'data_dir = "%s/.run/{{ character }}/data"\n' "$PWD"
    } > ".run/{{ character }}/config.toml"
    exec nix run .#larp-bot --accept-flake-config -- spec --config ".run/{{ character }}/config.toml"

# Flash a character's station card: the station image plus the character's
# files (wifi.env, identity, cast — assembled on the fly from secrets/) on
# the FAT boot partition. With no device, the SD card is auto-detected (see
# image::flash).
#
# The Pi does not host wifi: the mailbox image dropped AP mode (no hostapd,
# nothing reads wifi-ap.env any more), so every station JOINS a network that
# something else broadcasts — a router per station, all named OfflineWifi and
# open, so a player's phone re-joins by itself at every stop. wifi.env is read
# at boot by the image's wifi-client service, so the network can also be
# changed later by editing it on the card. Pass a password (8-63 chars, WPA2)
# for a locked network.
#
# Every character gets a card, Mira included: she is in town now, on the desk
# at the school shelter, not on the cloud droplet (sister.just is kept but
# unused — see its header).
[doc("Flash a character's station card; the Pi joins wifi <ssid> (open unless a password is given); auto-detects the SD card")]
flash character device="" ssid="OfflineWifi" password="":
    #!/usr/bin/env bash
    set -euo pipefail
    just={{ quote(just_executable()) }}
    ssid={{ quote(ssid) }}
    password={{ quote(password) }}
    # Nadia's bot lives on the base-station card, next to the mayor. A second
    # card with her identity would be a duplicate instance of her.
    if [ "{{ character }}" = "neighbour" ]; then
      echo "the neighbour lives on the base-station card — use 'just base-station::flash'"; exit 1
    fi
    [ -f "secrets/{{ character }}-identity.toml" ] || { echo "no secrets/{{ character }}-identity.toml — run 'just characters::generate' first"; exit 1; }
    [ -f secrets/larp-cast.toml ] || { echo "no secrets/larp-cast.toml — run 'just characters::generate' first"; exit 1; }
    if [ -n "$password" ] && { [ "${#password}" -lt 8 ] || [ "${#password}" -gt 63 ]; }; then
      echo "wifi password must be 8-63 characters (WPA2), got ${#password}"; exit 1
    fi
    if [ "${#ssid}" -gt 32 ]; then
      echo "SSID must be at most 32 bytes (802.11 limit), got ${#ssid}: $ssid"; exit 1
    fi
    envdir="$(mktemp -d)"
    trap 'rm -rf "$envdir"' EXIT
    # An empty WIFI_PASSWORD= line means an open network (see the mailbox
    # image's nix/wifi-client.nix).
    printf 'WIFI_SSID=%s\nWIFI_PASSWORD=%s\n' "$ssid" "$password" > "$envdir/wifi.env"
    cp "secrets/{{ character }}-identity.toml" "$envdir/larp-identity.toml"
    cp secrets/larp-cast.toml "$envdir/larp-cast.toml"
    "$just" image::flash {{ quote(device) }} "$envdir"
    echo ">> card is ready: joins wifi '$ssid'$([ -z "$password" ] && echo ' (open network)' || true) on boot, runs {{ character }}"
