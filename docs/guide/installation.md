# Installation

ccusage can be installed and used in several ways depending on your preferences and use case.

## Supported Platforms

The native CLI and npm packages support four platforms:

- Linux x64 (`x86_64-linux` in Nix)
- Linux ARM64 (`aarch64-linux`)
- macOS Intel (npm package cross-built on Apple Silicon)
- macOS Apple Silicon (`aarch64-darwin`)

Native Windows is unsupported. Windows users can run the Linux version inside WSL, with their package runner or Nix installed inside that Linux environment. npm installations use prebuilt native packages; they do not compile Rust during installation.

## Why Direct Execution Works Well

You do not need to install ccusage globally before trying it. Direct package runners work well for ad hoc usage:

- ✅ No global package to manage
- ✅ Easy access to the latest published version
- ✅ Cached package downloads after the first run

## Quick Start (Recommended)

The fastest way to use ccusage is to run it directly:

::: code-group

```bash [bunx (Recommended)]
bunx ccusage
```

```bash [pnpm]
pnpm dlx ccusage
```

```bash [npx]
npx ccusage@latest
```

```bash [Nix]
nix run github:ccusage/ccusage -- daily
```

:::

::: tip Speed Recommendation
We recommend [bunx](https://bun.com/docs/pm/bunx) for everyday use. It caches the downloaded package, so repeated runs are faster after the first launch.
:::

### Performance Comparison

Here's why runtime choice matters:

| Runtime  | First Run | Subsequent Runs | Notes                        |
| -------- | --------- | --------------- | ---------------------------- |
| bunx     | Fast      | **Instant**     | Recommended for everyday use |
| pnpm dlx | Fast      | Fast            | Good alternative             |
| npx      | Slow      | Moderate        | Widely available             |

## Global Installation (Optional)

You can install ccusage globally if you prefer a persistent command:

::: code-group

```bash [npm]
npm install -g ccusage
```

```bash [bun]
bun install -g ccusage
```

```bash [yarn]
yarn global add ccusage
```

```bash [pnpm]
pnpm add -g ccusage
```

:::

After global installation, run commands directly:

```bash
ccusage daily
ccusage monthly --breakdown
ccusage blocks --live
```

## Nix Installation

The flake supports Linux x64/ARM64 and macOS Apple Silicon build hosts. Intel Mac users should use the prebuilt npm package: the locked nixpkgs no longer supports native Intel macOS builds or development shells.

Install [Nix](https://nixos.org/download/) with the `nix-command` and `flakes` experimental features enabled. Nix runs the native CLI without Node.js or Bun:

```bash
nix run github:ccusage/ccusage -- daily --offline
nix profile add github:ccusage/ccusage
```

A cold Nix invocation needs network access for locked sources, dependencies, toolchains, or binary-cache artifacts. The actual builds run offline in the Nix sandbox; subsequent offline use requires those inputs to be cached. ccusage's `--offline` option controls runtime pricing access, not Nix or package-manager downloads.

## Development Installation

Use Nix with sandboxed builds; you do not need a host Node, pnpm, Bun, or Rust installation. Clone the repository:

```bash
git clone https://github.com/ccusage/ccusage.git
cd ccusage
nix develop # Optional pinned development shell
# Optional alternative with nix-direnv configured: direnv allow
```

Run the following commands from the repository root; entering the shell is optional:

```bash
nix fmt
nix build ".#checks.$(nix eval --impure --raw --expr builtins.currentSystem).js-typecheck"

# Hermetic Rust, Node, and performance-harness tests
system=$(nix eval --impure --raw --expr builtins.currentSystem)
nix build ".#checks.$system.ccusage-tests" \
  ".#checks.$system.node-tests" \
  ".#checks.$system.performance-harness"

nix flake check # Every flake check
nix build .#ccusage .#docs .#npm-tarballs # Native CLI, docs, and tested host npm tarballs
```

The shell, formatter, and schema generator do not install checkout dependencies. Builds and checks use an immutable JS workspace. For editor tooling, explicitly run `nix run .#js-install`: it copies only Nix-managed `node_modules`, including the separate models.dev tool dependencies, into your checkout. The copy is offline, but Nix may first need to fetch the locked closure. It does not run a registry install or overwrite source files.

`nix run .#docs-dev` explicitly performs that copy and runs pinned VitePress. `nix run .#docs-preview` serves the Nix-built site. For an imperative Rust edit/run loop using the pinned shell and Cargo `--locked`, run from the repository root:

```bash
nix develop --command cargo run --locked --manifest-path rust/Cargo.toml --bin ccusage -- daily --offline
nix develop --command cargo run --locked --manifest-path rust/Cargo.toml --bin ccusage -- monthly --json --offline
nix develop --command cargo test --locked --manifest-path rust/Cargo.toml -p ccusage-core
```

These local loops reuse mutable Cargo caches and are not substitutes for hermetic checks. Dependency updates are separate: run `nix develop --command pnpm install --lockfile-only --ignore-scripts`, update the `pnpmDeps` hash in `nix/javascript.nix`, then run `nix run .#js-install`. Standalone tool Bun locks are updated explicitly before running `nix run .#generate-bun-nix`. See [CONTRIBUTING.md](https://github.com/ccusage/ccusage/blob/main/CONTRIBUTING.md) for full procedures and the release network/credential boundary.

## Runtime Requirements

### Node.js

- Needed when using Node-based package runners or npm-style global installs
- Use Bun for direct execution when available

### Bun

- **Minimum**: Bun 1.3+
- **Recommended**: Latest stable release
- Recommended for `bunx ccusage` and for the fastest warm startup

## Verification

After installation, verify ccusage is working:

```bash
# Check version
ccusage --version

# Run help command
ccusage --help

# Test with daily report
ccusage daily
```

## Updating

### Direct Execution (npx/bunx)

Use an explicit `@latest` version when requesting the current release. Package runners may otherwise reuse cached versions.

### Nix

```bash
nix profile upgrade ccusage
```

### Global Installation

```bash
# Update with npm
npm update -g ccusage

# Update with bun
bun update -g ccusage
```

### Check Current Version

```bash
ccusage --version
```

## Uninstalling

### Global Installation

::: code-group

```bash [npm]
npm uninstall -g ccusage
```

```bash [bun]
bun remove -g ccusage
```

```bash [yarn]
yarn global remove ccusage
```

```bash [pnpm]
pnpm remove -g ccusage
```

:::

### Development Installation

```bash
# Remove cloned repository
rm -rf ccusage/
```

## Troubleshooting Installation

### Permission Errors

If you get permission errors during global installation:

::: code-group

```bash [npm]
# Use npx instead of global install
npx ccusage@latest

# Or configure npm to use a different directory
npm config set prefix ~/.npm-global
export PATH=~/.npm-global/bin:$PATH
```

```bash [Node Version Managers]
# Use nvm
nvm install 22
npm install -g ccusage

# Or use fnm
fnm install 22
npm install -g ccusage
```

:::

### Network Issues

If installation fails due to network issues:

```bash
# Try with different registry
npm install -g ccusage --registry https://registry.npmjs.org
```

Package runners still need network access for uncached packages. Likewise, Nix needs to fetch uncached inputs before its sandboxed build can run. The CLI's `--offline` flag does not bypass installation downloads.

### Version Conflicts

If you have multiple versions installed:

```bash
# Check which version is being used
which ccusage
ccusage --version

# Uninstall and reinstall
npm uninstall -g ccusage
npm install -g ccusage@latest
```

## Next Steps

After installation, check out:

- [Getting Started Guide](/guide/getting-started) - Your first usage report
- [Configuration](/guide/configuration) - Customize ccusage behavior
- [Daily Usage](/guide/daily-reports) - Understand daily usage patterns
