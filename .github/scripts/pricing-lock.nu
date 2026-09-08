# Shared helpers for the manual pricing lock update scripts.

# Hand the workflow both the verdict and the paths to commit, so the file list
# lives in one place instead of being repeated in the workflow.
export def report [result: record]: nothing -> nothing {
    [
        $"changed=($result.changed)"
        $"paths=($result.paths | str join ' ')"
    ]
    | to text
    | save --append $env.GITHUB_OUTPUT
}
