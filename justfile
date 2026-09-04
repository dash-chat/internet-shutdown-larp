# Recipes for the internet-shutdown LARP, organized in modules — run `just`
# to list everything. Run inside `nix develop` (or prefix with
# `nix run nixpkgs#just --`).

# Character provisioning: identity bundles, cast file, QR posters,
# per-station env dirs, local bot runs.
mod characters

# Station SD images: build + flash (station and base-station variants).
mod image

# The base station: flash the Pi card (joins the game's wifi, like the other
# stations); the map-lite submodule provisions a MikroTik mAP lite as the AP
# that broadcasts it.
mod base-station

# The sister's cloud host: CURRENTLY UNUSED — Mira runs on a Pi station like
# everyone else now. Kept for a future off-map character (doctl +
# nixos-infect deploy, ssh/logs/destroy).
mod sister

# Show available recipes.
_default:
    @just --list --list-submodules

# Run all tests (unit + e2e).
test:
    cargo test --workspace

# The direct-ethernet-cable helpers live in the mailbox image repo
# (scripts/ there, exported as nix packages: find-pi, ethernet-ssh,
# ethernet-set-time) and are re-exported by this flake. Caveat: with no
# DHCP server on the cable, the Pi's link only stays up ~2 min after boot
# (NetworkManager thrashes on the leaseless DHCP client) — power-cycle the
# Pi and run these shortly after it boots.

# Extra arguments become the remote command; with none you get an
# interactive shell.
[doc("SSH into the Pi on the direct ethernet link (optional remote command)")]
ssh *cmd:
    nix run .#ethernet-ssh --accept-flake-config -- {{ cmd }}

# Update a running, already-flashed station without reflashing. Two halves,
# shared by `deploy` (one Pi, ethernet cable) and `deploy-all` (every Pi on
# the wifi) via the _deploy-to helper:
#   1. the SYSTEM (bot binary, scenarios, services) — nix copy the
#      larp-station closure and switch the Pi to it. On an x86_64 host
#      without aarch64 emulation the exact working tree must already be in
#      the binary cache: commit, push, wait for CI.
#   2. the CARD FILES (identity, cast, extra bots) — refreshed on
#      /boot/firmware from secrets/ and the bots restarted. WHICH files is
#      read off the card itself: larp-identity.toml names the character, and
#      the presence of larp-mayor.toml marks the base station. wifi.env is
#      left alone.
# Reflash only for partition-layout or boot-breaking changes.
[doc("Deploy code + character files to the running Pi on the ethernet cable (no reflash)")]
deploy:
    #!/usr/bin/env bash
    set -euo pipefail
    just={{ quote(just_executable()) }}
    toplevel="$("$just" _toplevel)"
    pi="$(nix run .#find-pi --accept-flake-config)"
    "$just" _deploy-to "$pi" "$toplevel"

