# Command line reference

```
vers [GLOBAL OPTIONS] <COMMAND> [ARGS]
```

## Global options

| Option | Description |
|---|---|
| `--config <PATH>` | Use this config file instead of discovering one. It's treated as the config for every path. |
| `--format <text\|json>` | Output format. Default `text`. `json` is meant for scripts and is stable within a major version. |
| `--color <auto\|always\|never>` | Coloured output. Default `auto`. `NO_COLOR` is respected. |
| `-q`, `--quiet` | Print errors only. |
| `-v`, `--verbose` | Print more detail. Repeat for debug output (`-vv`). |
| `-h`, `--help` | Print help. |
| `-V`, `--version` | Print vers's own version. |

## Package selection

Commands that act on packages pick them like this:

- **No package arguments:** the package that owns the current directory, which
  is the one with the nearest `vers.toml` that has a `[package]` table.
- **Package names:** the named packages, searched for at or below the current
  directory.
- **`--all`:** every package at or below the current directory.

`status` is the exception: with no arguments it lists every package below the
current directory.

---

## `vers init`

Create a `vers.toml` in the current directory.

```
vers init [--name <NAME>] [--shared] [--no-detect] [--from-towncrier]
```

| Option | Description |
|---|---|
| `--name <NAME>` | Package name. Default: the directory name. |
| `--shared` | Create a config without a `[package]` table, for other configs to `extend`. Creates neither `changes/` nor `CHANGELOG.md`. |
| `--no-detect` | Don't detect version files or the repository host. |
| `--from-towncrier` | Build the config from `[tool.towncrier]`. See [Migrate from towncrier](../how-to/migrate-from-towncrier.md). |

It also:

- adds `extend = "<path>"` if a `vers.toml` exists in a parent directory
- creates `changes/.gitkeep`
- creates `CHANGELOG.md`, or adds `## [Unreleased]` to an existing one
- detects version files: `Cargo.toml`, `package.json`, `pyproject.toml`,
  `setup.cfg`, `Chart.yaml`, `pubspec.yaml`, `mix.exs`, `*.gemspec`,
  `build.gradle(.kts)`, `pom.xml` and `VERSION`
- detects the repository host from the `origin` remote

Fails if the current directory already contains a `vers.toml` or `.vers.toml`.

---

## `vers new`

Create a fragment.

```
vers new [TYPE] [REF_OR_SLUG] [--ref <REF>]... [-m <MESSAGE>] [--breaking] [--package <NAME>]
```

| Argument / option | Description |
|---|---|
| `TYPE` | A change type or alias. If left out in an interactive terminal, vers asks for it. |
| `REF_OR_SLUG` | The fragment name. A matching reference (`123`, `mr-45`, `PROJ-42`) is linked. Anything else is used as a slug. Default: a slug made from the first six words of the message. |
| `--ref <REF>` | An extra reference, written to frontmatter. Can be repeated. |
| `-m`, `--message <MESSAGE>` | Fragment content. Without it, vers opens `$VISUAL` or `$EDITOR`. If neither is set and stdin isn't a terminal, it reads stdin. |
| `--breaking` | Set `breaking: true` in frontmatter. |
| `--package <NAME>` | Create the fragment in this package instead of the current one. |

If a file with the same name already exists, vers adds a counter
(`123.fixed.2.md`). Prints the path of the created file.

---

## `vers status`

Show pending fragments and the next version for each package.

```
vers status [PACKAGE]...
```

```console
$ vers status
api  1.4.0 → 1.5.0 (inferred: minor)        2 fragments: 1 added, 1 changed
web  0.3.1 → 0.4.0 (from package.json)      2 fragments: 2 fixed
cli  2.0.0                                  no fragments
```

---

## `vers draft`

Print the changelog section the next release would add, without changing
anything.

```
vers draft [PACKAGE] [--version <X.Y.Z> | --bump <LEVEL>] [--pre <ID>]
```

The options are the same as for `vers release`.

---

## `vers release`

Release packages.

