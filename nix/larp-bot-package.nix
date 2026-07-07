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

  # Cargo feature-unifies workspace members' dev-deps into every build, so
  # dashchat-node compiles with its `testing` feature (pulled in by larp-e2e)
  # even for `cargo build -p larp-bot`. That module include_str!s a file from
  # its own workspace root, which cargo vendoring drops; the vendored path
  # resolves to $NIX_BUILD_TOP. An empty allowlist satisfies it — the testing
  # module is dead code in the shipped binary.
  preBuild = ''
    echo '[]' > "$NIX_BUILD_TOP/allowed-test-mailbox-url-patterns.json"
  '';

  # The unit tests run in the dev shell / CI shell; the e2e test spawns whole
  # p2panda nodes, which the build sandbox is the wrong place for.
  doCheck = false;

  meta.mainProgram = "larp-bot";
}
