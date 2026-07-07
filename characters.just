# Character provisioning (docs/design.md). Identities are private keys:
# secrets/ and stations/ are gitignored. Public halves go into larp-cast.toml.

# Generate one character's identity bundle (once per character, keep forever —
# re-generating invalidates the printed QR posters).
[doc("Generate a character's identity bundle (keep forever: re-generating invalidates the printed posters)")]
keygen character:
    #!/usr/bin/env bash
    set -euo pipefail
    mkdir -p secrets
    [ ! -f "secrets/{{ character }}-identity.toml" ] || { echo "secrets/{{ character }}-identity.toml already exists — refusing to overwrite"; exit 1; }
    nix run .#larp-bot --accept-flake-config -- keygen --character {{ character }} --out "secrets/{{ character }}-identity.toml"

# Assemble the public cast file from every generated identity.
cast:
    #!/usr/bin/env bash
    set -euo pipefail
    ids=(secrets/*-identity.toml)
    [ -f "${ids[0]}" ] || { echo "no identities in secrets/ — run 'just characters::keygen <character>' first"; exit 1; }
    args=(); for f in "${ids[@]}"; do args+=(--identity "$f"); done
    nix run .#larp-bot --accept-flake-config -- cast "${args[@]}" --out secrets/larp-cast.toml

# Render the QR wall posters (PNG per character) into secrets/.
posters:
    #!/usr/bin/env bash
    set -euo pipefail
    for f in secrets/*-identity.toml; do
      char="$(basename "$f" -identity.toml)"
      nix run .#larp-bot --accept-flake-config -- qr --identity "$f" --out "secrets/$char-qr.png"
    done

# Assemble a station's env dir for flashing: wifi-ap.env + identity + cast.
station character:
    #!/usr/bin/env bash
    set -euo pipefail
    [ -f "secrets/{{ character }}-identity.toml" ] || { echo "run 'just characters::keygen {{ character }}' first"; exit 1; }
    [ -f secrets/larp-cast.toml ] || { echo "run 'just characters::cast' first"; exit 1; }
    mkdir -p "stations/{{ character }}"
    printf 'SSID=larp-%s\nPASSWORD=dashchat\n' "{{ character }}" > "stations/{{ character }}/wifi-ap.env"
    cp "secrets/{{ character }}-identity.toml" "stations/{{ character }}/larp-identity.toml"
    cp secrets/larp-cast.toml "stations/{{ character }}/larp-cast.toml"
    echo ">> stations/{{ character }}/ ready — flash with: just image::flash /dev/sdX stations/{{ character }}"

# Run a character bot locally against a mailbox — the journalist against the
# cloud mailbox (no droplet needed for testing), or any character while
# developing. State lives in .run/<character>/ (wipe it to simulate a reset).
[doc("Run a character bot locally against a mailbox (journalist: no droplet needed); state in .run/<character>/")]
run character mailbox_url="https://mailbox.production.darksoil.studio":
    #!/usr/bin/env bash
    set -euo pipefail
    [ -f "secrets/{{ character }}-identity.toml" ] || { echo "run 'just characters::keygen {{ character }}' first"; exit 1; }
    [ -f secrets/larp-cast.toml ] || { echo "run 'just characters::cast' first"; exit 1; }
    mkdir -p ".run/{{ character }}"
    {
      printf 'mailbox_url = "%s"\n' "{{ mailbox_url }}"
      printf 'identity = "%s/secrets/{{ character }}-identity.toml"\n' "$PWD"
      printf 'cast = "%s/secrets/larp-cast.toml"\n' "$PWD"
      printf 'scenarios_dir = "%s/scenarios"\n' "$PWD"
      printf 'data_dir = "%s/.run/{{ character }}/data"\n' "$PWD"
    } > ".run/{{ character }}/config.toml"
    exec nix run .#larp-bot --accept-flake-config -- run --config ".run/{{ character }}/config.toml"
