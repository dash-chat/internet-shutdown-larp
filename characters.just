# Character provisioning (docs/design.md). Identities are private keys:
# secrets/ is gitignored. Public halves go into larp-cast.toml.

# Generate everything: one identity bundle per scenario pack (existing
# bundles are kept — re-generating one would invalidate its printed poster)
# plus the public cast file assembled from all of them. Idempotent; the cast
# is complete by construction, since the character list IS scenarios/*.toml —
# plus the two spec-bot characters, "mayor" and "anonymous", who have no
# scenario pack and stay OUT of the cast (the family doesn't know the
# informant exists, and the mayor trades no missions).
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

# Render the QR wall posters (PNG per character) into secrets/. Seven of them:
# the five family posters and the mayor's go on the base-station wall (his is
# the one players scan FIRST — his greeting is the onboarding), and the
# informant's is hidden somewhere on the map instead.
[doc("Render the QR wall posters (PNG per character) into secrets/ — five family, the mayor, the hidden informant")]
posters:
    #!/usr/bin/env bash
    set -euo pipefail
    ids=(secrets/*-identity.toml)
    [ -f "${ids[0]}" ] || { echo "no identities in secrets/ — run 'just characters::generate' first"; exit 1; }
    for f in "${ids[@]}"; do
      char="$(basename "$f" -identity.toml)"
      nix run .#larp-bot --accept-flake-config -- qr --identity "$f" --out "secrets/$char-qr.png"
    done

# Run a character bot locally against a mailbox — the sister against the
# cloud mailbox (no droplet needed for testing), or any character while
# developing. State lives in .run/<character>/ (wipe it to simulate a reset).
[doc("Run a character bot locally against a mailbox (sister: no droplet needed); state in .run/<character>/")]
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
# files (wifi-ap.env with SSID=<character><ssid_suffix>, identity, cast —
# assembled on the fly from secrets/) on the FAT boot partition. The AP is
# OPEN by default; pass a password (8-63 chars, WPA2) to lock it. With no
# device, the SD card is auto-detected (see image::flash).
#
# Only the in-town characters get a card: the sister is outside town and runs
# on the cloud droplet instead (just sister::deploy).
#
# The anonymous informant rides along on EVERY station: each card gets the
# shared anonymous identity, which arms the dormant larp-bot-anonymous
# service. Whichever station a player's contact request reaches first
# answers with the whole secret (see anonymous.toml).
[doc("Flash a character's station card; AP is <character><ssid_suffix> (open unless password given); auto-detects the SD card; every card also runs the anonymous informant")]
flash character device="" ssid_suffix="-larp" password="":
    #!/usr/bin/env bash
    set -euo pipefail
    just={{ quote(just_executable()) }}
    ssid={{ quote(character) }}{{ quote(ssid_suffix) }}
    password={{ quote(password) }}
    [ -f "secrets/{{ character }}-identity.toml" ] || { echo "no secrets/{{ character }}-identity.toml — run 'just characters::generate' first"; exit 1; }
    [ -f secrets/larp-cast.toml ] || { echo "no secrets/larp-cast.toml — run 'just characters::generate' first"; exit 1; }
    [ -f secrets/anonymous-identity.toml ] || { echo "no secrets/anonymous-identity.toml — run 'just characters::generate' first"; exit 1; }
    if [ -n "$password" ] && { [ "${#password}" -lt 8 ] || [ "${#password}" -gt 63 ]; }; then
      echo "wifi password must be 8-63 characters (WPA2), got ${#password}"; exit 1
    fi
    if [ "${#ssid}" -gt 32 ]; then
      echo "SSID must be at most 32 bytes (802.11 limit; hostapd refuses longer), got ${#ssid}: $ssid"; exit 1
    fi
    envdir="$(mktemp -d)"
    trap 'rm -rf "$envdir"' EXIT
    # An empty PASSWORD= line means an open network (see the mailbox image's
    # wifi-ap.env semantics).
    printf 'SSID=%s\nPASSWORD=%s\n' "$ssid" "$password" > "$envdir/wifi-ap.env"
    cp "secrets/{{ character }}-identity.toml" "$envdir/larp-identity.toml"
    cp secrets/larp-cast.toml "$envdir/larp-cast.toml"
    cp secrets/anonymous-identity.toml "$envdir/larp-anonymous.toml"
    "$just" image::flash {{ quote(device) }} "$envdir"
    echo ">> card is ready to boot as $ssid$([ -z "$password" ] && echo ' (open network)' || true) + anonymous informant"
