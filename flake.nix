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
          ];
        };

      packages.x86_64-linux = {
        # The LARP character bot: also the provisioning tool
        # (`nix run .#larp-bot -- keygen/qr/cast`).
        default = self.packages.x86_64-linux.larp-bot;
        larp-bot = (pkgsWithRust "x86_64-linux").callPackage ./nix/larp-bot-package.nix { };
        # The flashable station image (aarch64 build; needs binfmt emulation
        # on an x86_64 builder, same as the mailbox image).
        sdImage = self.nixosConfigurations.larp-station.config.system.build.sdImage;
        # The base-station variant: mAP lite as the AP, Pi wired behind it.
        sdImage-base-station = self.nixosConfigurations.base-station.config.system.build.sdImage;
      };

      packages.aarch64-linux = {
        default = self.packages.aarch64-linux.larp-bot;
        larp-bot = (pkgsWithRust "aarch64-linux").callPackage ./nix/larp-bot-package.nix { };
        sdImage = self.nixosConfigurations.larp-station.config.system.build.sdImage;
        sdImage-base-station = self.nixosConfigurations.base-station.config.system.build.sdImage;
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

      # The station image: the plain mailbox appliance extended with the
      # character bot. One image serves every station — the bot only starts on
      # cards whose FAT boot partition carries larp-identity.toml +
      # larp-cast.toml (see nix/larp-bot.nix and docs/design.md).
      nixosConfigurations.larp-station = mailbox-image.nixosConfigurations.mailbox-pi.extendModules {
        modules = [
          ./nix/larp-bot.nix
          {
            services.larp-bot = {
              enable = true;
              package = self.packages.aarch64-linux.larp-bot;
              scenariosDir = ./scenarios;
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
        modules = [ ./nix/base-station.nix ];
      };
    };
}
