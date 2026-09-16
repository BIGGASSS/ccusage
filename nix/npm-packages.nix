# Finished npm artifacts, not staging directories. No workspace installation,
# lifecycle hooks, registry access, or nested `nix` invocation occurs in a build.
_: {
  perSystem =
    { config, pkgs, ... }:
    let
      inherit (pkgs) lib;
      root = ./..;
      inherit (lib.importJSON (root + /package.json)) version;
      platform = if pkgs.stdenv.isDarwin then "darwin" else "linux";
      arch = if pkgs.stdenv.hostPlatform.isAarch64 then "arm64" else "x64";
      target = "${platform}-${arch}";
      nativeBinary =
        if pkgs.stdenv.isLinux then config.packages.ccusage-static else config.packages.ccusage;
      # Only release files are inputs; tests, node_modules and developer scripts
      # can never accidentally become package contents.
      launcherSrc = lib.fileset.toSource {
        inherit root;
        fileset = lib.fileset.unions [
          (root + /apps/ccusage/src/cli.js)
          (root + /apps/ccusage/config-schema.json)
        ];
      };
      manifest =
        path:
        let
          original = lib.removeAttrs (lib.importJSON path) [
            "scripts"
            "devDependencies"
            "packageManager"
            "private"
          ];
          release = builtins.mapAttrs (
            key: value:
            if
              builtins.elem key [
                "dependencies"
                "optionalDependencies"
                "peerDependencies"
              ]
            then
              builtins.mapAttrs (_: spec: if spec == "workspace:*" then version else spec) value
            else
              value
          ) original;
        in
        pkgs.writeText "npm-package.json" (builtins.toJSON (release // { inherit version; }));
      npmEnvironment = ''
        export HOME="$TMPDIR/home"
        mkdir -p "$HOME"
        export npm_config_cache="$TMPDIR/npm-cache"
        export npm_config_userconfig="$TMPDIR/npmrc" npm_config_globalconfig="$TMPDIR/global-npmrc"
        touch "$npm_config_userconfig" "$npm_config_globalconfig"
        export npm_config_update_notifier=false npm_config_audit=false npm_config_fund=false
        export SOURCE_DATE_EPOCH=499162500 TZ=UTC
      '';
      pack =
        name: packageManifest: stage:
        pkgs.runCommand "${name}-${version}-npm"
          {
            nativeBuildInputs = [
              pkgs.nodejs
              pkgs.nushell
              config.packages.publint
            ]
            ++ lib.optionals pkgs.stdenv.isLinux [ pkgs.binutils ]
            ++ lib.optionals pkgs.stdenv.isDarwin [ pkgs.darwin.cctools ];
          }
          ''
            ${npmEnvironment}
            mkdir -p package "$out" repack
            cp ${packageManifest} package/package.json
            cp ${root + /apps/ccusage/LICENSE} package/LICENSE
            cp ${root + /apps/ccusage/README.md} package/README.md
            chmod 644 package/*
            ${stage}
            cd package
            publint run . --pack false
            npm pack --offline --ignore-scripts --pack-destination "$out"
            # npm uses portable tar headers, ordering, and a fixed timestamp.
            # Assert reproducibility even when source mtimes change.
            find . -type f -exec touch -t 200001010000 {} +
            npm pack --offline --ignore-scripts --pack-destination ../repack
            cmp "$out"/*.tgz ../repack/*.tgz
          '';
      launcher = pack "ccusage" (manifest (root + /apps/ccusage/package.json)) ''
        cp -R ${launcherSrc}/apps/ccusage/src package/src
        cp ${launcherSrc}/apps/ccusage/config-schema.json package/config-schema.json
        chmod 644 package/config-schema.json
        chmod 755 package/src/cli.js
        # Do not patch this shebang: the tarball runs on machines without Nix.
        test "$(head -n 1 package/src/cli.js)" = '#!/usr/bin/env node'
      '';
      mkNative =
        nativeTarget: binary:
        pack "ccusage-${nativeTarget}" (manifest (root + "/packages/ccusage-${nativeTarget}/package.json"))
          ''
            mkdir -p package/bin
            cp ${binary}/bin/ccusage package/bin/ccusage
            chmod 755 package/bin/ccusage
            (cd package && nu ${root + /apps/ccusage/scripts/verify-native-package.nu})
            ${
              if pkgs.stdenv.isLinux then
                ''
                  readelf -h package/bin/ccusage > elf-header
                  grep -q 'Machine:.*${
                    if lib.hasSuffix "arm64" nativeTarget then "AArch64" else "Advanced Micro Devices X86-64"
                  }' elf-header
                  readelf -l package/bin/ccusage > elf-program-headers
                  readelf -d package/bin/ccusage > elf-dynamic
                  # Reject both an ELF interpreter and any shared-library imports.
                  if grep -q INTERP elf-program-headers || grep -q '(NEEDED)' elf-dynamic; then
                    echo 'npm Linux binaries must be fully static' >&2
                    exit 1
                  fi
                ''
              else
                ''
                  test "$(lipo -archs package/bin/ccusage)" = '${
                    if lib.hasSuffix "arm64" nativeTarget then "arm64" else "x86_64"
                  }'
                  # Use pinned cctools, not the runner's Xcode. Never ship Nix dylibs.
                  otool -L package/bin/ccusage > dylibs
                  if tail -n +2 dylibs | awk '{print $1}' | grep -Ev '^(/usr/lib/|/System/Library/)'; then
                    echo 'npm Darwin binaries may only link system dylibs' >&2
                    exit 1
                  fi
                ''
            }
          '';
      native = mkNative target nativeBinary;
      smoke =
        pkgs.runCommand "ccusage-npm-installed-smoke-${version}"
          {
            nativeBuildInputs = [
              pkgs.nodejs
              pkgs.pnpm
              pkgs.nushell
            ];
          }
          ''
            ${npmEnvironment}
            mkdir consumer
            cd consumer
            # pnpm's explicit ignore list avoids even resolving the other optional
            # platforms. npm --omit=optional still tries to resolve their versions,
            # which are not on the registry before release. Both dependencies below
            # are real, unmodified finished tarballs installed into an empty store.
            cat > pnpm-workspace.yaml <<'YAML'
            ignoredOptionalDependencies:
              - '@ccusage/ccusage-*'
            YAML
            cat > package.json <<'JSON'
            ${builtins.toJSON {
              private = true;
              dependencies = {
                ccusage = "file:${launcher}/ccusage-${version}.tgz";
                "@ccusage/ccusage-${target}" = "file:${native}/ccusage-ccusage-${target}-${version}.tgz";
              };
            }}
            JSON
            pnpm install --offline --ignore-scripts --store-dir "$TMPDIR/pnpm-store" \
              --config.manage-package-manager-versions=false --config.update-notifier=false
            # Assert exact version, native package resolution, and the real bin link.
            node - <<'JS'
            const assert = require('node:assert/strict');
            const { createRequire } = require('node:module');
            const fs = require('node:fs');
            const path = require('node:path');
            const pkg = require('./node_modules/ccusage/package.json');
            assert.equal(pkg.version, '${version}');
            for (const value of Object.values(pkg.optionalDependencies)) assert.equal(value, '${version}');
            assert.equal(Object.keys(pkg.optionalDependencies).length, 4);
            const fromLauncher = createRequire(path.resolve('node_modules/ccusage/src/cli.js'));
            const binary = fromLauncher.resolve('@ccusage/ccusage-${target}/bin/ccusage');
            assert.equal(fs.realpathSync(binary), fs.realpathSync('node_modules/@ccusage/ccusage-${target}/bin/ccusage'));
            assert.ok(fs.statSync(binary).mode & 0o111);
            JS
            test "$(./node_modules/.bin/ccusage --version)" = 'ccusage ${version}'
            nu ${root + /.github/scripts/generate-e2e-fixture.nu} "$TMPDIR/fixture"
            export CLAUDE_CONFIG_DIR="$TMPDIR/fixture" LOG_LEVEL=0
            for command in daily monthly session; do
              ./node_modules/.bin/ccusage "$command" --offline
              ./node_modules/.bin/ccusage "$command" --offline --json > report.json
              node - <<'JS'
            const assert = require('node:assert/strict');
            const report = JSON.parse(require('node:fs').readFileSync('report.json', 'utf8'));
            assert.equal(report.totals.inputTokens, 126000);
            assert.equal(report.totals.totalTokens, 222600);
            JS
            done
            touch "$out"
          '';
      tarballs = pkgs.runCommand "ccusage-npm-tarballs-${version}" { } ''
        test -f ${smoke}
        mkdir -p "$out"
        cp ${launcher}/*.tgz ${native}/*.tgz "$out/"
      '';
      # Not all nixpkgs revisions have tagpr. Keep the fallback source and Go
      # modules pinned too; prefer the distribution's package when available.
      tagpr =
        pkgs.tagpr or (pkgs.buildGoModule {
          pname = "tagpr";
          version = "1.20.1";
          src = pkgs.fetchFromGitHub {
            owner = "Songmu";
            repo = "tagpr";
            rev = "d1b8138b7a31075141b6cd64103de9485ced7ac9";
            hash = "sha256-9V3DR90ic42tfq5vGrO79wBnKqqaOR7Ld+pDOxSGVUo=";
          };
          vendorHash = "sha256-e4IOjepxPvk3Sas0GLRw6wHeL+Z76lBN79oqiR27seM=";
          subPackages = [ "cmd/tagpr" ];
          # Upstream integration tests call GitHub and manipulate Git repositories.
          doCheck = false;
          ldflags = [
            "-s"
            "-w"
          ];
          meta.mainProgram = "tagpr";
        });
      tagprApp = pkgs.writeShellApplication {
        name = "tagpr";
        runtimeInputs = [
          pkgs.git
          tagpr
        ];
        # tagpr itself appends tag=<version> to GITHUB_OUTPUT only when it
        # actually tags a release; never infer a release from `git describe`.
        text = ''exec ${lib.getExe tagpr} "$@"'';
      };
      publishScripts = lib.fileset.toSource {
        root = root + /.github/scripts;
        fileset = lib.fileset.unions [
          (root + /.github/scripts/verify-release-manifests.cjs)
          (root + /.github/scripts/publish-release.cjs)
        ];
      };
      publish = pkgs.writeShellApplication {
        name = "npm-publish";
        runtimeInputs = [
          pkgs.nodejs
          pkgs.coreutils
          pkgs.gnutar
          pkgs.gzip
        ];
        text = ''
          mode=()
          if [ "''${1-}" = '--manual' ]; then
            mode=(--manual)
            shift
          fi
          if [ "$#" -ne 1 ] || [ -z "''${1-}" ] || [[ "$1" == -* ]]; then
            echo 'usage: nix run .#npm-publish -- [--manual] <finished-tarball-directory>' >&2
            exit 1
          fi
          artifacts="$(realpath "$1")"
          tmp="$(mktemp -d)"
          trap 'rm -rf "$tmp"' EXIT
          # Extract the exact five expected manifests, including on retries.
          # The helper validates all of them and hashes all tarballs before any
          # registry request, then publishes missing native packages first.
          names=(ccusage-ccusage-linux-arm64 ccusage-ccusage-linux-x64
                 ccusage-ccusage-darwin-arm64 ccusage-ccusage-darwin-x64 ccusage)
          for name in "''${names[@]}"; do
            tar -xOf "$artifacts/$name-${version}.tgz" package/package.json > "$tmp/$name.json"
          done
          cd "$tmp"
          # npm 11 supports GitHub OIDC trusted publishing directly, without
          # setup-node or a token-bearing .npmrc. --manual disables provenance
          # for local publishing with the caller's own npm credentials. All registry
          # IO is runtime only; the helper skips only byte-identical published versions.
          node ${publishScripts}/publish-release.cjs "''${mode[@]}" "$artifacts" "$tmp" '${version}'
        '';
      };
    in
    {
      packages = {
        npm-launcher = launcher;
        npm-native = native;
        npm-tarballs = tarballs;
        inherit tagpr;
      }
      // lib.optionalAttrs pkgs.stdenv.isDarwin {
        # Cross-produced on arm64; structural/portability validation is performed
        # during packing, but it is deliberately not executed on the build host.
        npm-darwin-x64 = mkNative "darwin-x64" config.packages.ccusage-darwin-x64;
      };
      checks.npm-smoke = smoke;
      apps = {
        npm-publish = {
          type = "app";
          program = lib.getExe publish;
        };
        tagpr = {
          type = "app";
          program = lib.getExe tagprApp;
        };
      };
    };
}
