# The larp-bot binary, built from this repo's Cargo workspace (crates/).
# Callable only from a nixpkgs with rust-overlay applied (needs rust-bin):
# the dash-chat crate tree wants Rust 1.94, newer than the pinned nixpkgs'.
{
  lib,
  makeRustPlatform,
  rust-bin,
  pkg-config,
  openssl,
}:
let
  toolchain = rust-bin.stable."1.94.0".minimal;
  rustPlatform = makeRustPlatform {
    cargo = toolchain;
    rustc = toolchain;
  };
in
rustPlatform.buildRustPackage {
  pname = "larp-bot";
  version = "0.1.0";

  src = lib.fileset.toSource {
    root = ../.;
    fileset = lib.fileset.unions [
      ../Cargo.toml
      ../Cargo.lock
      ../crates
    ];
  };

  cargoLock = {
    lockFile = ../Cargo.lock;
    # Git deps (dash-chat, p2panda fork, iroh-blobs fork, …) are fetched with
    # builtins.fetchGit from the lockfile's pins — no outputHashes to maintain.
    allowBuiltinFetchGit = true;
  };

  cargoBuildFlags = [
    "-p"
    "larp-bot"
  ];

  nativeBuildInputs = [ pkg-config ];
  buildInputs = [ openssl ];

  # The unit tests run in the dev shell / CI shell; the e2e test spawns whole
  # p2panda nodes, which the build sandbox is the wrong place for.
  doCheck = false;

  meta.mainProgram = "larp-bot";
}
