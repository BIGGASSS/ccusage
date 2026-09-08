# Contributing

This fork accepts bug reports, feature proposals, and pull requests directly. No contributor approval is required.

- For bugs, include the ccusage version, reproduction steps, and expected behavior. Remove private information from logs.
- Keep changes focused. Explain the problem, your approach, and how you tested it.
- Discuss substantial changes in an issue before investing in an implementation.
- Review your changes and add regression tests for behavior changes.

## Development

The CLI is Rust-first: agent adapters live in `rust/adapters/`, shared crates in `rust/crates/`, and the npm launcher and packaging in `apps/ccusage/`.

Use the pinned Nix development shell. With Nix and direnv installed:

```sh
direnv allow
direnv exec . just install
direnv exec . just fmt
direnv exec . just typecheck
direnv exec . just test
```

Run `just --list` inside the shell for additional commands. Install dependencies again after lockfile changes.

Use the canonical `ccusage` command with agent subcommands (for example, `ccusage codex daily`). Update user-facing documentation when behavior changes. Never include credentials or private agent logs in a contribution.