# Deploy to EVERY station Pi on the wifi network the laptop is currently
# joined to (all the stations at home on one AP, say). Discovery is an SSH
# sweep, not mDNS — the mailbox's _dashchat announce has served stale
# addresses before, while "answers ssh as admin@ and has a station card" is
# the ground truth the deploy needs anyway. The closure is built once and
# pushed to each Pi in turn; one failing Pi doesn't stop the rest.
#
# The game wifi has NO internet, so everything this needs must already be
# local: run `just deploy-fetch` while still on a connected network, THEN
# switch to the game wifi and run this. nmap comes from the devShell for the
# same reason. Sweep details: -Pn because unprivileged nmap can't ARP/ICMP
# ping, so its host discovery probes ports 80/443 — which the Pis don't open
# — and reports an all-22 LAN as empty; and every IPv4 subnet on the
# interface is swept, because a leftover address (e.g. MikroTik's default
# 192.168.88.x from mAP provisioning) can shadow the live one.
[doc("Deploy code + character files to every station Pi on the current wifi LAN (run deploy-fetch first while online)")]
deploy-all subnet="" iface="":
    #!/usr/bin/env bash
    set -euo pipefail
    just={{ quote(just_executable()) }}
    subnet={{ quote(subnet) }}
    iface={{ quote(iface) }}
    subnets=()
    if [ -n "$subnet" ]; then
      subnets=("$subnet")
    else
      if [ -z "$iface" ]; then
        for d in /sys/class/net/*; do
          dev=$(basename "$d")
          [ -d "$d/wireless" ] || continue
          [ "$(cat "$d/operstate" 2>/dev/null)" = up ] || continue
          iface="$dev"; break
        done
        [ -n "$iface" ] || { echo "error: no wifi interface is up — join the game's wifi first" >&2; exit 1; }
      fi
      mapfile -t subnets < <(ip -4 -o addr show dev "$iface" scope global | awk '{ print $4 }')
      [ "${#subnets[@]}" -gt 0 ] || { echo "error: no IPv4 address on $iface — join the game's wifi first" >&2; exit 1; }
    fi
    command -v nmap >/dev/null || { echo "error: nmap not found — re-enter 'nix develop' (the shell provides it)" >&2; exit 1; }
    hosts=()
    for net in "${subnets[@]}"; do
      echo ">> scanning $net${iface:+ ($iface)} for hosts with ssh open"
      mapfile -t -O "${#hosts[@]}" hosts < <(nmap -p 22 --open -T4 -n -Pn -oG - "$net" 2>/dev/null \
        | awk '/22\/open/ { print $2 }')
    done
    # A station is a host that answers ssh as admin@ AND carries a station
    # card — that filters out the laptop itself and any non-LARP machine.
    ssh_opts=(-o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o BatchMode=yes -o ConnectTimeout=4 -o LogLevel=ERROR)
    pis=()
    mapfile -t hosts < <(printf '%s\n' "${hosts[@]:-}" | sort -u)
    for h in "${hosts[@]:-}"; do
      [ -n "$h" ] || continue
      if ssh "${ssh_opts[@]}" "admin@$h" "test -f /boot/firmware/larp-identity.toml" 2>/dev/null; then
        pis+=("$h")
      fi
    done
    [ "${#pis[@]}" -gt 0 ] || { echo "error: no station Pis found on ${subnets[*]} — are they powered and on this wifi?" >&2; exit 1; }
    echo ">> found ${#pis[@]} station(s): ${pis[*]}"
    # The closure deploy-fetch recorded — no nix evaluation here, it needs
    # the network this wifi doesn't have.
    [ -f .deploy-toplevel ] || { echo "error: no .deploy-toplevel — run 'just deploy-fetch' while online first" >&2; exit 1; }
    toplevel="$(cat .deploy-toplevel)"
    [ -e "$toplevel" ] || { echo "error: recorded toplevel $toplevel is gone from the store (GC?) — re-run 'just deploy-fetch' online" >&2; exit 1; }
    echo ">> deploying recorded closure $toplevel"
    failed=()
    for pi in "${pis[@]}"; do
      echo
      echo "==== $pi ===="
      "$just" _deploy-to "$pi" "$toplevel" || failed+=("$pi")
    done
    echo
    if [ "${#failed[@]}" -gt 0 ]; then
      echo ">> deployed to $((${#pis[@]} - ${#failed[@]}))/${#pis[@]} station(s) — FAILED: ${failed[*]}" >&2
      exit 1
    fi
    echo ">> all ${#pis[@]} station(s) deployed"

# The game wifi is offline, so everything a deploy needs has to be in the
# local nix store BEFORE the laptop switches over. This substitutes the
# station closure from the binary cache and RECORDS its store path in
# .deploy-toplevel — deploy-all then deploys exactly that path with no nix
# evaluation at all, because evaluating offline stalls on the flake's git
# inputs and can't substitute anyway. nmap and ssh come from the devShell.
# After it succeeds: join the game wifi, `just deploy-all`, switch back.
[doc("While still online: fetch + record everything deploy-all needs, so it can run on the offline game wifi")]
deploy-fetch:
    #!/usr/bin/env bash
    set -euo pipefail
    just={{ quote(just_executable()) }}
    toplevel="$("$just" _toplevel)"
    printf '%s\n' "$toplevel" > .deploy-toplevel
    echo ">> recorded $toplevel"
    echo ">> ready: join the game wifi and run 'just deploy-all' (no internet needed there)"

# Build (in practice: substitute from the binary cache) the larp-station
# system closure and print its store path — the online first step behind
# `deploy` (cable) and `deploy-fetch` (before going to the offline wifi).
_toplevel:
    #!/usr/bin/env bash
    set -euo pipefail
    attr=".#nixosConfigurations.larp-station.config.system.build.toplevel"
    echo ">> building/substituting $attr" >&2
    if ! nix build --no-link --print-out-paths --accept-flake-config "$attr"; then
      echo "error: toplevel build failed. On an x86_64 host without aarch64 emulation this" >&2
      echo "usually means the binary cache doesn't have this exact tree yet — commit, push," >&2
      echo "wait for CI, and re-run (or enable boot.binfmt.emulatedSystems = [ \"aarch64-linux\" ])." >&2
      exit 1
    fi

# Full deploy of one already-built toplevel to the Pi at <pi> (an IP, or an
# IPv6 link-local with %zone): card files, system switch, bot restarts.
_deploy-to pi toplevel:
    #!/usr/bin/env bash
    set -euo pipefail
    pi={{ quote(pi) }}
    toplevel={{ quote(toplevel) }}
    ssh_opts=(-o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o BatchMode=yes -o ConnectTimeout=8 -o LogLevel=ERROR)
    on_pi() { ssh "${ssh_opts[@]}" "admin@$pi" "$@"; }
    char="$(on_pi "sed -n 's/^character = \"\(.*\)\"/\1/p' /boot/firmware/larp-identity.toml")"
    [ -n "$char" ] || { echo "no character on the card (/boot/firmware/larp-identity.toml) — is this a flashed station?"; exit 1; }
    [ -f "secrets/$char-identity.toml" ] || { echo "the card runs '$char' but there is no secrets/$char-identity.toml"; exit 1; }
    [ -f secrets/larp-cast.toml ] || { echo "no secrets/larp-cast.toml — run 'just characters::generate' first"; exit 1; }
    # Card files FIRST, system second: the switch restarts the bots, and a
    # new bot against old card files refuses to start (the identity-vs-
    # profile-name guard), while an old bot with new files is fine.
    echo ">> station runs '$char' — refreshing character files on the boot partition"
    on_pi "sudo tee /boot/firmware/larp-identity.toml >/dev/null" < "secrets/$char-identity.toml"
    on_pi "sudo tee /boot/firmware/larp-cast.toml >/dev/null" < secrets/larp-cast.toml
    units=(larp-bot)
    if on_pi "test -f /boot/firmware/larp-mayor.toml"; then
      on_pi "sudo tee /boot/firmware/larp-mayor.toml >/dev/null" < secrets/mayor-identity.toml
      units+=(larp-bot-mayor)
    fi
    # nix's ssh:// URLs need IPv6 addresses bracketed, with a link-local
    # zone separator URL-escaped (fe80::x%eth0 -> [fe80::x%25eth0]).
    host="$pi"
    case "$host" in
      *%*) host="[${host//%/%25}]" ;;
      *:*) host="[$host]" ;;
    esac
    echo ">> copying missing store paths to admin@$pi"
    NIX_SSHOPTS="${ssh_opts[*]}" nix copy --to "ssh://admin@$host" "$toplevel"
    echo ">> switching the system"
    on_pi "sudo nix-env -p /nix/var/nix/profiles/system --set '$toplevel' \
      && sudo '$toplevel/bin/switch-to-configuration' switch"
    echo ">> restarting: ${units[*]}"
    on_pi "sudo systemctl restart ${units[*]}"
    echo ">> deployed: '$char' station is on the new system with fresh card files"

# Writes the RTC when one is present (battery on J5 — then the time
# survives power-off and reflashing; without it, only until shutdown).
[doc("Push the laptop's time to the Pi on the direct ethernet link (writes the RTC if present)")]
set-time iface="":
    nix run .#ethernet-set-time --accept-flake-config -- {{ iface }}
