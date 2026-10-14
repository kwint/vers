# Require fragments in CI

`vers check` fails when a branch changes a package without adding a fragment for
it. It also validates every fragment the branch adds. This guide shows how to
run it on every merge request.

## GitHub Actions

```yaml
# .github/workflows/changelog.yml
name: changelog
on: pull_request

jobs:
  vers-check:
    runs-on: ubuntu-latest
    if: ${{ !contains(github.event.pull_request.labels.*.name, 'skip-changelog') }}
    steps:
      - uses: actions/checkout@v4
        with:
          fetch-depth: 0
      - name: Install vers
        run: |
          curl -LsSf https://github.com/kwint/vers/releases/latest/download/vers-changelog-installer.sh | sh
          echo "$HOME/.local/bin" >> "$GITHUB_PATH"
      - run: vers check
```

## GitLab CI

```yaml
vers-check:
  image: alpine:3
  rules:
    - if: $CI_PIPELINE_SOURCE == "merge_request_event" && $CI_MERGE_REQUEST_LABELS !~ /skip-changelog/
  variables:
    GIT_DEPTH: 0
  before_script:
    - apk add --no-cache curl git
    - curl -LsSf https://github.com/kwint/vers/releases/latest/download/vers-changelog-installer.sh | sh
    - export PATH="$HOME/.local/bin:$PATH"
  script:
    - vers check
```

## Why a full clone

vers needs the history back to the point where the branch split from the target
branch, both to compute the diff and to sort fragments by when they were added.
In a shallow clone it prints a warning. `fetch-depth: 0` or `GIT_DEPTH: 0`
avoids this.

## The base branch

vers picks the base to compare against automatically:

1. `--base <ref>`, if given
2. `GITHUB_BASE_REF` (GitHub Actions pull requests)
3. `CI_MERGE_REQUEST_DIFF_BASE_SHA` (GitLab merge request pipelines)
4. `origin/HEAD`

## Skip the check for one change

- **Label:** use the `if:` or `rules:` conditions shown above.
- **Environment variable:** set `VERS_SKIP=1`.

## Skip the check for some files

Changes to docs or tests often don't need a changelog entry:

```toml
[check]
ignore = ["docs/**", "tests/**", "*.md"]
```

Patterns are relative to the package's `vers.toml`. If every changed file in a
package matches, that package doesn't need a fragment.

## Turn the check off for one package

```toml
# tools/scripts/vers.toml
[check]
require_fragment = false
```

## Run it locally

```sh
vers check --base main
```
