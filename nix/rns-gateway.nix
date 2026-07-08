# NixOS service for the RNS mailbox gateway (docs/rns-gateway.md): relays
# mailbox HTTP over a Reticulum LoRa link and advertises LoRa peers to the
# local mailbox via mDNS. Baked into every station image; per-card gating
# follows the identity-bundle convention — no /boot/firmware/lora.env on the
# FAT partition, no gateway (the card stays a plain station).
{
  config,
  lib,
  pkgs,
  ...
}:
let
  cfg = config.services.rns-gateway;
in
{
  options.services.rns-gateway = {
    enable = lib.mkEnableOption "RNS LoRa mailbox gateway";

    package = lib.mkOption {
      type = lib.types.package;
      description = "The rns-gateway package to run.";
    };

    mailboxUrl = lib.mkOption {
      type = lib.types.str;
      default = "http://127.0.0.1:3000";
      description = "The on-device mailbox the gateway fronts.";
    };

    envFile = lib.mkOption {
      type = lib.types.str;
      default = "/boot/firmware/lora.env";
      description = ''
        Per-card radio configuration (RNODE_PORT, RNODE_FREQ, RNODE_SF, … and
        optional GATEWAY_* overrides), flashed next to wifi-ap.env. The
        service is gated on this path existing.
      '';
    };
  };

  config = lib.mkIf cfg.enable {
    systemd.services.rns-gateway = {
      description = "RNS LoRa mailbox gateway";
      wantedBy = [ "multi-user.target" ];
      # The gateway retries the mailbox /health itself; the ordering just
      # avoids a pointless first retry cycle on boot.
      after = [ "dashchat-mailbox.service" ];

      # The per-card switch: no lora.env on the boot partition → no gateway.
      unitConfig.ConditionPathExists = [ cfg.envFile ];

      serviceConfig = {
        ExecStart = lib.getExe cfg.package;
        EnvironmentFile = cfg.envFile;
        StateDirectory = "rns-gateway";
        Restart = "always";
        RestartSec = 5;
        DynamicUser = true;
        # The RNode is a USB serial device (dialout group on NixOS).
        SupplementaryGroups = [ "dialout" ];

        ProtectSystem = "strict";
        ProtectHome = true;
        NoNewPrivileges = true;
        PrivateTmp = true;
      };

      environment = {
        GATEWAY_MAILBOX_URL = cfg.mailboxUrl;
        GATEWAY_DATA_DIR = "/var/lib/rns-gateway";
      };
    };
  };
}
