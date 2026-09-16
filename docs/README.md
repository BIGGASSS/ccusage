# Documentation Site

This directory contains the VitePress documentation website for ccusage, hosted on Cloudflare at https://ccusage.com.

## Structure

- `guide/` - user guides and tutorials.
- `public/` - screenshots, static assets, and generated config schema.
- `.vitepress/` - VitePress configuration and theme customization.

The Nix docs build copies `apps/ccusage/config-schema.json` into the site's `public/` directory in its sandbox before running the pinned VitePress. The finished static site is `packages.docs`; local `.vitepress/dist` and checkout dependencies are not build inputs.

## Prerequisites

Install Nix with `nix-command` and `flakes` enabled. Supported development systems are Linux on x86-64 and ARM64, and macOS on Apple Silicon. Intel macOS npm releases are cross-built on Apple Silicon; this flake does not support native Intel Mac development. Native Windows is unsupported (use Linux in WSL). A cold run may fetch the locked tool and dependency closure; the site build itself runs offline in the Nix sandbox. See [CONTRIBUTING.md](../CONTRIBUTING.md) for setup, dependency updates, and the full network boundary.

## Commands

From the repository root:

```sh
nix build .#docs
nix run .#docs-dev
nix run .#docs-preview
nix build ".#checks.$(nix eval --impure --raw --expr builtins.currentSystem).js-typecheck"
nix fmt
```

`docs-dev` explicitly runs `js-install` to copy only Nix-managed `node_modules` into the checkout before launching VitePress. That includes the separate models.dev tool dependencies; it does not run a registry install or copy source files from the store. `docs-preview` serves the immutable Nix-built site, not local build output. Entering the shell, formatting, and generating schemas do not install checkout dependencies.

To change the schema, run `nix run .#generate-schema` from the repository root; do not edit the generated JSON manually. Dependency lockfile updates are a separate, explicit operation documented in [CONTRIBUTING.md](../CONTRIBUTING.md).

## Cloudflare deployment

`wrangler.jsonc` builds `..#docs` into the `result` symlink and deploys those assets. Run Wrangler from `docs/` in an environment with Nix available. A Node-only Cloudflare builder is not sufficient: provision Nix or upload the already-built static site from a Nix-capable runner. Cloudflare deployment tooling, credentials, and upload access remain an external networked boundary; they are not part of the hermetic site build. Do not add a pnpm install/build fallback.
