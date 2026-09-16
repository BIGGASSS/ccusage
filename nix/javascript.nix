# The workspace lockfile is fetched once as a fixed-output derivation. Every
# consumer installs offline with lifecycle scripts disabled; entering the shell
# never runs a package manager or changes node_modules.
{ inputs, ... }:
let
  root = ./..;
in
{
  perSystem =
    { pkgs, system, ... }:
    let
      inherit (pkgs) lib;
      workspaceDirs = [
        "apps/ccusage"
        "docs"
        "packages/ccusage-darwin-arm64"
        "packages/ccusage-darwin-x64"
        "packages/ccusage-linux-arm64"
        "packages/ccusage-linux-x64"
      ];
      manifests = lib.fileset.toSource {
        inherit root;
        fileset = lib.fileset.unions (
          [
            (root + /package.json)
            (root + /pnpm-lock.yaml)
            (root + /pnpm-workspace.yaml)
          ]
          ++ map (dir: root + "/${dir}/package.json") workspaceDirs
        );
      };
      pnpmDeps = pkgs.fetchPnpmDeps {
        pname = "ccusage-workspace";
        src = manifests;
        inherit (pkgs) pnpm;
        fetcherVersion = 4;
        hash = "sha256-7OSfxYDcwE6V1Y+6OpsuD0IK4ZaT8nm/ICsqCvZ8+qc=";
      };
      nixFilter = inputs.nix-filter.lib;
      src = nixFilter {
        inherit root;
        include = [
          "package.json"
          "pnpm-lock.yaml"
          "pnpm-workspace.yaml"
          ".oxlintrc.json"
          ".github/scripts"
          "apps/ccusage"
          "docs"
          "packages"
          "nix/tools/models-dev-gen"
        ];
        exclude = map nixFilter.matchName [
          "node_modules"
          "dist"
          "cache"
          ".temp"
          "target"
          "coverage"
          "bin"
          # A local dev server/build creates this; always copy the tracked schema.
          "config-schema.json"
        ];
      };
      bunNodeModules = pkgs.callPackage ./bun-node-modules.nix {
        bun2nix = inputs.bun2nix.packages.${system}.default;
      };
      modelsDevModules = bunNodeModules { toolDir = ./tools/models-dev-gen; };
      workspace = pkgs.stdenvNoCC.mkDerivation {
        pname = "ccusage-js-workspace";
        version = "0.0.0";
        inherit src pnpmDeps;
        nativeBuildInputs = [
          pkgs.nodejs
          pkgs.pnpm
          pkgs.pnpmConfigHook
        ]
        ++ lib.optionals pkgs.stdenv.isLinux [ pkgs.autoPatchelfHook ];
        buildInputs = lib.optionals pkgs.stdenv.isLinux [
          pkgs.stdenv.cc.cc.lib
          pkgs.zlib
        ];
        dontBuild = true;
        # Retain the pnpm workspace layout: its links are relative to the root.
        installPhase = ''
          runHook preInstall
          # These pnpm install caches contain timestamps and a temporary store
          # path. Consumers run the installed tools, never pnpm install against
          # this tree, so retain the dependencies and links but not the caches.
          rm node_modules/.modules.yaml node_modules/.pnpm-workspace-state-v1.json
          cp ${root + /apps/ccusage/config-schema.json} apps/ccusage/config-schema.json
          cp apps/ccusage/config-schema.json docs/public/config-schema.json
          cp -R ${modelsDevModules}/node_modules nix/tools/models-dev-gen/node_modules
          mkdir -p "$out"
          cp -R . "$out/"
          runHook postInstall
        '';
      };
      docs = pkgs.runCommand "ccusage-docs" { nativeBuildInputs = [ pkgs.nodejs ]; } ''
        # Nix's build PTY can report zero columns. Disable VitePress's spinner:
        # ora otherwise computes an infinite line count and spins clearing it.
        export HOME="$TMPDIR/home" TZ=UTC CI=1
        mkdir -p "$HOME"
        cp -R ${workspace} source
        chmod -R u+w source
        cd source/docs
        node node_modules/vitepress/bin/vitepress.js build
        cp -R .vitepress/dist "$out"
      '';
      install = pkgs.writeShellApplication {
        name = "js-install";
        runtimeInputs = [ pkgs.coreutils ];
        text = ''
          if [[ $# -ne 0 || ! -f flake.nix || ! -f pnpm-lock.yaml || ! -d apps/ccusage || ! -d docs ]]; then
            echo 'Run nix run .#js-install from the ccusage checkout root (no arguments).' >&2
            exit 1
          fi
          # This is deliberately explicit and only replaces dependency directories,
          # never source files, lockfiles or manifests. No registry is contacted.
          for dir in . ${lib.escapeShellArgs workspaceDirs} nix/tools/models-dev-gen; do
            # Also clear stale dependencies in packages which no longer need any.
            rm -rf -- "$dir/node_modules"
            if [[ -d "${workspace}/$dir/node_modules" ]]; then
              mkdir -p "$dir"
              cp -R "${workspace}/$dir/node_modules" "$dir/node_modules"
              chmod -R u+w "$dir/node_modules"
            fi
          done
        '';
      };
      docsDev = pkgs.writeShellApplication {
        name = "docs-dev";
        runtimeInputs = [
          pkgs.nodejs
          pkgs.coreutils
        ];
        text = ''
          ${lib.getExe install}
          cp apps/ccusage/config-schema.json docs/public/config-schema.json
          exec node docs/node_modules/vitepress/bin/vitepress.js dev docs "$@"
        '';
      };
      docsPreview = pkgs.writeShellApplication {
        name = "docs-preview";
        runtimeInputs = [
          pkgs.nodejs
          pkgs.coreutils
        ];
        text = ''
          # Vite bundles the config beside its source, so it cannot load it
          # directly from the read-only store. Only the config/source copy is
          # writable; dependencies and the built site stay in the store.
          previewRoot="$(mktemp -d -t ccusage-docs-preview.XXXXXXXX)"
          trap 'rm -rf -- "$previewRoot"' EXIT
          cp -R ${workspace}/docs "$previewRoot/docs"
          chmod -R u+w "$previewRoot/docs"
          ln -s ${workspace}/node_modules "$previewRoot/node_modules"
          # VitePress preview reads the configured dist directory, not --outDir.
          ln -s ${docs} "$previewRoot/docs/.vitepress/dist"
          node ${workspace}/docs/node_modules/vitepress/bin/vitepress.js preview "$previewRoot/docs" "$@"
        '';
      };
    in
    {
      packages = {
        js-deps = pnpmDeps;
        js-workspace = workspace;
        inherit docs;
      };
      checks = {
        inherit docs;
        js-typecheck =
          pkgs.runCommand "ccusage-js-typecheck"
            {
              nativeBuildInputs = [
                pkgs.oxlint
                pkgs.nodejs
              ];
            }
            ''
              cp -R ${workspace} source
              chmod -R u+w source
              cd source
              oxlint .
              touch "$out"
            '';
      };
      apps = {
        js-install = {
          type = "app";
          program = lib.getExe install;
        };
        docs-dev = {
          type = "app";
          program = lib.getExe docsDev;
        };
        docs-preview = {
          type = "app";
          program = lib.getExe docsPreview;
        };
      };
    };
}
