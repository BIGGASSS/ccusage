# Contributing

This fork accepts bug reports, feature proposals, and pull requests directly. No contributor approval is required.

- For bugs, include the ccusage version, reproduction steps, and expected behavior. Remove private information from logs.
- Keep changes focused. Explain the problem, your approach, and how you tested it.
- Discuss substantial changes in an issue before investing in an implementation.
- Review your changes and add regression tests for behavior changes.

## Prerequisites

Install [Nix](https://nixos.org/download/) with the `nix-command` and `flakes` experimental features enabled and use sandboxed builds. Git is needed to work on the checkout. [direnv](https://direnv.net/) with [nix-direnv](https://github.com/nix-community/nix-direnv) is optional; host Node, pnpm, Bun, and Rust installations are not required.

npm releases support four targets. The flake builds on three host systems; Intel macOS packages are cross-built on Apple Silicon because the locked nixpkgs no longer supports native Intel macOS development:

| Operating system | Architecture  | Nix system                      | npm target   |
| ---------------- | ------------- | ------------------------------- | ------------ |
| Linux            | x86-64        | `x86_64-linux`                  | linux-x64    |
| Linux            | ARM64         | `aarch64-linux`                 | linux-arm64  |
| macOS            | Intel         | Cross-build on `aarch64-darwin` | darwin-x64   |
| macOS            | Apple Silicon | `aarch64-darwin`                | darwin-arm64 |

On Apple Silicon, `nix build .#npm-darwin-x64` produces the Intel macOS tarball. Intel Mac users can run that npm release, but cannot use this flake's development shell natively.

Native Windows builds and npm packages are unsupported. On Windows, use WSL with a supported Linux environment and install Nix and ccusage inside it.

A cold Nix invocation needs network access to fetch locked sources, dependency archives, toolchains, and available binary-cache artifacts. Fixed-output dependency fetches happen separately from the sandboxed builds and tests; those builds do not resolve dependencies from registries or download tools. Once the required closure is available locally, it can be reused offline. This does not make a fresh checkout an offline installation.

## Development

The CLI is Rust-first: agent adapters live in `rust/adapters/`, shared crates in `rust/crates/`, and the npm launcher and packaging in `apps/ccusage/`.

Run these commands from the repository root; entering a development shell is not required:

```sh
nix flake show                          # List flake outputs
nix build .#ccusage .#docs .#npm-tarballs  # Native CLI, docs, and host npm tarballs
nix flake check                         # Every flake check, including npm smoke tests
nix fmt                                 # Pinned formatters and generators
```

For narrower builds, select an individual output, such as `nix build .#ccusage`, `nix build .#docs`, or `nix build .#js-workspace`. `npm-tarballs` contains the launcher and the native package for the host platform, not all four native packages.

Type-check or run specific test suites without running every check:

```sh
system=$(nix eval --impure --raw --expr builtins.currentSystem)
nix build ".#checks.$system.js-typecheck"
nix build ".#checks.$system.ccusage-tests" ".#checks.$system.node-tests" ".#checks.$system.performance-harness"
```

For an interactive shell with the pinned tools, run `nix develop` (or `direnv allow` with nix-direnv configured).

Neither entering the shell nor formatting or generating the schema installs JavaScript dependencies into your checkout. Hermetic builds use their own immutable inputs, not a local `node_modules`. Nix only includes Git-tracked files from a Git checkout: stage new source files with `git add` before building them (a commit is not required).

### Explicit checkout dependencies

For editor tooling or other local JS work, explicitly materialize the managed dependencies:

```sh
nix run .#js-install
```

This builds `packages.js-workspace` and copies **only** its managed `node_modules` directories to the current checkout, including the separate models.dev generator tool dependencies. The copy step is offline; it does not run a package-manager install or overwrite source files. Nix may first need to fetch the dependency closure as described above. Run it again after changing dependency locks. Existing managed `node_modules` directories are replaced; do not keep hand-edited dependencies there.

For docs, `nix run .#docs-dev` explicitly runs that materialization step before starting the pinned VitePress server. `nix build .#docs` builds the immutable site; `nix run .#docs-preview` serves that Nix output rather than local `.vitepress/dist` contents.

### Imperative Rust fast loops

For short edit/test cycles, run Cargo through the pinned shell from the repository root:

```sh
nix develop --command cargo build --locked --manifest-path rust/Cargo.toml --bin ccusage
nix develop --command cargo run --locked --manifest-path rust/Cargo.toml --bin ccusage -- codex daily --offline
nix develop --command cargo test --locked --manifest-path rust/Cargo.toml -p ccusage-core
nix develop --command cargo run --locked --manifest-path rust/Cargo.toml --bin ccusage -- statusline --offline < apps/ccusage/test/statusline-test.json
```

These run Cargo with `--locked` through the pinned development shell and reuse mutable local Cargo artifacts. They are not substitutes for sandboxed checks or release builds. Cargo may fetch locked crates into its local cache on the first run; the shell supplies the pinned pricing input without a build-time pricing download.

Use any of the `apps/ccusage/test/statusline-test-*.json` fixtures to preview other models. To analyze Rust visibility or generate a benchmark fixture with the pinned tools:

```sh
nix develop --command cargo hawk check --manifest-path rust/Cargo.toml
nix develop --command bun apps/ccusage/scripts/generate-large-fixture.ts --output-dir /tmp/ccusage-claude --codex-output-dir /tmp/ccusage-codex --size-mib 1024
```

### Updating dependencies and generated files

Dependency updates are deliberate networked maintenance operations, separate from `nix run .#js-install`:

1. Edit the workspace manifests or catalogs, then update only the pnpm lockfile with lifecycle scripts disabled:

   ```sh
   nix develop --command pnpm install --lockfile-only --ignore-scripts
   ```

2. Update the `pnpmDeps` hash in `nix/javascript.nix`. Temporarily use `lib.fakeHash`, build `nix build .#js-workspace`, then replace it with the hash Nix reports and rebuild. Do not commit the fake hash.
3. Run `nix run .#js-install`, then `nix flake check`. Commit the manifests, lockfile, and updated hash together.

The standalone tools under `nix/tools/` have separate Bun locks. To update one intentionally, for example:

```sh
(cd nix/tools/models-dev-gen && nix develop ../../.. --command bun install --lockfile-only --ignore-scripts)
nix run .#generate-bun-nix
nix run .#js-install
nix flake check
```

Commit the tool's manifest, `bun.lock`, and generated `bun.nix` together. `nix run .#generate-bun-nix` only regenerates Nix expressions from existing locks; it does not update or install dependencies.

Other generators also use pinned flake inputs:

```sh
nix run .#generate-schema             # Both config-schema.json copies
nix run .#generate-models-dev-pricing  # All three committed models.dev snapshots
```

To deliberately update the pricing inputs (requires network access), regenerate where needed and validate:

```sh
nix flake update litellm
nix flake check

nix flake update models-dev
nix run .#generate-models-dev-pricing
nix flake check
```

## Release boundary

Nix builds and validates the finished npm tarballs, with no networked install, lifecycle-hook build, or prepack fallback. The release workflow checks out the emitted release tag, gathers artifacts for the four supported targets, and publishes those exact tarballs without rebuilding them. Linux npm binaries are static; macOS binaries may link only system libraries. Cross-produced Darwin x64 artifacts receive architecture and linkage validation; they are not executed on the ARM build host.

Release orchestration is deliberately outside the build sandbox. GitHub-hosted runners, checkout/artifact services, GitHub API access, and publishing credentials are external infrastructure, not supplied or made offline by Nix. `nix run .#tagpr` requires GitHub permissions; `nix run .#npm-publish -- /path/to/finished-tarballs` requires registry access and publishes the complete five-tarball set (four native packages plus the launcher).

For GitHub OIDC publishing, configure an npm trusted publisher for **each** published package, matching this repository and `.github/workflows/release.yaml` (and an environment if configured). The publishing job needs `id-token: write`. Pinning npm does not configure registry trust or grant package ownership. Manual publishing instead requires appropriately scoped npm credentials; never commit them.

Use the canonical `ccusage` command with agent subcommands (for example, `ccusage codex daily`). Update user-facing documentation when behavior changes. Never include credentials or private agent logs in a contribution.
