# Provisioning + flashing recipes for the internet-shutdown LARP.
# Run inside `nix develop` (or prefix with `nix run nixpkgs#just --`).

# The flashable image and the default env dir copied onto the FAT boot
# partition. Per-station: just env_dir=stations/<character> flash /dev/sdX
image := "larp-station.img"
env_dir := "env"

# Show available recipes.
_default:
    @just --list

# Build the station SD image and decompress it for flashing (aarch64 build;
# on x86_64 this needs `boot.binfmt.emulatedSystems = [ "aarch64-linux" ]`).
build:
    #!/usr/bin/env bash
    set -euo pipefail
    nix build .#sdImage -L --accept-flake-config
    zst="$(echo result/sd-image/*.img.zst)"
    [ -f "$zst" ] || { echo "no *.img.zst under result/sd-image/ — did the build succeed?"; exit 1; }
    echo ">> decompressing $zst -> {{image}}"
    rm -f "{{image}}"
    zstd -d "$zst" -o "{{image}}"
    ls -lh "{{image}}"

# List candidate block devices, to pick the SD-card target for `flash`.
devices:
    lsblk -do NAME,SIZE,TYPE,TRAN,VENDOR,MODEL,RM

# Flash the image to an SD card and copy env_dir/* onto its FAT boot partition.
# Usage: just env_dir=stations/firefighters flash /dev/sdX
flash device:
    #!/usr/bin/env bash
    set -euo pipefail
    img="{{image}}"; dev="{{device}}"; envdir="{{env_dir}}"

    [ -f "$img" ] || { echo "image '$img' not found — run 'just build' first"; exit 1; }
    [ -b "$dev" ] || { echo "'$dev' is not a block device; run 'just devices'"; exit 1; }
    [ "$(lsblk -dno TYPE "$dev")" = "disk" ] || { echo "'$dev' is not a whole disk"; exit 1; }

    echo "This will ERASE and overwrite:"
    lsblk -o NAME,SIZE,TYPE,TRAN,VENDOR,MODEL,MOUNTPOINTS "$dev"
    read -rp "Retype the device path to confirm ($dev): " ok
    [ "$ok" = "$dev" ] || { echo "no match; aborting"; exit 1; }

    for p in $(lsblk -rno NAME "$dev" | tail -n +2); do sudo umount "/dev/$p" 2>/dev/null || true; done

    echo ">> flashing $img -> $dev"
    sudo dd if="$img" of="$dev" bs=4M conv=fsync status=progress
    sync
    sudo partprobe "$dev" 2>/dev/null || true
    sudo udevadm settle 2>/dev/null || true

    boot=""
    for _ in $(seq 10); do
      boot="$(lsblk -rno NAME,FSTYPE "$dev" | awk '$2=="vfat"{print "/dev/"$1; exit}')"
      [ -n "$boot" ] && break
      sudo partprobe "$dev" 2>/dev/null || true; sudo udevadm settle 2>/dev/null || true; sleep 1
    done
    if [ -z "$boot" ]; then boot="${dev}1"; [ -b "$boot" ] || boot="${dev}p1"; fi
    [ -b "$boot" ] || { echo "could not find the FAT boot partition on $dev"; exit 1; }

    mnt="$(mktemp -d)"
    trap 'sudo umount "$mnt" 2>/dev/null || true; rmdir "$mnt" 2>/dev/null || true' EXIT
    sudo mount "$boot" "$mnt"
    shopt -s nullglob
    files=("$envdir"/*)
    if [ "${#files[@]}" -eq 0 ]; then
      echo ">> note: '$envdir/' is empty — nothing to copy"
    else
      echo ">> copying ${#files[@]} file(s) from $envdir/ to $boot"
      for f in "${files[@]}"; do [ -f "$f" ] && sudo cp -v "$f" "$mnt/"; done
      sync
    fi
    echo ">> done — $dev is ready to boot"

# ---------------------------------------------------------------------------
# Character provisioning (docs/design.md). Identities are private keys:
# secrets/ and stations/ are gitignored. Public halves go into larp-cast.toml.

# Generate one character's identity bundle (once per character, keep forever —
# re-generating invalidates the printed QR posters).
larp-keygen character:
    #!/usr/bin/env bash
    set -euo pipefail
    mkdir -p secrets
    [ ! -f "secrets/{{character}}-identity.toml" ] || { echo "secrets/{{character}}-identity.toml already exists — refusing to overwrite"; exit 1; }
    nix run .#larp-bot --accept-flake-config -- keygen --character {{character}} --out "secrets/{{character}}-identity.toml"

# Assemble the public cast file from every generated identity.
larp-cast:
    #!/usr/bin/env bash
    set -euo pipefail
    ids=(secrets/*-identity.toml)
    [ -f "${ids[0]}" ] || { echo "no identities in secrets/ — run 'just larp-keygen <character>' first"; exit 1; }
    args=(); for f in "${ids[@]}"; do args+=(--identity "$f"); done
    nix run .#larp-bot --accept-flake-config -- cast "${args[@]}" --out secrets/larp-cast.toml

# Render the QR wall posters (PNG per character) into secrets/.
larp-posters:
    #!/usr/bin/env bash
    set -euo pipefail
    for f in secrets/*-identity.toml; do
      char="$(basename "$f" -identity.toml)"
      nix run .#larp-bot --accept-flake-config -- qr --identity "$f" --out "secrets/$char-qr.png"
    done

# Assemble a station's env dir for flashing: wifi-ap.env + identity + cast.
# Flash with: just env_dir=stations/<character> flash /dev/sdX
larp-station character:
    #!/usr/bin/env bash
    set -euo pipefail
    [ -f "secrets/{{character}}-identity.toml" ] || { echo "run 'just larp-keygen {{character}}' first"; exit 1; }
    [ -f secrets/larp-cast.toml ] || { echo "run 'just larp-cast' first"; exit 1; }
    mkdir -p "stations/{{character}}"
    printf 'SSID=larp-%s\nPASSWORD=dashchat\n' "{{character}}" > "stations/{{character}}/wifi-ap.env"
    cp "secrets/{{character}}-identity.toml" "stations/{{character}}/larp-identity.toml"
    cp secrets/larp-cast.toml "stations/{{character}}/larp-cast.toml"
    echo ">> stations/{{character}}/ ready — flash with: just env_dir=stations/{{character}} flash /dev/sdX"

# Run all tests (unit + e2e).
test:
    cargo test --workspace
