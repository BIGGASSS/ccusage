{
  lib,
  ...
}:
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
    {
      pre-commit = {
        check.enable = false;
        inherit pkgs;
        settings = {
          src = root;
          package = pkgs.prek;
          hooks = {
            ccusage-treefmt = {
              enable = true;
              name = "treefmt";
              # `--no-cache` keeps the hook safe under prek's parallel,
              # file-batched invocations. Without it each concurrent treefmt
              # process races for the shared eval-cache SQLite db and the loser
              # dies with "failed to open cache: ... timeout" (notably when
              # staging every package.json at once). prek already selects the
              # changed files, so treefmt's own cache is redundant here anyway.
              entry = "${lib.getExe config.treefmt.build.wrapper} --no-cache";
              files = ".*";
              pass_filenames = true;
              stages = [ "pre-commit" ];
              priority = 10;
            };
            gitleaks-protect = {
              enable = true;
              name = "gitleaks";
              entry = "${lib.getExe pkgs.gitleaks} protect --staged --config .gitleaks.toml";
              pass_filenames = false;
              always_run = true;
              stages = [ "pre-commit" ];
              priority = 20;
            };
            ccusage-flake-check = {
              enable = true;
              name = "flake checks";
              # Use the same derivations as CI, without depending on checkout
              # node_modules or a populated Cargo cache. Nix is the bootstrap
              # prerequisite; all tools used by the checks come from the flake.
              entry = "nix flake check --print-build-logs";
              pass_filenames = false;
              always_run = true;
              stages = [ "pre-push" ];
              priority = 0;
            };
            gitleaks-detect = {
              enable = true;
              name = "gitleaks";
              entry = "${lib.getExe pkgs.gitleaks} detect --config .gitleaks.toml";
              files = ".*";
              pass_filenames = false;
              always_run = true;
              stages = [ "pre-push" ];
              priority = 0;
            };
          };
        };
      };
    };
}
