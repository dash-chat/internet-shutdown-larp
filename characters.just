# Character provisioning (docs/design.md). Identities are private keys:
# secrets/ is gitignored. Public halves go into larp-cast.toml.

# Generate everything: one identity bundle per scenario pack (existing
# bundles are kept — re-generating one would invalidate its printed poster)
# plus the public cast file assembled from all of them. Idempotent; the cast
# is complete by construction, since the character list IS scenarios/*.toml —
# plus the secret "anonymous" character, who has no scenario pack and stays
# OUT of the cast (no other character knows he exists).
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
    if [ -f "secrets/anonymous-identity.toml" ]; then
      echo ">> anonymous: identity exists, keeping it"
    else
      "$just" characters::keygen anonymous
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

# Assemble the public cast file from every generated identity — except
# anonymous, who must never appear in the cast the stations receive.
[private]
cast:
    #!/usr/bin/env bash
    set -euo pipefail
    ids=(secrets/*-identity.toml)
    [ -f "${ids[0]}" ] || { echo "no identities in secrets/ — run 'just characters::generate' first"; exit 1; }
    args=()
    for f in "${ids[@]}"; do
      [ "$(basename "$f")" = anonymous-identity.toml ] && continue
      args+=(--identity "$f")
    done
    nix run .#larp-bot --accept-flake-config -- cast "${args[@]}" --out secrets/larp-cast.toml

# Render the QR wall posters (PNG per character) into secrets/.
posters:
    #!/usr/bin/env bash
    set -euo pipefail
    ids=(secrets/*-identity.toml)
    [ -f "${ids[0]}" ] || { echo "no identities in secrets/ — run 'just characters::generate' first"; exit 1; }
    for f in "${ids[@]}"; do
      char="$(basename "$f" -identity.toml)"
      nix run .#larp-bot --accept-flake-config -- qr --identity "$f" --out "secrets/$char-qr.png"
    done

# Run a character bot locally against a mailbox — the journalist against the
# cloud mailbox (no droplet needed for testing), or any character while
# developing. State lives in .run/<character>/ (wipe it to simulate a reset).
[doc("Run a character bot locally against a mailbox (journalist: no droplet needed); state in .run/<character>/")]
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

# Run the anonymous informant locally against a mailbox, with one of the
# variants from anonymous.toml ("portal" or "code"). State lives in
# .run/anonymous-<variant>/ (wipe it to simulate a reset).
[doc("Run the anonymous informant locally (variant: portal|code); state in .run/anonymous-<variant>/")]
run-anonymous variant mailbox_url="https://mailbox.production.darksoil.studio":
    #!/usr/bin/env bash
    set -euo pipefail
    [ -f secrets/anonymous-identity.toml ] || { echo "no secrets/anonymous-identity.toml — run 'just characters::generate' first"; exit 1; }
    grep -q "^{{ variant }} = \[" anonymous.toml || { echo "unknown anonymous variant '{{ variant }}' — see [variants] in anonymous.toml"; exit 1; }
    mkdir -p ".run/anonymous-{{ variant }}"
    { cat secrets/anonymous-identity.toml; printf 'variant = "%s"\n' "{{ variant }}"; } > ".run/anonymous-{{ variant }}/identity.toml"
    {
      printf 'mailbox_url = "%s"\n' "{{ mailbox_url }}"
      printf 'identity = "%s/.run/anonymous-{{ variant }}/identity.toml"\n' "$PWD"
      printf 'spec = "%s/anonymous.toml"\n' "$PWD"
      printf 'data_dir = "%s/.run/anonymous-{{ variant }}/data"\n' "$PWD"
    } > ".run/anonymous-{{ variant }}/config.toml"
    exec nix run .#larp-bot --accept-flake-config -- anonymous --config ".run/anonymous-{{ variant }}/config.toml"

# Flash a character's station card: the station image plus the character's
# files (wifi-ap.env with SSID=<character><ssid_suffix>, identity, cast —
# assembled on the fly from secrets/) on the FAT boot partition. The AP is
# OPEN by default; pass a password (8-63 chars, WPA2) to lock it. With no
# device, the SD card is auto-detected (see image::flash).
#
# The anonymous informant rides along on a fixed pair of stations, one per
# side of the fire line: the firefighters card always gets the "portal"
# variant and the hospital card always gets the "code" variant (see
# anonymous.toml). Those cards get the shared anonymous identity with the
# variant appended, which arms the dormant larp-bot-anonymous service; other
# characters' cards never do.
[doc("Flash a character's station card; AP is <character><ssid_suffix> (open unless password given); auto-detects the SD card; firefighters/hospital also run the anonymous informant (portal/code)")]
flash character device="" ssid_suffix="-larp" password="":
    #!/usr/bin/env bash
    set -euo pipefail
    just={{ quote(just_executable()) }}
    ssid={{ quote(character) }}{{ quote(ssid_suffix) }}
    password={{ quote(password) }}
    case {{ quote(character) }} in
      firefighters) anonymous=portal ;;
      hospital) anonymous=code ;;
      *) anonymous="" ;;
    esac
    [ -f "secrets/{{ character }}-identity.toml" ] || { echo "no secrets/{{ character }}-identity.toml — run 'just characters::generate' first"; exit 1; }
    [ -f secrets/larp-cast.toml ] || { echo "no secrets/larp-cast.toml — run 'just characters::generate' first"; exit 1; }
    if [ -n "$password" ] && { [ "${#password}" -lt 8 ] || [ "${#password}" -gt 63 ]; }; then
      echo "wifi password must be 8-63 characters (WPA2), got ${#password}"; exit 1
    fi
    if [ "${#ssid}" -gt 32 ]; then
      echo "SSID must be at most 32 bytes (802.11 limit; hostapd refuses longer), got ${#ssid}: $ssid"; exit 1
    fi
    if [ -n "$anonymous" ]; then
      [ -f secrets/anonymous-identity.toml ] || { echo "no secrets/anonymous-identity.toml — run 'just characters::generate' first"; exit 1; }
      grep -q "^$anonymous = \[" anonymous.toml || { echo "unknown anonymous variant '$anonymous' — see [variants] in anonymous.toml"; exit 1; }
    fi
    envdir="$(mktemp -d)"
    trap 'rm -rf "$envdir"' EXIT
    # An empty PASSWORD= line means an open network (see the mailbox image's
    # wifi-ap.env semantics).
    printf 'SSID=%s\nPASSWORD=%s\n' "$ssid" "$password" > "$envdir/wifi-ap.env"
    cp "secrets/{{ character }}-identity.toml" "$envdir/larp-identity.toml"
    cp secrets/larp-cast.toml "$envdir/larp-cast.toml"
    if [ -n "$anonymous" ]; then
      # The bundle plus the one flash-time field (the bundle toml is flat
      # key-value, so appending a line keeps it valid).
      { cat secrets/anonymous-identity.toml; printf 'variant = "%s"\n' "$anonymous"; } > "$envdir/larp-anonymous.toml"
    fi
    "$just" image::flash {{ quote(device) }} "$envdir"
    echo ">> card is ready to boot as $ssid$([ -z "$password" ] && echo ' (open network)' || true)$([ -n "$anonymous" ] && echo " + anonymous informant ($anonymous)" || true)"
