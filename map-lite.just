# The base-station mAP lite (router/base-station.rsc.tpl). The mAP only
# broadcasts the wifi; the base-station Pi wired behind it does everything
# else (nix/base-station.nix).

# Every device has its own host key and the link is local, so don't pin them.
# RouterOS (< 7.9) only offers an ssh-rsa (SHA-1) host key and SHA-1 DH kex,
# which OpenSSH >= 8.8 / 10.0 removed from its defaults — re-enable them or the
# handshake fails before the password is ever sent.
# ServerAlive*: a dropped wifi link under an open ssh session hangs forever
# without keepalives (ConnectTimeout only bounds connection setup), so make a
# dead link fail within ~6s instead.
ssh_opts := "-o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o ConnectTimeout=8 -o ServerAliveInterval=2 -o ServerAliveCountMax=3 -o LogLevel=ERROR -o PreferredAuthentications=password -o PubkeyAuthentication=no -o HostKeyAlgorithms=+ssh-rsa -o KexAlgorithms=+diffie-hellman-group14-sha1,diffie-hellman-group-exchange-sha1"

# Join a wifi network with NetworkManager (empty key = open network)
connect ssid wifi_key="":
    #!/usr/bin/env bash
    set -euo pipefail
    ssid={{ quote(ssid) }}
    key={{ quote(wifi_key) }}
    nmcli radio wifi on
    found=""
    security=""
    for _ in $(seq 1 20); do
      # -t escapes ':' inside fields as '\:', so a plain SSID matches $1 as-is
      hit=$(nmcli -t -f SSID,SECURITY device wifi list --rescan yes 2>/dev/null \
        | awk -F: -v s="$ssid" '$1 == s { print "found:" $2; exit }')
      if [ -n "$hit" ]; then found=1; security=${hit#found:}; break; fi
      sleep 2
    done
    [ -n "$found" ] || { echo "error: wifi network '$ssid' not found — is the mAP lite powered on and in range?" >&2; exit 1; }
    # a key/security mismatch would otherwise surface as nmcli's cryptic
    # "secrets were required, but not provided" — catch it before connecting
    if [ -n "$key" ] && { [ -z "$security" ] || [ "$security" = "--" ]; }; then
      echo "warning: '$ssid' is an open network — ignoring the given wifi key" >&2
      key=""
    fi
    if [ -z "$key" ] && [ -n "$security" ] && [ "$security" != "--" ]; then
      echo "error: '$ssid' is a secured network ($security) — a wifi key is required" >&2
      exit 1
    fi
    # drop any stale profile so a previously stored key can't shadow the given one
    nmcli connection delete "$ssid" >/dev/null 2>&1 || true
    on_fail() {
      nmcli connection delete "$ssid" >/dev/null 2>&1 || true
      echo "error: joining '$ssid' failed${key:+ — wrong wifi key?}" >&2
      exit 1
    }
    if [ -n "$key" ]; then
      nmcli device wifi connect "$ssid" password "$key" >/dev/null || on_fail
    else
      nmcli device wifi connect "$ssid" >/dev/null || on_fail
    fi
    echo "connected to '$ssid'"

# Open a RouterOS CLI on the mAP lite
cli admin_password="" router_ip="192.168.88.1":
    #!/usr/bin/env bash
    set -euo pipefail
    pass={{ quote(admin_password) }}
    ip={{ quote(router_ip) }}
    sshpass -p "$pass" ssh {{ ssh_opts }} "admin@$ip"

# Turns a stock mAP lite into the base-station AP: ether1 joins the LAN
# bridge (the cable to the Pi), the built-in DHCP server is disabled (the Pi
# behind it serves DHCP/DNS and the captive portal — the base-station image),
# and the wifi range is clamped, mirroring the Pi stations' range-limited
# APs: fixed tx power (dBm; the radio clamps at its own floor) and the
# weakest client signal (dBm, at the AP) allowed to join — and to stay:
# connected clients below it for 10s are kicked. For a bigger bubble raise
# both: `just base-station::map-lite::provision <ssid> '' <pass> 5 -70`.
# router_ip is auto-detected from the wifi gateway when empty.
# Takes the device's CURRENT wifi credentials (empty wifi_key = open network,
# empty admin_password = factory-default) and never changes them.
# Factory-fresh devices first need one manual WebFig login (the sticker
# password ships expired for ssh): set a real admin password and use that.
[doc("Configure a mAP lite as the base-station AP (bridge ether1, DHCP off, short range)")]
provision ssid wifi_key="" admin_password="" tx_power="1" min_signal="-60" router_ip="":
    #!/usr/bin/env bash
    set -euo pipefail
    ssid={{ quote(ssid) }}
    wifi_key={{ quote(wifi_key) }}
    admin_password={{ quote(admin_password) }}
    router_ip={{ quote(router_ip) }}
    tx_power={{ quote(tx_power) }}
    min_signal={{ quote(min_signal) }}
    just={{ quote(just_executable()) }}

    log()  { printf '\033[1;34m==>\033[0m %s\n' "$*"; }
    warn() { printf '\033[1;33mwarning:\033[0m %s\n' "$*" >&2; }
    die()  { printf '\033[1;31merror:\033[0m %s\n' "$*" >&2; exit 1; }
    ros() { sshpass -p "$admin_password" ssh {{ ssh_opts }} "admin@$router_ip" "$1" | tr -d '\r'; }
    wait_for_router() {
      log "Waiting for $router_ip to respond"
      for _ in $(seq 1 30); do
        if ping -c 1 -W 1 "$router_ip" >/dev/null 2>&1; then return 0; fi
        sleep 1
      done
      die "$router_ip is not responding"
    }

    # on failure, get back on the wifi we came from instead of leaving the
    # machine stranded on the device's network without internet
    rsc=""
    prev_wifi=""
    cleanup() {
      status=$?
      [ -n "$rsc" ] && rm -f "$rsc"
      if [ "$status" -ne 0 ] && [ -n "$prev_wifi" ]; then
        warn "reconnecting to '$prev_wifi'"
        nmcli connection up "$prev_wifi" >/dev/null 2>&1 || true
      fi
    }
    trap cleanup EXIT

    [[ "$tx_power" =~ ^-?[0-9]+$ ]] || die "tx_power must be an integer (dBm), got '$tx_power'"
    [[ "$min_signal" =~ ^-[0-9]+$ ]] || die "min_signal must be a negative integer (dBm), got '$min_signal'"

    # 1. reach the device over its wifi (empty wifi_key = open network)
    command -v nmcli >/dev/null || die "nmcli not found (NetworkManager is required)"
    prev_wifi=$(nmcli -t -f NAME,TYPE connection show --active | awk -F: '$2 == "802-11-wireless" { print $1; exit }' || true)
    "$just" base-station::map-lite::connect "$ssid" "$wifi_key"
    wifi_dev=$(nmcli -t -f DEVICE,TYPE,STATE device status | awk -F: '$2 == "wifi" && $3 == "connected" { print $1; exit }')
    [ -n "$wifi_dev" ] || die "connected to '$ssid' but no wifi device reports as connected"
    if [ -z "$router_ip" ]; then
      router_ip=$(nmcli -g IP4.GATEWAY device show "$wifi_dev" | head -n1)
      [ -n "$router_ip" ] || router_ip="192.168.88.1"
    fi
    wait_for_router

    # 2. authenticate
    log "Logging in to RouterOS at $router_ip"
    if ! auth_err=$(ros ':put ok' 2>&1 >/dev/null); then
      # a message here means ssh itself failed (e.g. algorithm negotiation),
      # not a rejected password — sshpass reports a bad password silently
      [ -n "$auth_err" ] && printf '%s\n' "$auth_err" | sed 's/^/    /' >&2
      warn "factory-fresh device? The sticker password ships expired and can't be used over ssh. Do the first login by hand in WebFig at http://$router_ip (it makes you set a new password) and re-run with that password."
      die "cannot log in as admin@$router_ip with the given password"
    fi
    model=$(ros ':put ([/system routerboard get model] . " / RouterOS " . [/system resource get version])' 2>/dev/null || echo "unknown")
    log "Connected to: $model"
    case "$model" in
      *mAP*) ;;
      *) warn "device does not report as a mAP — continuing anyway" ;;
    esac

    # 3. render and apply the base-station script (idempotent)
    log "Applying base-station.rsc (tx-power $tx_power dBm, signal gate $min_signal dBm)"
    rsc=$(mktemp -t base-station.XXXXXX.rsc)
    sed -e "s|@TX_POWER@|$tx_power|g" -e "s|@MIN_SIGNAL@|$min_signal|g" router/base-station.rsc.tpl > "$rsc"
    sshpass -p "$admin_password" scp {{ ssh_opts }} -q "$rsc" "admin@$router_ip:/base-station.rsc"
    # the wireless reconfig at the end of the script can blip the wifi under
    # this ssh session — if the confirmation goes missing, reconnect and let
    # the verification below decide (the script is idempotent anyway)
    import_out=$(ros '/import file-name=base-station.rsc' || true)
    printf '%s\n' "$import_out" | sed 's/^/    /'
    if ! printf '%s' "$import_out" | grep -q 'executed successfully'; then
      warn "no import confirmation (wifi blip during the wireless reconfig?) — reconnecting to verify"
      sleep 3
      "$just" base-station::map-lite::connect "$ssid" "$wifi_key"
      wait_for_router
    fi

    # 4. verify
    log "Verifying"
    [ "$(ros ':put [:len [/interface bridge port find where interface="ether1"]]')" = 1 ] \
      || die "ether1 is not a bridge port"
    [ "$(ros ':put [:len [/ip dhcp-server find where disabled=no]]')" = 0 ] \
      || die "a DHCP server is still enabled on the device"
    [ "$(ros ':put [:len [/ip hotspot find]]')" = 0 ] \
      || die "a RouterOS hotspot is still configured on the device"
    [ "$(ros ':put [/interface wireless get [find default-name="wlan1"] tx-power-mode]')" = "all-rates-fixed" ] \
      || die "wlan1 tx-power is not clamped"
    [ "$(ros ':put [:len [/interface wireless access-list find where comment="base-station: proximity gate"]]')" = 1 ] \
      || die "the proximity-gate access-list is missing"

    log "Done. '$ssid' is the base-station AP."
    cat <<EOF

        SSID:            $ssid
        Router address:  $router_ip

        Wire the mAP lite's ethernet port to the base-station Pi
        ('just image::build-base-station', then 'just base-station::flash
        /dev/sdX'). The Pi serves DHCP/DNS and the
        captive portal — until it's up and cabled, clients joining '$ssid'
        will get no IP address (this machine's current lease survives until
        it expires or reconnects).

        Wifi SSID/key stay whatever the device had — change them in WebFig
        at http://$router_ip if needed.

        Range is clamped: tx-power $tx_power dBm; clients the AP hears
        below $min_signal dBm can't join and are kicked after 10s outside
        that range. Tune with
        'just base-station::map-lite::provision <ssid> [key] [pass] <tx_power> <min_signal>'
        (re-runs are idempotent) after a walk test.
    EOF
