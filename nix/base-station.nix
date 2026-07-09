# CURRENTLY UNUSED: the flake no longer imports this module — for now the
# base station hosts its own Pi wifi like every other station (wifi-ap.env).
# Kept for the mAP-lite setup below, should the Pi AP prove too weak.
#
# Base-station networking: a MikroTik mAP lite broadcasts the mesh (a real AP
# that comfortably carries 30-40 concurrent clients — the Pi's brcmfmac AP
# mode does not), wired to this Pi over ethernet (the Pi can even power it,
# see the mailbox image's usb_max_current_enable). The Pi hosts no wifi at
# all; instead it owns DHCP + wildcard DNS on the cable, so every client's
# connectivity probe lands on the portal nginx and the captive-portal screen
# pops — without RouterOS's hotspot feature, which is locked behind
# device-mode (physical button press) on current firmware.
#
# Traffic-wise this is the same as the old hotspot plan: clients sync with
# the Pi's mailbox client -> mAP -> Pi, one wifi transit plus the cable.
# Nothing is gated: the portal is onboarding UX (the mayor), and headless
# clients need no bypass.
#
# The mAP side is one idempotent script applied by `just provision` in
# ../map-lite-portal: ether1 moves from WAN to the LAN bridge and the
# built-in DHCP server is disabled.
{
  config,
  lib,
  pkgs,
  ...
}:
let
  cfg = config.dashchat.baseStation;
  # First three octets, for deriving the DHCP range.
  lanPrefix = lib.concatStringsSep "." (lib.take 3 (lib.splitString "." cfg.address));
in
{
  options.dashchat.baseStation = {
    interface = lib.mkOption {
      type = lib.types.str;
      default = "end0";
      description = "Ethernet interface cabled to the mAP lite.";
    };
    address = lib.mkOption {
      type = lib.types.str;
      default = "192.168.88.2";
      description = ''
        Static IPv4 address on the cable. Must be inside the mAP lite's LAN
        subnet (defconf: 192.168.88.1/24) and outside the DHCP range this
        module hands out (.10-.250).
      '';
    };
  };

  config = {
    # No Pi-hosted wifi and no client mode either: the mAP broadcasts the
    # mesh. Keep wifi-provision off so a stray wifi-ap.env/wifi.env on the
    # boot partition can't start a competing AP.
    systemd.services.wifi-provision.wantedBy = lib.mkForce [ ];

    # Static address on the cable to the mAP.
    networking.networkmanager.ensureProfiles.profiles.base-station-lan = {
      connection = {
        id = "base-station-lan";
        type = "ethernet";
        interface-name = cfg.interface;
        autoconnect = true;
      };
      ipv4 = {
        method = "manual";
        address1 = "${cfg.address}/24";
        never-default = true;
      };
      ipv6.method = "disabled";
    };

    # DHCP + DNS for the mAP's wifi clients (its own DHCP server is disabled
    # by map-lite-portal's `just provision`). The wildcard resolves EVERY
    # name to the Pi — same captive-portal trick as AP mode's dnsmasq, just
    # bound to the cable instead of wlan0.
    environment.etc."dashchat-lan/dnsmasq.conf".text = ''
      port=53
      interface=${cfg.interface}
      bind-dynamic
      no-resolv
      dhcp-range=${lanPrefix}.10,${lanPrefix}.250,12h
      dhcp-option=option:router,${cfg.address}
      dhcp-option=option:dns-server,${cfg.address}
      dhcp-authoritative
      address=/#/${cfg.address}
    '';

    systemd.services.dashchat-lan-dnsmasq = {
      description = "DHCP/DNS for the mAP-lite LAN (base station)";
      wantedBy = [ "multi-user.target" ];
      after = [ "network.target" ];
      serviceConfig = {
        ExecStart = "${pkgs.dnsmasq}/bin/dnsmasq --keep-in-foreground --conf-file=/etc/dashchat-lan/dnsmasq.conf";
        Restart = "on-failure";
        RestartSec = 2;
      };
    };

    # Our captive-portal module (./captive-portal.nix, re-added here after
    # the mailbox image dropped its own) already provides the nginx pair
    # (mayor portal + catch-all 302) — re-point it at the LAN address.
    services.nginx.virtualHosts.captive-portal.serverAliases = [ cfg.address ];
    services.nginx.virtualHosts.captive-catchall.locations."/".return =
      lib.mkForce "302 http://${cfg.address}/";

    networking.firewall = {
      # end0 is in the mailbox module's trustedInterfaces, but don't depend
      # on that: open what the LAN role needs explicitly.
      interfaces.${cfg.interface} = {
        allowedUDPPorts = [
          53
          67
        ];
        allowedTCPPorts = [
          53
          80
        ];
      };

      # Clients with hardcoded DNS (8.8.8.8 …) send it to their gateway — us.
      # Reuse the captive-portal REDIRECT chain ./captive-portal.nix defines
      # for wlan0 (mkAfter: the chain must exist before we jump to it).
      extraCommands = lib.mkAfter ''
        iptables -t nat -C PREROUTING -i ${cfg.interface} -j captive-portal 2>/dev/null || \
          iptables -t nat -A PREROUTING -i ${cfg.interface} -j captive-portal
      '';
      extraStopCommands = ''
        iptables -t nat -D PREROUTING -i ${cfg.interface} -j captive-portal 2>/dev/null || true
      '';
    };
  };
}
