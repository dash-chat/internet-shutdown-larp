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
    # The mayor's spec bot touches this when his trigger fires. Only the base
    # card ever has both bots, so only Nadia ever sees it appear — her
    # mayor_fallen eruption rides on it. NOT inside the mayor's state dir:
    # with DynamicUser, a StateDirectory really lives under /var/lib/private
    # (0700, root-only) behind a symlink, so a flag in there is invisible to
    # every other service. /var/lib/larp is a shared tmpfiles dir instead.
    mayor_fallen_flag = "/var/lib/larp/mayor-triggered"

    [timing]
    min_interval_secs = ${toString cfg.timing.minIntervalSecs}
    max_interval_secs = ${toString cfg.timing.maxIntervalSecs}
    first_mission_delay_secs = ${toString cfg.timing.firstMissionDelaySecs}
    poll_interval_secs = ${toString cfg.timing.pollIntervalSecs}
  '';

  # The spec bot (docs/design.md): a scripted character with no scenario pack
  # — the mayor. Same binary, own identity, own data dir, gated on his own
  # flashed identity file.
  specConfigFile =
    unit:
    {
      identityFile,
      spec,
      avatar,
      triggeredFlag ? null,
    }:
    pkgs.writeText "${unit}.toml" ''
      mailbox_url = "${cfg.mailboxUrl}"
      identity = "${identityFile}"
      spec = "${spec}"
      ${lib.optionalString (avatar != null) ''avatar = "${avatar}"''}
      data_dir = "/var/lib/${unit}"
      ${lib.optionalString (triggeredFlag != null) ''triggered_flag = "${triggeredFlag}"''}
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

        # The stations have no NTP and (batteryless) no RTC; the bot steps
        # the clock forward from the timestamps on received messages instead
        # (crates/larp-bot/src/clock.rs). Without this the step is refused
        # and replies never thread onto the deliveries they answer.
        AmbientCapabilities = [ "CAP_SYS_TIME" ];
        CapabilityBoundingSet = [ "CAP_SYS_TIME" ];

        ProtectSystem = "strict";
        ProtectHome = true;
        NoNewPrivileges = true;
        PrivateTmp = true;
      }
      // lib.optionalAttrs (args.triggeredFlag or null != null) {
        # ProtectSystem=strict leaves only the StateDirectory writable; the
        # trigger flag deliberately lives outside it (see mayor_fallen_flag
        # above), so its directory must be opened up explicitly.
        ReadWritePaths = [ (builtins.dirOf args.triggeredFlag) ];
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

    mayorSpec = lib.mkOption {
      type = lib.types.nullOr lib.types.path;
      default = null;
      description = ''
        The mayor's script (mayor.toml, baked into the image): his greeting
        is the official emergency notice, and his trigger phrase is the
        endgame. When set, a second service runs him — gated, like the
        character bot, on his own flashed identity. Only the base station
        card gets that identity (base-station.just), so he exists once,
        where the game begins — sharing the Pi with the neighbour's
        character bot, whose greeting carries the actual onboarding.
      '';
    };

    mayorAvatar = lib.mkOption {
      type = lib.types.nullOr lib.types.path;
      default = null;
      description = ''
        The mayor's chat avatar PNG (baked into the image). Explicit,
        unlike the scenario packs' sibling <character>.png convention: the
        spec is copied into the store as a lone file.
      '';
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
    # The cross-service handshake dir (the mayor's trigger flag): sticky and
    # world-writable like /tmp, because the flag's writer is a DynamicUser
    # with no stable uid to chown to. Wiped with the rest of /var/lib on a
    # game-day reset.
    systemd.tmpfiles.rules = [ "d /var/lib/larp 1777 root root -" ];

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

        # See specService above: the offline stations' clock is stepped
        # forward from received-message timestamps, which needs CAP_SYS_TIME.
        AmbientCapabilities = [ "CAP_SYS_TIME" ];
        CapabilityBoundingSet = [ "CAP_SYS_TIME" ];

        ProtectSystem = "strict";
        ProtectHome = true;
        NoNewPrivileges = true;
        PrivateTmp = true;
      };

      environment.RUST_LOG = lib.mkDefault "larp_bot=info,dashchat_node=warn,mailbox_client=warn";
    };

    # The mayor: the official notice in his greeting, the endgame in his
    # trigger. Only the base-station card is flashed with his identity, so
    # he exists exactly once.
    systemd.services.larp-bot-mayor =
      specService "larp-bot-mayor" "LARP mayor bot (Dash Chat node)" {
        identityFile = cfg.mayorIdentityFile;
        spec = cfg.mayorSpec;
        avatar = cfg.mayorAvatar;
        # Where Nadia's bot looks (mayor_fallen_flag in configFile above) —
        # outside his private state dir, so her service can actually see it.
        triggeredFlag = "/var/lib/larp/mayor-triggered";
      };
  };
}
