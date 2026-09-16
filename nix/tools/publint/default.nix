# Nix-built `publint` CLI used by the package checks (`nix flake check`) and by
# the `build`/`prepack` scripts of every published package.
#
# The dependency set sits next to this file: `bun.lock` and `bun.nix` are real
# files that `bun install` and `bun2nix` regenerate. After updating the lock,
# run `nix run .#generate-bun-nix` instead of hand-editing the Nix expression.
{ bunCli, lib }:
bunCli {
  toolDir = ./.;
  pname = "publint";
  meta = {
    description = "Lint packaging errors";
    homepage = "https://publint.dev";
    license = lib.licenses.mit;
  };
}
