# Turn a stock (defconf) MikroTik mAP lite into the base-station AP: the mAP
# only broadcasts the wifi and bridges it to ether1, where the base-station
# Pi (nix/base-station.nix) owns DHCP/DNS and serves the captive portal +
# mailbox.
# Template rendered by `just provision` (@PLACEHOLDER@ values) and imported
# on the router with /import. Idempotent: safe to re-import.

# ether1 is a WAN port in the default config (DHCP client, WAN interface
# list, untrusted by the firewall) — make it a LAN bridge port instead: it's
# the cable to the Pi.
/ip dhcp-client remove [find where interface="ether1"]
/interface list member remove [find where interface="ether1" and list="WAN"]
:if ([:len [/interface bridge port find where interface="ether1"]] = 0) do={
  /interface bridge port add bridge=bridge interface=ether1 comment="base-station: cable to the Pi"
}
:put "base-station: ether1 bridged into the LAN"

# The Pi owns DHCP + DNS on this network.
/ip dhcp-server disable [find where name="defconf"]
:put "base-station: built-in DHCP server disabled (the Pi serves DHCP/DNS)"

# Clean up anything left from the RouterOS-hotspot era of this tooling
# (formerly the map-lite-portal repo).
/ip hotspot remove [find where name="map-lite-portal"]
/ip hotspot profile remove [find where name="map-lite-portal"]
/ip hotspot ip-binding remove [find where comment~"map-lite-portal"]
/file remove [find where name~"flash/portal"]
:put "base-station: legacy hotspot portal removed"

# Shrink the wifi bubble, like the Pi stations' range-limited APs: fixed
# minimum tx power, plus a signal gate — clients the AP hears weaker than
# @MIN_SIGNAL@ dBm can't associate, and connected ones that fall below it for
# 10s are kicked. Unlike the Pi's brcmfmac (which reports no per-client
# signal, so its RSSI gate fails open and eviction needs a bitrate guard),
# RouterOS tracks per-client signal and enforces both ends of this natively.
# LAST in this script: changing wireless settings can blip the link this
# import arrived over; everything above is already committed by then.
/interface wireless set [find default-name="wlan1"] tx-power-mode=all-rates-fixed tx-power=@TX_POWER@
/interface wireless set [find default-name="wlan1"] default-authentication=no
:if ([:len [/interface wireless access-list find where comment="base-station: proximity gate"]] = 0) do={
  /interface wireless access-list add signal-range=@MIN_SIGNAL@..120 allow-signal-out-of-range=10s authentication=yes comment="base-station: proximity gate"
} else={
  /interface wireless access-list set [find where comment="base-station: proximity gate"] signal-range=@MIN_SIGNAL@..120 allow-signal-out-of-range=10s authentication=yes
}
:put "base-station: wifi range clamped (tx-power @TX_POWER@ dBm, join/kick gate at @MIN_SIGNAL@ dBm)"

:put "base-station: done"
