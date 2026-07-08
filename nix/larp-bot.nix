# NixOS module: run a LARP character bot (crates/larp-bot) against a mailbox.
#
# One image serves every station: the service only starts when the card's FAT
# boot partition carries an identity bundle (larp-identity.toml) and the cast
# file (larp-cast.toml) — both produced offline by `larp-bot keygen` / `cast`
# and copied by `just flash`. Stations without them (the base station, the
# relative-near LoRa end) run the plain mailbox appliance unchanged.
#
# The same module serves the journalist's Digital Ocean droplet: point
# `identityFile`/`castFile` at the deployed secret paths and `mailboxUrl` at
# the cloud mailbox.
{
  config,
  lib,
  pkgs,
  ...
}:
let
  cfg = config.services.larp-bot;

  configFile = pkgs.writeText "larp-bot-config.toml" ''
    mailbox_url = "${cfg.mailboxUrl}"
    identity = "${cfg.identityFile}"
    cast = "${cfg.castFile}"
    scenarios_dir = "${cfg.scenariosDir}"
    data_dir = "/var/lib/larp-bot"

    [timing]
    min_interval_secs = ${toString cfg.timing.minIntervalSecs}
    max_interval_secs = ${toString cfg.timing.maxIntervalSecs}
    first_mission_delay_secs = ${toString cfg.timing.firstMissionDelaySecs}
    max_outstanding = ${toString cfg.timing.maxOutstanding}
    poll_interval_secs = ${toString cfg.timing.pollIntervalSecs}
  '';
in
{
  options.services.larp-bot = {
    enable = lib.mkEnableOption "LARP character bot";

    package = lib.mkOption {
      type = lib.types.package;
      description = "The larp-bot package to run.";
    };

    mailboxUrl = lib.mkOption {
      type = lib.types.str;
      default = "http://127.0.0.1:3000";
      description = ''
        Mailbox the bot syncs through. Default is the on-device mailbox
        (services.dashchat-mailbox); the journalist droplet points this at the
        cloud mailbox instead.
      '';
    };

    identityFile = lib.mkOption {
      type = lib.types.str;
      default = "/boot/firmware/larp-identity.toml";
      description = ''
        The character's flashed identity bundle. The service is gated on this
        path existing, so a card without a bundle simply runs no bot.
      '';
    };

    castFile = lib.mkOption {
      type = lib.types.str;
      default = "/boot/firmware/larp-cast.toml";
      description = "The public cast file (all characters' agent/device ids).";
    };

    scenariosDir = lib.mkOption {
      type = lib.types.path;
      description = "Directory with all characters' scenario packs (baked into the image).";
    };

    timing = {
      minIntervalSecs = lib.mkOption {
        type = lib.types.ints.positive;
        default = 180;
        description = "Minimum seconds between missions, per group.";
      };
      maxIntervalSecs = lib.mkOption {
        type = lib.types.ints.positive;
        default = 480;
        description = "Maximum seconds between missions, per group.";
      };
      firstMissionDelaySecs = lib.mkOption {
        type = lib.types.ints.unsigned;
        default = 5;
        description = "Seconds between a group's welcome message and its first mission.";
      };
      maxOutstanding = lib.mkOption {
        type = lib.types.ints.positive;
        default = 3;
        description = "Unacked missions per group before the bot pauses.";
      };
      pollIntervalSecs = lib.mkOption {
        type = lib.types.ints.positive;
        default = 3;
        description = "Seconds between group polls.";
      };
    };
  };

  config = lib.mkIf cfg.enable {
    systemd.services.larp-bot = {
      description = "LARP character bot (Dash Chat node)";
      wantedBy = [ "multi-user.target" ];
      after = [
        "network-online.target"
        "dashchat-mailbox.service"
      ];
      wants = [ "network-online.target" ];

      # The per-card switch: no identity bundle on the boot partition → no bot.
      unitConfig.ConditionPathExists = [
        cfg.identityFile
        cfg.castFile
      ];

      serviceConfig = {
        ExecStart = "${lib.getExe' cfg.package "larp-bot"} run --config ${configFile}";
        StateDirectory = "larp-bot";
        Restart = "always";
        RestartSec = 5;
        DynamicUser = true;

        ProtectSystem = "strict";
        ProtectHome = true;
        NoNewPrivileges = true;
        PrivateTmp = true;
      };

      environment.RUST_LOG = lib.mkDefault "larp_bot=info,dashchat_node=warn,mailbox_client=warn";
    };
  };
}
