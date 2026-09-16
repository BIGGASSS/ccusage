# Shared helpers for the scripts that stage and validate the Rust binary
# shipped inside packages/ccusage-<platform>-<arch>.

# Only the Linux and macOS native packages are supported. Windows users can
# run the Linux package inside WSL; do not silently stage a Windows artifact.
export def binary-name [platform: string]: nothing -> string {
    match $platform {
        'linux' => 'ccusage'
        'darwin' => 'ccusage'
        _ => { error make {msg: $"Unsupported native platform: ($platform). Use Linux or macOS."} }
    }
}

# The dynamic libraries `otool -L` reports for a Mach-O binary.
#
# `ok` is false when otool itself failed, so a caller can choose between
# aborting with the captured stderr and treating the binary as unusable.
export def linked-dylibs [binary: path]: nothing -> record {
    let result = run-external otool '-L' $binary | complete
    {
        ok: ($result.exit_code == 0)
        stderr: $result.stderr
        dylibs: (match $result.exit_code {
            # Only the `<dylib> (compatibility version ...)` rows are tab
            # indented. Selecting on that indent rather than skipping a fixed
            # number of leading rows keeps the unindented `<binary>
            # (architecture <arch>):` header out of the result for fat binaries,
            # which repeat that header once per architecture.
            0 => (
                $result.stdout
                | lines
                | where {|line| $line | str starts-with (char tab) }
                | each {|line| $line | str trim | split row --regex '\s+' | first }
            )
            _ => []
        })
    }
}
