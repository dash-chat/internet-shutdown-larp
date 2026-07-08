# The RNS mailbox gateway (gateway/rns_gateway.py) as a runnable package.
# Python sidecar by design — the Rust-native migration path is documented in
# docs/rns-gateway.md. Needs a nixpkgs with `rns` allowed (unfree: Reticulum
# 1.x uses the non-OSI Reticulum License); the flake's pkgs import does that.
{
  python3,
  writeShellScriptBin,
  esptool,
}:
let
  # nixpkgs' rns patch hardcodes esptool store paths into rnodeconf's
  # per-board flasher commands but misses the newer ESP32-S3 boards (Heltec
  # V4, T3S3, T-Beam Supreme, T-Deck, XIAO): their branches still read
  # `sys.executable, flasher`, and `flasher` is left unbound by that same
  # patch — so `rnodeconf --autoinstall` crashes at the flash step with
  # "cannot access local variable 'flasher'". Present in nixpkgs rns 1.0.3
  # and still in 1.2.9; substitute those branches too.
  python3' = python3.override {
    packageOverrides = _self: super: {
      rns = super.rns.overridePythonAttrs (old: {
        postPatch =
          (old.postPatch or "")
          + ''
            substituteInPlace RNS/Utilities/rnodeconf.py \
              --replace-fail "sys.executable, flasher," '"${esptool}/bin/esptool",'
          '';
      });
    };
  };
  py = python3'.withPackages (
    ps: with ps; [
      rns
      msgpack
      zeroconf
    ]
  );
in
(writeShellScriptBin "rns-gateway" ''
  exec ${py}/bin/python3 ${../gateway/rns_gateway.py} "$@"
'').overrideAttrs
  (old: {
    # rnodeconf (ships with rns) flashes RNode firmware onto the Heltecs:
    # exposed here so `just lora::rnode-install` can `nix run` it.
    passthru = (old.passthru or { }) // {
      inherit py;
    };
    meta = (old.meta or { }) // {
      mainProgram = "rns-gateway";
      description = "Dash Chat mailbox gateway over Reticulum/LoRa";
    };
  })
