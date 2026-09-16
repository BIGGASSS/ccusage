# Shared by treefmt and the checkout-validating generate-schema app.
{
  writeShellApplication,
  coreutils,
  diffutils,
  oxfmt,
  generateConfigSchema,
}:
writeShellApplication {
  name = "ccusage-schema-gen";
  runtimeInputs = [
    coreutils
    diffutils
    oxfmt
    generateConfigSchema
  ];
  # Only overwrite files when content differs. Bumping an unchanged file's
  # mtime makes treefmt --fail-on-change report a spurious change.
  text = ''
    tmp="$(mktemp --suffix=.json)"
    trap 'rm -f "$tmp"' EXIT
    generate-config-schema "$tmp"
    oxfmt --config .oxfmtrc.json --write "$tmp"
    targets=(apps/ccusage/config-schema.json)
    if [[ -d docs/public ]]; then
      targets+=(docs/public/config-schema.json)
    fi
    for target in "''${targets[@]}"; do
      if ! cmp -s "$tmp" "$target"; then
        install -m 644 "$tmp" "$target"
      fi
    done
  '';
}
