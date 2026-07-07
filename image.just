# Building + flashing the station SD images.

# Build the station SD image and decompress it for flashing (aarch64 build;
# on x86_64 this needs `boot.binfmt.emulatedSystems = [ "aarch64-linux" ]`).
[doc("Build the station SD image (aarch64; x86_64 builders need binfmt emulation)")]
build: (build-image "sdImage" "larp-station.img")

# Build the base-station SD image: the mAP lite is the AP, the Pi sits wired
# behind it (nix/base-station.nix; mAP side: `just base-station::provision`).
# Flash with `just image::flash /dev/sdX "" base-station.img` — no env dir:
# the base station hosts no Pi wifi (and the image ignores wifi-ap.env).
[doc("Build the base-station SD image (mAP lite is the AP; flash with no env dir)")]
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

# Flash an image to an SD card and copy env_dir/* onto its FAT boot
# partition (empty env_dir = copy nothing).
# Stations:     just image::flash /dev/sdX stations/firefighters
# Base station: just image::flash /dev/sdX "" base-station.img
[doc("Flash an image and copy env_dir/* to the boot partition (empty env_dir = nothing)")]
flash device env_dir="env" img="larp-station.img":
    #!/usr/bin/env bash
    set -euo pipefail
    img="{{ img }}"; dev="{{ device }}"; envdir="{{ env_dir }}"

    [ -f "$img" ] || { echo "image '$img' not found — run 'just image::build' (or build-base-station) first"; exit 1; }
    [ -b "$dev" ] || { echo "'$dev' is not a block device; run 'just image::devices'"; exit 1; }
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

    if [ -z "$envdir" ]; then
      echo ">> no env dir given — leaving the boot partition as-is"
      echo ">> done — $dev is ready to boot"
      exit 0
    fi

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
