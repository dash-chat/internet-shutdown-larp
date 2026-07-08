# Building + flashing the station SD images.

# Build the station SD image and decompress it for flashing (aarch64 build;
# on x86_64 this needs `boot.binfmt.emulatedSystems = [ "aarch64-linux" ]`).
[doc("Build the station SD image (aarch64; x86_64 builders need binfmt emulation)")]
build: (build-image "sdImage" "larp-station.img")

# Build the base-station SD image: the station image with the mayor portal
# in place of the generic captive-portal SPA. The Pi hosts its own wifi like
# the other stations. Flash with `just base-station::flash /dev/sdX`, which
# writes the wifi-ap.env.
[doc("Build the base-station SD image (mayor portal; flash with base-station::flash)")]
build-base-station: (build-image "sdImage-base-station" "base-station.img")

[private]
build-image attr out:
    #!/usr/bin/env bash
    set -euo pipefail
    nix build .#{{ attr }} -L --accept-flake-config
    zst="$(echo result/sd-image/*.img.zst)"
    [ -f "$zst" ] || { echo "no *.img.zst under result/sd-image/ — did the build succeed?"; exit 1; }
    echo ">> decompressing $zst -> {{ out }}"
    rm -f "{{ out }}"
    zstd -d "$zst" -o "{{ out }}"
    ls -lh "{{ out }}"

# List candidate block devices, to pick the SD-card target for `flash`.
devices:
    lsblk -do NAME,SIZE,TYPE,TRAN,VENDOR,MODEL,RM

# Flash an image to an SD card, copying env_dir/* (if given) onto its FAT
# boot partition — via the mailbox image's flash-sd-image helper, which
# auto-detects the SD card when no device is given (the single removable/
# USB disk that isn't the system disk; ambiguity aborts) and asks to retype
# the device path before erasing. For character stations use
# `just characters::flash`; the base station card via `just base-station::flash`.
[doc("Flash an image, copying env_dir/* (if given) onto the boot partition; auto-detects the SD card")]
flash device="" env_dir="" img="larp-station.img":
    #!/usr/bin/env bash
    set -euo pipefail
    [ -f "{{ img }}" ] || { echo "image '{{ img }}' not found — run 'just image::build' (or build-base-station) first"; exit 1; }
    nix run .#flash-sd-image --accept-flake-config -- "{{ img }}" "{{ device }}" "{{ env_dir }}"
