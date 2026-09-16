# Dependency-free workflow tests and the regeneration apps used by contributors.
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
      inherit (pkgs) lib;
      nixFilter = inputs.nix-filter.lib;
      mkRepoCheck = import ./mk-repo-check.nix { inherit pkgs nixFilter root; };

      rustToolchain = pkgs.rust-bin.fromRustupToolchainFile (root + /rust-toolchain.toml);
      craneLib = (inputs.crane.mkLib pkgs).overrideToolchain rustToolchain;
      inherit (config.packages.ccusage.passthru) commonArgs workspaceArtifacts;
      # Shared with the schema drift check and treefmt. The config layer needs
      # only the foundation artifacts, not another build of every adapter.
      generateConfigSchema = craneLib.buildPackage (
        commonArgs
        // {
          pname = "generate-config-schema";
          cargoArtifacts = workspaceArtifacts.foundation;
          cargoExtraArgs = "-p ccusage-config --bin generate-config-schema";
          doCheck = false;
          meta.mainProgram = "generate-config-schema";
        }
      );

      # These apps intentionally modify the caller's checkout, never the copy
      # of the source used to build the flake. Reject subdirectories and unrelated
      # repositories before generating or copying anything.
      requireCheckoutRoot = ''
        if [[ $# -ne 0 ]]; then
          echo "This app takes no arguments; run it from the ccusage checkout root." >&2
          exit 1
        fi
        if [[ ! -f flake.nix || ! -f package.json || ! -f .oxfmtrc.json ||
              ! -f rust/Cargo.toml || ! -f apps/ccusage/package.json ||
              ! -f rust/crates/ccusage-config/src/config_schema.rs || ! -w . ]]; then
          echo "Run this app from a writable ccusage checkout root." >&2
          exit 1
        fi
        requireWritableTarget() {
          if [[ ! -w "$(dirname "$1")" || ( -e "$1" && ! -w "$1" ) ]]; then
            echo "Cannot update non-writable checkout file: $1" >&2
            exit 1
          fi
        }
      '';
      schemaGen = pkgs.callPackage ./schema-gen.nix { inherit generateConfigSchema; };
      generateSchema = pkgs.writeShellApplication {
        name = "generate-schema";
        runtimeInputs = [ pkgs.coreutils ];
        text = ''
          ${requireCheckoutRoot}
          targets=(apps/ccusage/config-schema.json)
          if [[ -d docs/public ]]; then
            targets+=(docs/public/config-schema.json)
          fi
          for target in "''${targets[@]}"; do
            requireWritableTarget "$target"
          done

          exec ${lib.getExe schemaGen}
        '';
      };

      modelsDevSnapshots = [
        {
          name = "models-dev-pricing.json";
          target = "rust/crates/ccusage-core/src/models-dev-pricing.json";
        }
        {
          name = "models-dev-catalog-rules.json";
          target = "rust/crates/ccusage-core/src/models-dev-catalog-rules.json";
        }
        {
          name = "codex-auto-review-fallbacks.json";
          target = "rust/adapters/codex/src/codex-auto-review-fallbacks.json";
        }
      ];
      snapshotTargets = lib.escapeShellArgs (map (snapshot: snapshot.target) modelsDevSnapshots);
      generateModelsDevPricing = pkgs.writeShellApplication {
        name = "generate-models-dev-pricing";
        runtimeInputs = [
          pkgs.coreutils
          pkgs.oxfmt
        ];
        text = ''
          ${requireCheckoutRoot}
          for target in ${snapshotTargets}; do
            requireWritableTarget "$target"
          done
          ${lib.concatMapStringsSep "\n" (snapshot: ''
            install -m 644 ${config.packages.models-dev-pricing}/${snapshot.name} ${lib.escapeShellArg snapshot.target}
          '') modelsDevSnapshots}
          oxfmt --config .oxfmtrc.json --write ${snapshotTargets}
        '';
      };
    in
    {
      packages.generate-config-schema = generateConfigSchema;

      apps = {
        generate-schema = {
          type = "app";
          program = lib.getExe generateSchema;
        };
        generate-models-dev-pricing = {
          type = "app";
          program = lib.getExe generateModelsDevPricing;
        };
      };

      checks = {
        # These suites use only Node built-ins. In particular, do not install the
        # pnpm workspace or add node_modules to the sandbox just to run these.
        node-tests = mkRepoCheck "node-tests" [ pkgs.nodejs ] ''
          TZ=UTC node --test apps/ccusage/src/cli.test.ts nix/tools/models-dev-gen/compact.test.ts .github/scripts/publish-release.test.cjs
        '';
        performance-harness = mkRepoCheck "performance-harness" [ pkgs.babashka ] ''
          # Calling bb directly bypasses the script's network-capable nix shebang.
          bb apps/ccusage/scripts/compare-pr-performance_test.clj
        '';
        tirith = mkRepoCheck "tirith-check" [ pkgs.tirith ] ''
          # Snapshot fixtures intentionally contain ANSI escape sequences.
          # Exit 2 means findings exist, but all are below the high threshold.
          code=0
          tirith scan --ci --fail-on high --exclude '*.snap' ./ || code=$?
          case "$code" in
            0 | 2) ;;
            *) exit "$code" ;;
          esac
        '';
        models-dev-snapshots =
          mkRepoCheck "models-dev-snapshots-check"
            [
              pkgs.jq
              pkgs.diffutils
            ]
            (
              lib.concatMapStringsSep "\n" (snapshot: ''
                # Compare parsed, key-sorted JSON rather than formatter whitespace.
                jq --sort-keys . ${lib.escapeShellArg snapshot.target} > committed.json
                jq --sort-keys . ${config.packages.models-dev-pricing}/${snapshot.name} > generated.json
                if ! diff -u --label ${lib.escapeShellArg snapshot.target} --label generated/${snapshot.name} committed.json generated.json; then
                  echo "ERROR: ${snapshot.target} is out of sync with the pinned models.dev input." >&2
                  echo "Run 'nix run .#generate-models-dev-pricing' and commit all three snapshots." >&2
                  exit 1
                fi
              '') modelsDevSnapshots
            );
      };
    };
}
