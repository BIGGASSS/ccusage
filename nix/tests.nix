# Rust workspace test run as a Nix derivation so it shares the crane
# `cargoArtifacts` cache (warm on the per-job Blacksmith sticky disk) instead of
# recompiling into an uncached `rust/target` on every CI run. Included in
# `nix flake check` and also available as `nix build .#ccusage-tests`; the same
# derivation succeeds only when `cargo test --workspace` passes.
{ inputs, ... }:
let
  root = ./..;
in
{
  perSystem =
    {
      config,
      pkgs,
      ...
    }:
    let
      rustToolchain = pkgs.rust-bin.fromRustupToolchainFile (root + /rust-toolchain.toml);
      craneLib = (inputs.crane.mkLib pkgs).overrideToolchain rustToolchain;
      inherit (config.packages.ccusage.passthru) cargoArtifacts commonArgs;
      nixFilter = inputs.nix-filter.lib;
      testSrc = import ./rust-src.nix { inherit nixFilter root; };
    in
    {
      checks.ccusage-tests = config.packages.ccusage-tests;
      packages.ccusage-tests = craneLib.cargoTest (
        commonArgs
        // {
          src = testSrc;
          sourceRoot = "source/rust";
          cargoLock = root + /rust/Cargo.lock;
          inherit cargoArtifacts;
          # commonArgs disables checks for the release build; re-enable here so
          # cargoTest actually runs the test suite rather than skipping it.
          doCheck = true;
          cargoExtraArgs = "--workspace";
          # jiff resolves named time zones (e.g. Asia/Tokyo) from the system
          # zoneinfo database, which the hermetic build sandbox lacks, so it
          # would fall back to UTC and shift the timezone-dependent tests by a
          # day. Point it at the nixpkgs tzdata; referencing the store path here
          # also pulls it into the sandbox as a build input.
          TZDIR = "${pkgs.tzdata}/share/zoneinfo";
          # The workspace tests resolve default Claude data directories from
          # $HOME; the sandbox has none, so seed a writable HOME with the same
          # empty directory layout the CI test job creates before running tests.
          preBuild = ''
            export HOME=$(mktemp -d)
            mkdir -p "$HOME/.claude/projects" "$HOME/.config/claude/projects"
          '';
        }
      );
    };
}
