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
          # Reticulum 1.x ships under the (non-OSI) Reticulum License; allow
          # exactly that one package for the RNS gateway.
          config.allowUnfreePredicate = pkg: nixpkgs.lib.getName pkg == "rns";
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
            doctl # journalist droplet recipes (just journalist::*)
            # The RNS gateway (gateway/): its runtime deps for the unit
            # tests, plus rnodeconf for flashing the Heltecs.
            (python3.withPackages (
              ps: with ps; [
                rns
                msgpack
                zeroconf
              ]
            ))
          ];
        };

      packages.x86_64-linux = {
        # The LARP character bot: also the provisioning tool
        # (`nix run .#larp-bot -- keygen/qr/cast`).
        default = self.packages.x86_64-linux.larp-bot;
        larp-bot = (pkgsWithRust "x86_64-linux").callPackage ./nix/larp-bot-package.nix { };
        # The LoRa mailbox gateway (docs/rns-gateway.md) and the RNode
        # firmware flasher it ships with (`just lora::rnode-install`).
        rns-gateway = (pkgsWithRust "x86_64-linux").callPackage ./nix/rns-gateway-package.nix { };
        # The flashable station image (aarch64 build; needs binfmt emulation
        # on an x86_64 builder, same as the mailbox image).
        sdImage = self.nixosConfigurations.larp-station.config.system.build.sdImage;
        # The base-station variant: mAP lite as the AP, Pi wired behind it.
        sdImage-base-station = self.nixosConfigurations.base-station.config.system.build.sdImage;
        # The mailbox image's flashing helpers, reused by the just recipes.
        inherit (mailbox-image.packages.x86_64-linux) detect-sd-card flash-sd-image;
      };

      packages.aarch64-linux = {
        default = self.packages.aarch64-linux.larp-bot;
        larp-bot = (pkgsWithRust "aarch64-linux").callPackage ./nix/larp-bot-package.nix { };
        rns-gateway = (pkgsWithRust "aarch64-linux").callPackage ./nix/rns-gateway-package.nix { };
        sdImage = self.nixosConfigurations.larp-station.config.system.build.sdImage;
        sdImage-base-station = self.nixosConfigurations.base-station.config.system.build.sdImage;
        inherit (mailbox-image.packages.aarch64-linux) detect-sd-card flash-sd-image;
      };

      # The bot as a reusable NixOS module — e.g. for the journalist's cloud
      # host, which runs only the bot against the cloud mailbox:
      #
      #   imports = [ internet-shutdown-larp.nixosModules.larp-bot ];
      #   services.larp-bot = {
      #     enable = true;
      #     package = internet-shutdown-larp.packages.x86_64-linux.larp-bot;
      #     scenariosDir = "${internet-shutdown-larp}/scenarios";
      #     mailboxUrl = "<the cloud mailbox URL the players' app uses>";
      #     identityFile = "/var/lib/larp-secrets/journalist-identity.toml";
      #     castFile = "/var/lib/larp-secrets/larp-cast.toml";
      #   };
      nixosModules.larp-bot = ./nix/larp-bot.nix;
      nixosModules.rns-gateway = ./nix/rns-gateway.nix;

      # The journalist's cloud host (docs/design.md §Journalist): a droplet
      # running only the bot against the cloud mailbox. Deployed with
      # `just journalist::deploy` — doctl creates an Ubuntu droplet,
      # nixos-infect converts it in place, and nixos-rebuild pushes this
      # config over SSH.
      nixosConfigurations.journalist-droplet = nixpkgs.lib.nixosSystem {
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
            networking.hostName = "larp-journalist";
            time.timeZone = "Europe/Madrid"; # match the stations' log clock
            system.stateVersion = "25.11";

            services.larp-bot = {
              enable = true;
              package = self.packages.x86_64-linux.larp-bot;
              scenariosDir = ./scenarios;
              # Must match the mailbox the players' app build uses — release
              # builds sync through the production mailbox (docs/design.md).
              mailboxUrl = "https://mailbox.production.darksoil.studio";
              identityFile = "/var/lib/larp-secrets/journalist-identity.toml";
              castFile = "/var/lib/larp-secrets/larp-cast.toml";
            };
          }
        ];
      };

      # The station image: the plain mailbox appliance extended with the
      # character bot. One image serves every station — the bot only starts on
      # cards whose FAT boot partition carries larp-identity.toml +
      # larp-cast.toml (see nix/larp-bot.nix and docs/design.md).
      nixosConfigurations.larp-station = mailbox-image.nixosConfigurations.mailbox-pi.extendModules {
        modules = [
          ./nix/larp-bot.nix
          ./nix/timezone.nix
          ./nix/rns-gateway.nix
          {
            services.larp-bot = {
              enable = true;
              package = self.packages.aarch64-linux.larp-bot;
              scenariosDir = ./scenarios;
            };
            # The LoRa gateway (relative link, docs/rns-gateway.md), gated
            # like the bot: no lora.env on the card → no gateway.
            services.rns-gateway = {
              enable = true;
              package = self.packages.aarch64-linux.rns-gateway;
            };
          }
        ];
      };

      # The base-station image: the station image minus Pi-hosted wifi — a
      # MikroTik mAP lite broadcasts the mesh (a real AP, comfortable with
      # 30-40 clients) and the Pi, wired behind it, owns DHCP/DNS and serves
      # the captive portal + the mailbox (see nix/base-station.nix; the mAP
      # side is provisioned with ../map-lite-portal). The bot stays flashable
      # like any other card.
      nixosConfigurations.base-station = self.nixosConfigurations.larp-station.extendModules {
        modules = [
          ./nix/base-station.nix
          (
            { pkgs, ... }:
            {
              # The mayor's onboarding page (portal/index.html — a single
              # static file, no build step) replaces the mailbox image's
              # generic captive-portal SPA. The module's nginx keeps serving
              # it and proxying /api/ to the mailbox.
              dashchat.captivePortal.package = pkgs.runCommand "mayor-portal" { } ''
                mkdir -p $out
                cp ${./portal/index.html} $out/index.html
              '';
            }
          )
        ];
      };
    };
}