```
vers release [PACKAGE]... [--all] [--version <X.Y.Z> | --bump <major|minor|patch>]
             [--pre <ID>] [--tag | --no-tag] [--dry-run] [--force]
```

| Option | Description |
|---|---|
| `--all` | Release every package at or below the current directory that has pending fragments. Others are skipped. |
| `--version <X.Y.Z>` | Release exactly this version. |
| `--bump <LEVEL>` | Bump by this level instead of inferring it. |
| `--pre <ID>` | Make a pre-release, such as `rc` → `1.3.0-rc.1`. |
| `--tag`, `--no-tag` | Override `[version] tag` for this run. |
| `--dry-run` | Print the changes as a diff without writing anything. |
| `--force` | Skip the version file checks (files disagree with each other, or are older than the changelog). |

`--version` and `--bump` can't be used together, and `--version` can't be used
with `--all` or with more than one package.

The version is chosen in this order: CLI option, then the version files if
they're newer than the changelog, then inference. See
[How vers chooses a version](../concepts/versioning.md).

A package with no pending fragments isn't released. This is an error unless
`--all` was used, or the current version is a pre-release being finalised.

All changes for all selected packages are computed and validated before any
file is written.

---

## `vers check`

Check a branch's changes for missing or invalid fragments. Intended for CI.

```
vers check [--base <REF>]
```

| Option | Description |
|---|---|
| `--base <REF>` | Compare against this ref. Default: `GITHUB_BASE_REF`, then `CI_MERGE_REQUEST_DIFF_BASE_SHA`, then `origin/HEAD`. |

The check fails if:

- a package has changed files (other than those matching `[check] ignore`) but
  no new fragment, and its `require_fragment` setting is on
- a new or changed fragment has an unknown type, a filename that looks like a
  reference but matches no reference kind, malformed frontmatter or an empty
  body

It's skipped entirely when `VERS_SKIP=1` is set.

---

## `vers version`

Print a package's current version.

```
vers version [PACKAGE] [--next]
```

| Option | Description |
|---|---|
| `--next` | Print the version `vers release` would release instead. |

Exits with an error if the version files disagree with each other or are older
than the changelog.

---

## `vers migrate towncrier`

Rename towncrier fragments to vers types.

```
vers migrate towncrier [--dry-run] [--map <FROM=TO>]... [--changelog]
```

| Option | Description |
|---|---|
| `--map <FROM=TO>` | Override the type mapping, for example `misc=changed`. |
| `--changelog` | Also rewrite towncrier-style release headings to Keep a Changelog headings. |

---

## `vers completions`

```
vers completions <bash|zsh|fish|powershell|elvish>
```

Prints a shell completion script to stdout.

## `vers self update`

Update vers to the latest release. Only available when vers was installed with
the standalone installer. Package-manager installs should be updated through the
package manager.

---

## Exit codes

| Code | Meaning |
|---|---|
| `0` | Success. |
| `1` | The command ran but failed: `vers check` found problems, validation failed, or there was nothing to release. |
| `2` | Usage or configuration error: bad arguments, invalid `vers.toml`, or a `required_version` mismatch. |

## Environment variables

| Variable | Effect |
|---|---|
| `VERS_SKIP` | When set to `1`, `vers check` passes without checking. |
| `VISUAL`, `EDITOR` | Editor used by `vers new` when `-m` isn't given. |
| `NO_COLOR` | Turns off coloured output. |
| `GITHUB_BASE_REF`, `CI_MERGE_REQUEST_DIFF_BASE_SHA` | Default base for `vers check`. |

## JSON output

With `--format json`, each command prints a single JSON document to stdout.
Example for `vers status`:

```json
{
  "packages": [
    {
      "name": "api",
      "path": "packages/api",
      "current_version": "1.4.0",
      "next_version": "1.5.0",
      "version_source": "inferred",
      "bump": "minor",
      "fragments": [
        { "path": "packages/api/changes/mr-12.added.md", "type": "added", "refs": ["mr-12"], "breaking": false }
      ]
    }
  ]
}
```
