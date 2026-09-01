# Character provisioning (docs/design.md). Identities are private keys:
# secrets/ is gitignored. Public halves go into larp-cast.toml.

# The one character whose card carries the informant: the character who hands
# him out (`informant_tip` in scenarios/<character>.toml — a linted pairing).
# His bot runs there and nowhere else, so change both together.
informant_character := "sister"

# Generate everything: one identity bundle per scenario pack (existing
# bundles are kept — re-generating one would invalidate its printed poster)
# plus the public cast file assembled from all of them. Idempotent; the cast
# is complete by construction, since the character list IS scenarios/*.toml —
# plus the two spec-bot characters, "mayor" and "anonymous", who have no
# scenario pack and stay OUT of the cast (the family doesn't know the
# informant exists, and nothing is ever addressed to the mayor — his opening
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
    for char in mayor anonymous; do
      if [ -f "secrets/$char-identity.toml" ]; then
        echo ">> $char: identity exists, keeping it"
      else
        "$just" characters::keygen "$char"
      fi
    done

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
# the cast is built from those and nothing else: anonymous (no scenario pack)
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
#
# The informant deliberately gets NO poster: he is met only through the deep
# link Mira hands out after a player carries something to her
# (scenarios/sister.toml). Printing one would put the side plot on the wall
# next to everything else and cost it its whole secrecy.
[doc("Render the QR wall posters (PNG per character) into secrets/ — five characters + the mayor; the informant has none on purpose")]
posters:
    #!/usr/bin/env bash
    set -euo pipefail
    ids=(secrets/*-identity.toml)
    [ -f "${ids[0]}" ] || { echo "no identities in secrets/ — run 'just characters::generate' first"; exit 1; }
    for f in "${ids[@]}"; do
      char="$(basename "$f" -identity.toml)"
      if [ "$char" = "anonymous" ]; then
        echo ">> anonymous: no poster (players reach him through Mira's link)"
        continue
      fi
      nix run .#larp-bot --accept-flake-config -- qr --identity "$f" --out "secrets/$char-qr.png"
    done

# Run a character bot locally against a mailbox, while developing. State lives
# in .run/<character>/ (wipe it to simulate a reset). The informant identity is
# passed along when secrets/ has one, so Mira's tip carries a real deep link.
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
      # No informant identity in secrets/ is fine: the bot logs it and never
      # tips. An `if` (not `&&`) so the group still exits 0 when it's absent.
      if [ -f secrets/anonymous-identity.toml ]; then
        printf 'anonymous_identity = "%s/secrets/anonymous-identity.toml"\n' "$PWD"
      fi
    } > ".run/{{ character }}/config.toml"
    exec nix run .#larp-bot --accept-flake-config -- run --config ".run/{{ character }}/config.toml"

# Run a spec bot locally against a mailbox — the mayor (onboarding + the
# endgame trigger) or the anonymous informant. These two have no scenario
# pack: their whole script is <character>.toml in the repo root. State lives
# in .run/<character>/ (wipe it to simulate a reset).
[doc("Run a spec bot (mayor | anonymous) locally; state in .run/<character>/")]
run-spec character mailbox_url="https://mailbox.production.darksoil.studio":
    #!/usr/bin/env bash
    set -euo pipefail
    case {{ quote(character) }} in
      mayor|anonymous) ;;
      *) echo "run-spec takes 'mayor' or 'anonymous' (the characters with no scenario pack)"; exit 1 ;;
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
#
# The anonymous informant rides on MIRA'S CARD ALONE (informant_character,
# top of this file). Hers is the card that needs his identity twice over: it arms the
# dormant larp-bot-anonymous service, and it is what her bot reads to build
# the add-contact deep link in her informant_tip. He has no poster, and she
# is the only one who hands him out, so no other station has any use for him
# — and like the mayor he now exists exactly once, where the side plot opens
# (see anonymous.toml).
#
# The catch to know: a tapped link is only answered where he runs. A player
# who taps it at Mira's station gets him within seconds; one who taps it and
# walks off carries the contact request in their pocket until they are back
# in her bubble. Her tip line says to do it there.
[doc("Flash a character's station card; the Pi joins wifi <ssid> (open unless a password is given); auto-detects the SD card; the sister's card also runs the anonymous informant")]
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
    if [ "{{ character }}" = "{{ informant_character }}" ]; then
      [ -f secrets/anonymous-identity.toml ] || { echo "no secrets/anonymous-identity.toml — run 'just characters::generate' first"; exit 1; }
    fi
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
    informant=""
    if [ "{{ character }}" = "{{ informant_character }}" ]; then
      cp secrets/anonymous-identity.toml "$envdir/larp-anonymous.toml"
      informant=" + the anonymous informant"
    fi
    "$just" image::flash {{ quote(device) }} "$envdir"
    echo ">> card is ready: joins wifi '$ssid'$([ -z "$password" ] && echo ' (open network)' || true) on boot, runs {{ character }}$informant"
