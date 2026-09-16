# One package set for builds, checks, formatters and the development shell.
{ inputs, ... }:
{
  perSystem =
    { system, ... }:
    {
      _module.args.pkgs =
        let
          base = import inputs.nixpkgs {
            inherit system;
            overlays = [ inputs.rust-overlay.overlays.default ];
          };
        in
        base
        // {
          # VitePress requires newer Node than the locked default LTS. Select
          # it for our consumers, without rebuilding unrelated nixpkgs tools
          # (oxlint, pnpm, etc.) against a different build-time Node dependency.
          nodejs = base.nodejs_26;
        };
    };
}
