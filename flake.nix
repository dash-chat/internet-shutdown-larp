{
  description = "Internet-shutdown LARP: Dash Chat character bots + station images (see docs/design.md)";

  nixConfig = {
    extra-substituters = [
      "https://dash-chat.cachix.org"
      "https://nixos-raspberrypi.cachix.org"
    ];
    extra-trusted-public-keys = [
      "dash-chat.cachix.org-1:oAsoaEZ7e4UJlveRXF45MJ1P+Tf3OKFN5QkB8BuPaiM="
      "nixos-raspberrypi.cachix.org-1:4iMO9LXa8BqhU+Rpg6LQKiGa2lsNh/j2oiYLNOQ5sPI="
    ];
  };

  inputs = {
    # The plain AP + mailbox Raspberry Pi image this repo extends with the
    # character bot. For local development:
    #   nix flake update mailbox-image --override-input mailbox-image path:../raspberry-pi-mailbox-server
    mailbox-image.url = "github:dash-chat/raspberry-pi-mailbox-server";

    # Reuse the nixpkgs the image is built against.
    nixpkgs.follows = "mailbox-image/nixpkgs";

    # The dash-chat crate tree wants Rust 1.94, newer than the pinned
    # nixpkgs' rustc.
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    {
      self,
      mailbox-image,
      nixpkgs,
      rust-overlay,
      ...
    }:
    let
      pkgsWithRust =
        system:
        import nixpkgs {
          inherit system;
          overlays = [ rust-overlay.overlays.default ];
        };
    in
    {
      devShells.x86_64-linux.default =
        let
          pkgs = pkgsWithRust "x86_64-linux";
          # Match dash-chat's rust-toolchain.toml. Minimal profile: skips the
          # hefty rust-docs component.
          rustToolchain = pkgs.rust-bin.stable."1.94.0".minimal.override {
            extensions = [
              "rust-src"
              "clippy"
              "rustfmt"
            ];
          };
        in
        pkgs.mkShell {
          packages = with pkgs; [
            just # provisioning + flashing recipes
            zstd # decompress the built .img.zst
            rustToolchain # larp-bot workspace (crates/)
            pkg-config # native deps of the dash-chat crate tree
            openssl
            doctl # sister droplet recipes (the out-of-town character) (just sister::*)
            nmap # deploy-all's station sweep — in the shell, not `nix run`,
            # because the game wifi has no internet to fetch it from
          ];
        };

      packages.x86_64-linux = {
        # The LARP character bot: also the provisioning tool
        # (`nix run .#larp-bot -- keygen/qr/cast`).
        default = self.packages.x86_64-linux.larp-bot;
        larp-bot = (pkgsWithRust "x86_64-linux").callPackage ./nix/larp-bot-package.nix { };
        # The flashable station image (aarch64 build; needs binfmt emulation
        # on an x86_64 builder, same as the mailbox image). There is only one:
        # the base station runs this image too, and differs only in the files
        # its card is flashed with.
        sdImage = self.nixosConfigurations.larp-station.config.system.build.sdImage;
        # The mailbox image's flashing + cable-debugging helpers, reused by
        # the just recipes.
        inherit (mailbox-image.packages.x86_64-linux)
          detect-sd-card
          flash-sd-image
          find-pi
          ethernet-ssh
          ethernet-set-time
          ethernet-deploy
          ;
      };

      packages.aarch64-linux = {
        default = self.packages.aarch64-linux.larp-bot;
        larp-bot = (pkgsWithRust "aarch64-linux").callPackage ./nix/larp-bot-package.nix { };
        sdImage = self.nixosConfigurations.larp-station.config.system.build.sdImage;
        inherit (mailbox-image.packages.aarch64-linux)
          detect-sd-card
          flash-sd-image
          find-pi
          ethernet-ssh
          ethernet-set-time
          ethernet-deploy
          ;
      };

      # The bot as a reusable NixOS module — e.g. for the out-of-town character's cloud
      # host, which runs only the bot against the cloud mailbox:
      #
      #   imports = [ internet-shutdown-larp.nixosModules.larp-bot ];
      #   services.larp-bot = {
      #     enable = true;
      #     package = internet-shutdown-larp.packages.x86_64-linux.larp-bot;
      #     scenariosDir = "${internet-shutdown-larp}/scenarios";
      #     mailboxUrl = "<the cloud mailbox URL the players' app uses>";
      #     identityFile = "/var/lib/larp-secrets/sister-identity.toml";
      #     castFile = "/var/lib/larp-secrets/larp-cast.toml";
      #   };
      nixosModules.larp-bot = ./nix/larp-bot.nix;

      # CURRENTLY UNUSED (kept, see sister.just): Mira moved into town onto her
      # own Pi station, so no character runs off-map any more. This is the
      # cloud host she used to run on: a droplet running only the bot against
      # the cloud mailbox, deployed with `just sister::deploy` — doctl creates
      # an Ubuntu droplet, nixos-infect converts it in place, and nixos-rebuild
      # pushes this config over SSH.
      nixosConfigurations.sister-droplet = nixpkgs.lib.nixosSystem {
        system = "x86_64-linux";
        modules = [
          # Boot, SSH keys + hostname from droplet metadata, do-agent.
          "${nixpkgs}/nixos/modules/virtualisation/digital-ocean-config.nix"
          ./nix/larp-bot.nix
          {
            # nixos-infect keeps the Ubuntu root filesystem, so the DO image
            # module's by-label device doesn't exist — use the partition.
            fileSystems."/" = {
              device = "/dev/vda1";
              fsType = "ext4";
            };
            networking.hostName = "larp-sister";
            # nixos-infect leaves DNS to DHCP; when that hands nothing over
            # the bot dies with "dns error" on every mailbox sync. Pin DO's
            # resolvers with a public fallback.
            networking.nameservers = [
              "67.207.67.2"
              "67.207.67.3"
              "1.1.1.1"
            ];
            time.timeZone = "Europe/Madrid"; # match the stations' log clock
            system.stateVersion = "25.11";

            services.larp-bot = {
              enable = true;
              package = self.packages.x86_64-linux.larp-bot;
              scenariosDir = ./scenarios;
              # Must match the mailbox the players' app build uses — release
              # builds sync through the production mailbox (docs/design.md).
              # Same URL as dash-chat's PRODUCTION_MAILBOX_URL: plain http,
              # the server has no TLS listener on 443.
              mailboxUrl = "http://mailbox.darksoil.studio";
              identityFile = "/var/lib/larp-secrets/sister-identity.toml";
              castFile = "/var/lib/larp-secrets/larp-cast.toml";
            };
          }
        ];
      };

      # The station image: the plain mailbox appliance extended with the
      # character bot. ONE image serves every station, base station included —
      # which of the three bots run is decided entirely by the files a flash
      # recipe puts on the FAT boot partition (see nix/larp-bot.nix and
      # docs/design.md):
      #
      #   larp-identity.toml + larp-cast.toml → the character bot
      #   larp-anonymous.toml                 → the informant (sister's card only)
      #   larp-mayor.toml                     → the mayor (base station only)
      #
      # No captive portal anywhere: the mayor moved into Dash Chat, so the
      # game's wifi looks like a dead network and the app finds the mailbox
      # over mDNS on its own port.
      nixosConfigurations.larp-station = mailbox-image.nixosConfigurations.mailbox-pi.extendModules {
        modules = [
          ./nix/larp-bot.nix
          ./nix/timezone.nix
          (
            { ... }:
            {
              services.larp-bot = {
                enable = true;
                package = self.packages.aarch64-linux.larp-bot;
                scenariosDir = ./scenarios;
                # Arms the informant service on every station card; it only
                # starts where a flash recipe wrote the anonymous identity —
                # the sister's card alone (characters.just), since Mira is
                # the one who hands out his contact link.
                anonymousSpec = ./anonymous.toml;
                anonymousAvatar = ./anonymous.png;
                # Arms the mayor the same way. Only base-station.just writes
                # his identity, so he runs on exactly one card.
                mayorSpec = ./mayor.toml;
                mayorAvatar = ./mayor.png;
              };

              # No wifi overrides here any more. The mailbox image dropped AP
              # mode altogether (the Pi's brcmfmac AP was the main source of
              # field failures), taking the range limiting with it: a station
              # now JOINS the game's wifi as a plain client, reading SSID and
              # password at boot from wifi.env on the FAT boot partition
              # (nix/wifi-client.nix upstream, written by the flash recipes).
              # An AP per station broadcasts it — all named OfflineWifi and
              # open, so a player's phone re-joins by itself at every stop.
              # The wait for the wireless IPv4 before the mailbox's one-shot
              # mDNS announcement also lives in the mailbox image
              # (appliance.nix, added 2026-07-10).
            }
          )
        ];
      };
    };
}
