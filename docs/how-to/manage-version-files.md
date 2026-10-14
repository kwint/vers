# Manage version files

*Version files* are the files where your project stores its version, such as
`Cargo.toml`, `package.json` or a `VERSION` file. vers reads the current version
from them and writes the new version on release.

`vers init` detects common version files for you. This guide covers adding,
changing and checking them by hand.

## Structured files: TOML, JSON, YAML

Point vers at the file and the key that holds the version, written as a
dotted path:

```toml
[[version.files]]
path = "Cargo.toml"
key  = "package.version"

[[version.files]]
path = "package.json"
key  = "version"

[[version.files]]
path = "pyproject.toml"
key  = "project.version"

[[version.files]]
path = "chart/Chart.yaml"
key  = "version"
```

The format is detected from the file extension. vers edits only the version
value and keeps comments, formatting and key order intact.

## A file that contains only the version

Leave out `key` and `pattern`:

```toml
[[version.files]]
path = "VERSION"
```

## Any other file: use a regex

Use `pattern` with a named group called `version`:

```toml
[[version.files]]
path    = "src/version.h"
pattern = '#define APP_VERSION "(?<version>[^"]+)"'

[[version.files]]
path    = "docs/conf.py"
pattern = '^release = "(?<version>[^"]+)"'
```

The pattern must match exactly once in the file. If it matches zero times or
more than once, vers stops with an error.

## Workspace versions (Cargo, npm workspaces)

If several packages share a version through a workspace file, point every
package at that file:

```toml
# crates/core/vers.toml
[[version.files]]
path = "../../Cargo.toml"
key  = "workspace.package.version"
```

Paths are resolved relative to the `vers.toml` they're written in.

## Bump the version yourself

You can bump the version in your files before releasing, by hand or with another
tool such as `npm version` or `cargo set-version`. If the version in your files
is newer than the latest release in the changelog, vers releases that version
and doesn't touch the files. See
[How vers chooses a version](../concepts/versioning.md).

## Check that the version files agree

```sh
vers version
```

This prints the current version. If the version files disagree with each other,
or are older than the changelog, it prints an error naming each file and the
version in it.
