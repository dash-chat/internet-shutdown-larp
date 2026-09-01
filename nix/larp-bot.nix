# NixOS module: run a LARP character bot (crates/larp-bot) against a mailbox.
#
# One image serves every station: the service only starts when the card's FAT
# boot partition carries an identity bundle (larp-identity.toml) and the cast
# file (larp-cast.toml) — both produced offline by `larp-bot keygen` / `cast`
# and copied by `just flash`. Stations without them (the base station) run
# the plain mailbox appliance unchanged.
#
# The same module serves the out-of-town character's Digital Ocean droplet: point
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
    # Only its public half is read: the contact code that goes into Mira's
    # informant tip. A card without the file just never tips.
    anonymous_identity = "${cfg.anonymousIdentityFile}"

    [timing]
    min_interval_secs = ${toString cfg.timing.minIntervalSecs}
    max_interval_secs = ${toString cfg.timing.maxIntervalSecs}
    first_mission_delay_secs = ${toString cfg.timing.firstMissionDelaySecs}
    poll_interval_secs = ${toString cfg.timing.pollIntervalSecs}
  '';

  # The spec bots (docs/design.md): scripted characters with no scenario pack
  # — the anonymous informant and the mayor. Same binary, own identity, own
  # data dir, each gated on its own flashed identity file.
  specConfigFile =
    unit:
    {
      identityFile,
      spec,
      avatar,
    }:
    pkgs.writeText "${unit}.toml" ''
      mailbox_url = "${cfg.mailboxUrl}"
      identity = "${identityFile}"
      spec = "${spec}"
      ${lib.optionalString (avatar != null) ''avatar = "${avatar}"''}
      data_dir = "/var/lib/${unit}"
    '';

  specService =
    unit: description: args:
    lib.mkIf (args.spec != null) {
      inherit description;
      wantedBy = [ "multi-user.target" ];
      after = [
        "network-online.target"
        "dashchat-mailbox.service"
      ];
      wants = [ "network-online.target" ];

      unitConfig.ConditionPathExists = [ args.identityFile ];

      serviceConfig = {
        ExecStart = "${lib.getExe' cfg.package "larp-bot"} spec --config ${specConfigFile unit args}";
        StateDirectory = unit;
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
        (services.dashchat-mailbox); the sister's droplet points this at the
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
      description = ''
        The public cast file (all characters' agent/device ids). Deliveries
        are recognized by their text now, so this only serves to tell
        character bots apart from players.
      '';
    };

    scenariosDir = lib.mkOption {
      type = lib.types.path;
      description = "Directory with all characters' scenario packs (baked into the image).";
    };

    anonymousSpec = lib.mkOption {
      type = lib.types.nullOr lib.types.path;
      default = null;
      description = ''
        The anonymous informant's script (anonymous.toml, baked into the
        image). When set, a second service runs the informant next to the
        character bot — gated, like the bot, on its own flashed identity.
        Every flash recipe copies that identity, so every station card runs
        the informant.
      '';
    };

    anonymousAvatar = lib.mkOption {
      type = lib.types.nullOr lib.types.path;
      default = null;
      description = ''
        The informant's chat avatar PNG (baked into the image). Explicit,
        unlike the scenario packs' sibling <character>.png convention: the
        spec is copied into the store as a lone file.
      '';
    };

    anonymousIdentityFile = lib.mkOption {
      type = lib.types.str;
      default = "/boot/firmware/larp-anonymous.toml";
      description = ''
        The flashed anonymous identity bundle. The informant service is
        gated on this path existing, and the character bot reads its public
        half to build the add-contact deep link Mira hands out (the informant
        has no QR poster). Missing means no informant and no tips, and the
        character bot runs as usual.
      '';
    };

    mayorSpec = lib.mkOption {
      type = lib.types.nullOr lib.types.path;
      default = null;
      description = ''
        The mayor's script (mayor.toml, baked into the image): his greeting
        is the game's onboarding, and his trigger phrase is the endgame.
        When set, a third service runs him — gated, like the others, on his
        own flashed identity. Only the base station card gets that identity
        (base-station.just), so he exists once, where the game begins.
      '';
    };

    mayorAvatar = lib.mkOption {
      type = lib.types.nullOr lib.types.path;
      default = null;
      description = "The mayor's chat avatar PNG (baked into the image).";
    };

    mayorIdentityFile = lib.mkOption {
      type = lib.types.str;
      default = "/boot/firmware/larp-mayor.toml";
      description = ''
        The flashed mayor identity bundle. The mayor service is gated on this
        path existing, so only the base-station card runs him.
      '';
    };

    timing = {
      minIntervalSecs = lib.mkOption {
        type = lib.types.ints.positive;
        default = 180;
        description = "Minimum seconds between missions, per player chat.";
      };
      maxIntervalSecs = lib.mkOption {
        type = lib.types.ints.positive;
        default = 480;
        description = "Maximum seconds between missions, per player chat.";
      };
      firstMissionDelaySecs = lib.mkOption {
        type = lib.types.ints.unsigned;
        default = 5;
        description = "Seconds between a player's welcome message and their first mission.";
      };
      pollIntervalSecs = lib.mkOption {
        type = lib.types.ints.positive;
        default = 3;
        description = "Seconds between direct-chat polls.";
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

    # The anonymous informant (docs/design.md): dormant unless the card was
    # flashed with the anonymous identity — every flash recipe copies it, so
    # every station runs him and a player finds him wherever they are.
    systemd.services.larp-bot-anonymous =
      specService "larp-bot-anonymous" "LARP anonymous informant bot (Dash Chat node)"
        {
          identityFile = cfg.anonymousIdentityFile;
          spec = cfg.anonymousSpec;
          avatar = cfg.anonymousAvatar;
        };

    # The mayor: onboarding in his greeting, the endgame in his trigger. Only
    # the base-station card is flashed with his identity, so unlike the
    # informant he exists exactly once.
    systemd.services.larp-bot-mayor =
      specService "larp-bot-mayor" "LARP mayor bot (Dash Chat node)" {
        identityFile = cfg.mayorIdentityFile;
        spec = cfg.mayorSpec;
        avatar = cfg.mayorAvatar;
      };
  };
}
