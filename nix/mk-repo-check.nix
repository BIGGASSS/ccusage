# Repository-wide checks share the same clean source and writable HOME.
{
  pkgs,
  nixFilter,
  root,
}:
let
  src = nixFilter {
    inherit root;
    exclude = map nixFilter.matchName [
      ".git"
      ".direnv"
      "node_modules"
      "target"
      "dist"
      "coverage"
    ];
  };
in
name: nativeBuildInputs: command:
pkgs.runCommand name
  {
    inherit src nativeBuildInputs;
  }
  ''
    cp -R "$src" source
    chmod -R u+w source
    cd source
    export HOME="$TMPDIR/home"
    mkdir -p "$HOME"
    ${command}
    touch "$out"
  ''
