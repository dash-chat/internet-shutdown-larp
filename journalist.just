# The journalist's cloud host (docs/design.md §Journalist): a Digital Ocean
# droplet running only the bot against the cloud mailbox. One-time setup:
# `doctl auth init` with a DO API token, and at least one SSH key registered
# in the account (`doctl compute ssh-key import`). doctl comes with the
# devShell (flake.nix).

droplet_name := "larp-journalist"

# Deploy is idempotent: the first run creates an Ubuntu droplet whose
# cloud-init converts it to NixOS (nixos-infect), then pushes the flake's
# journalist-droplet config + secrets; later runs skip straight to the push,
# so re-run after changing scenarios/, the bot, or the flake config.
[doc("Deploy the journalist bot to a DO droplet (creates + converts to NixOS on first run; re-run to update)")]
deploy region="ams3" size="s-1vcpu-1gb":
    #!/usr/bin/env bash
    set -euo pipefail
    ssh_opts=(-o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null)
    name={{ quote(droplet_name) }}

    [ -f secrets/journalist-identity.toml ] || { echo "no secrets/journalist-identity.toml — run 'just characters::generate' first"; exit 1; }
    [ -f secrets/larp-cast.toml ] || { echo "no secrets/larp-cast.toml — run 'just characters::generate' first"; exit 1; }
    doctl account get > /dev/null || { echo "doctl is not authorized — run 'doctl auth init'"; exit 1; }

    ip="$(doctl compute droplet get "$name" --format PublicIPv4 --no-header 2>/dev/null || true)"
    if [ -z "$ip" ]; then
      keys="$(doctl compute ssh-key list --format FingerPrint --no-header | paste -sd, -)"
      [ -n "$keys" ] || { echo "no SSH keys in the DO account — run: doctl compute ssh-key import my-key --public-key-file ~/.ssh/id_ed25519.pub"; exit 1; }
      echo ">> creating droplet $name ({{ region }}, {{ size }})"
      ip="$(doctl compute droplet create "$name" \
        --image ubuntu-24-04-x64 --region {{ quote(region) }} --size {{ quote(size) }} \
        --ssh-keys "$keys" --user-data '#cloud-config
    runcmd:
      - curl -fsSL https://raw.githubusercontent.com/elitak/nixos-infect/master/nixos-infect | PROVIDER=digitalocean NIX_CHANNEL=nixos-25.11 bash 2>&1 | tee /tmp/infect.log' \
        --wait --format PublicIPv4 --no-header)"
      echo ">> droplet is up at $ip — waiting for nixos-infect (~5-10 min, ends in a reboot)"
    else
      echo ">> droplet $name exists at $ip"
    fi

    until ssh "${ssh_opts[@]}" -o ConnectTimeout=5 "root@$ip" test -e /etc/NIXOS 2>/dev/null; do
      echo "   ...not NixOS yet, retrying in 15s"
      sleep 15
    done

    echo ">> pushing secrets"
    ssh "${ssh_opts[@]}" "root@$ip" mkdir -p /var/lib/larp-secrets
    scp "${ssh_opts[@]}" secrets/journalist-identity.toml secrets/larp-cast.toml "root@$ip:/var/lib/larp-secrets/"
    # The bot runs as a DynamicUser, so the secrets must be world-readable —
    # same exposure as the stations' FAT boot partition.
    ssh "${ssh_opts[@]}" "root@$ip" chmod 644 /var/lib/larp-secrets/journalist-identity.toml /var/lib/larp-secrets/larp-cast.toml

    echo ">> building + switching the droplet to the flake config"
    export NIX_SSHOPTS="-o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null"
    nixos-rebuild switch --flake .#journalist-droplet --target-host "root@$ip" --use-substitutes
    echo ">> deployed — follow the bot with 'just journalist::logs'"

# SSH into the droplet (extra args are passed to ssh).
ssh *args:
    #!/usr/bin/env bash
    set -euo pipefail
    ip="$(doctl compute droplet get {{ quote(droplet_name) }} --format PublicIPv4 --no-header)"
    exec ssh -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null "root@$ip" {{ args }}

# Follow the journalist bot's journal on the droplet.
logs:
    @just journalist::ssh journalctl -u larp-bot -f

[doc("Tear the droplet down (the identity survives in secrets/ — a later deploy recreates the same character)")]
[confirm("destroy the journalist droplet? [y/N]")]
destroy:
    doctl compute droplet delete {{ quote(droplet_name) }} --force
