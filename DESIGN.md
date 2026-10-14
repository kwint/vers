# vers — design

vers is a changelog and versioning tool built on the news-fragment idea from
[towncrier]: every change adds a small file to a `changes/` directory, and a
release combines those files into a changelog section. It differs from towncrier
in four ways:

1. **Keep a Changelog by default.** Output follows [Keep a Changelog 1.0.0] and
   versions follow [SemVer], with no configuration needed.
2. **Monorepos are first-class.** Config discovery is modelled on [ruff].
3. **Language-agnostic.** vers is a single static Rust binary. It does not
   prefer any ecosystem's manifest, and version strings in any file format can
   be updated.
4. **Open contribution policy.** AI-assisted contributions are welcome under the
   same review standards as any other contribution (see `CONTRIBUTING.md`).

[towncrier]: https://towncrier.readthedocs.io/
[Keep a Changelog 1.0.0]: https://keepachangelog.com/en/1.0.0/
[SemVer]: https://semver.org/
[ruff]: https://docs.astral.sh/ruff/configuration/

---

## 1. Fragments

### 1.1 Filename

```
changes/<name>.<type>[.<counter>].md
```

- `<type>` is a change type (§1.2).
- `<name>` is matched against the configured reference patterns (§2). If it
  matches, the fragment is linked to that ticket or merge request. Otherwise
  `<name>` is just a free-form slug.
- The optional `<counter>` lets one reference have several fragments of the same
  type (`123.fixed.md`, `123.fixed.2.md`), as in towncrier.

Examples:

```
changes/123.fixed.md                 # issue #123
changes/mr-45.added.md               # merge request !45
changes/PROJ-42.changed.md           # Jira ticket PROJ-42
changes/oauth-login.added.md         # no reference, just a slug
changes/123.fixed.2.md               # a second "fixed" entry for #123
```

### 1.2 Types

The defaults are exactly the Keep a Changelog categories, rendered in this order:

| Type         | Heading      | Aliases          |
|--------------|--------------|------------------|
| `added`      | Added        | `feat`, `feature`|
| `changed`    | Changed      | `change`         |
| `deprecated` | Deprecated   | `deprecation`    |
| `removed`    | Removed      | `removal`        |
| `fixed`      | Fixed        | `fix`, `bugfix`  |
| `security`   | Security     | `sec`            |

Aliases are accepted in filenames and by `vers new`. `vers new` always writes the
canonical name. You can add custom types or redefine the list in config, but the
defaults are what most projects should use.

### 1.3 Content

The content is Markdown, and each fragment becomes one bullet. If the content
spans several lines, the lines after the first are indented under the bullet.

Optional TOML frontmatter (`+++`) or YAML frontmatter (`---`) holds metadata:

```markdown
---
breaking: true
refs: ["mr-45", "PROJ-42"]
---
Config key `timeout` is now in seconds instead of milliseconds.
```

| Key        | Meaning                                                              |
|------------|----------------------------------------------------------------------|
| `breaking` | Forces a major bump (§4.1). The bullet gets a `**Breaking:**` prefix (configurable). |
| `refs`     | References in addition to any in the filename.                       |

### 1.4 Ordering within a category

Fragments are sorted by **when they were added**. Ties, and fragments with no
known time, are sorted alphabetically by filename. The time comes from:

1. **Git add-time:** the author time of the commit that first added the file. All
   fragments are resolved with **one** `git log --diff-filter=A --name-only
   --format=%at -- <changes dirs>` call, not one call per file.
2. **Filesystem creation time** (`Metadata::created()`, which is `statx` btime on
   Linux) for files git doesn't know about yet: untracked or staged but not
   committed. These are naturally the newest.
3. **Alphabetical** by filename if neither time is available (for example, the
   filesystem doesn't support btime).

Filesystem times are *not* used for committed files. After a fresh clone every
file has the checkout time, which would make the order meaningless.

If the repo is a shallow clone (common in CI), add-times can be wrong. vers warns
about this and suggests `fetch-depth: 0` or the equivalent.

### 1.5 Merging duplicates

As in towncrier, fragments of the same type with identical content are merged
into one bullet with all their references combined:
`- Fix crash on startup. ([#12](…), [#15](…))`.

---

## 2. Ticket and merge request references

References are configured as named **ref kinds**. Each kind has a pattern to
match, a display format and a URL template:

```toml
[refs.issue]
match   = '^(?<id>\d+)$'                        # bare number: 123.fixed.md
display = "#{id}"
url     = "{repo_url}/-/issues/{id}"

[refs.mr]
match   = '^mr-(?<id>\d+)$'                     # mr-45.added.md
display = "!{id}"
url     = "{repo_url}/-/merge_requests/{id}"

[refs.jira]
match   = '^(?<id>[A-Z][A-Z0-9]+-\d+)$'         # PROJ-42.changed.md
display = "{id}"
url     = "https://example.atlassian.net/browse/{id}"
```

- Kinds are tried in the order they're defined, and the first match wins.
- `{repo_url}` comes from `[changelog] repo_url`.
- The `repo = "github"` and `repo = "gitlab"` presets define `issue` and `pr`/`mr`
  kinds with the correct URL shapes (`/issues/N` and `/pull/N` on GitHub;
  `/-/issues/N` and `/-/merge_requests/N` on GitLab), so most projects only set
  `repo_url`.
- Filenames avoid `#` and `!` because those characters cause trouble in shells,
  which is why the MR prefix is `mr-`. The *display* still uses the platform's
  conventional sigil.
- References are rendered after the bullet text in configured order:
  `- Text. ([#123](…), [!45](…))`. This placement is configurable with
  `[changelog] ref_position = "end" | "start"`.
- A reference that matches no kind is a hard error in a filename. That's how you
  tell a slug like `oauth-login` apart from a typo in a ticket ID. In frontmatter
  `refs`, an unmatched reference is rendered as plain text with a warning.

`vers new` takes references directly:

```
vers new fixed 123                    # → changes/123.fixed.md
vers new added --ref mr-45 --ref PROJ-42 -m "OAuth login"
```

---

## 3. Changelog output

```markdown
# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [1.3.0] - 2026-10-14

### Added

- OAuth login support. ([!45](https://gitlab.example.com/org/repo/-/merge_requests/45))

### Fixed

- Crash when the config file is empty. ([#123](https://gitlab.example.com/org/repo/-/issues/123))

[Unreleased]: https://gitlab.example.com/org/repo/-/compare/v1.3.0...HEAD
[1.3.0]: https://gitlab.example.com/org/repo/-/compare/v1.2.0...v1.3.0
```

- The `## [Unreleased]` heading is always kept. New releases are inserted
  directly below it.
- The link references at the bottom are maintained automatically. Their URL shape
  comes from the `repo` preset or `[changelog] compare_url`, and the tag names
  from `tag_format` (§5).
- Dates use ISO 8601. Categories with no fragments are left out.
- The changelog is parsed only at the level of `## [x.y.z]` headings and the link
  reference block. Everything else, including hand edits to old releases, is kept
  byte-for-byte.
- **The changelog is the source of truth for the current version:** it's the
  newest `## [x.y.z]` heading. Git tags are only consulted if the changelog has
  no releases yet. vers keeps no separate state file.

---

## 4. Versioning

### 4.1 Choosing the release version

`vers release` picks the version to release in this order:

1. **CLI argument:** `--version X.Y.Z` or `--bump major|minor|patch`.
2. **Version files:** if the version in the version files (§4.2) is *newer* than
   the latest version in the changelog, that version is used. This covers
   projects where the version is bumped by hand, or by another tool, before the
   release. The version files are then already correct and aren't rewritten.
3. **Inference** from the pending fragments (below).

Whichever source is used, the result must be newer than the latest version in
the changelog, otherwise vers refuses to release.

If the version files give a smaller bump than the fragments imply (for example,
the files say `1.2.1` but a fragment is `breaking`), vers releases the version
from the files and prints a warning.

#### Inference

The bump level is the highest level implied by any pending fragment:

| Pending fragments                             | Bump  |
|-----------------------------------------------|-------|
| any `breaking: true`, or any `removed`        | major |
| any `added` or `deprecated`                   | minor |
| anything else                                 | patch |

- While the version is `0.x`, a major bump becomes minor and a minor bump becomes
  patch, following SemVer's "anything may change" rule for 0.x. This can be
  switched off with `[version] zero_major_shift = false`.
- The mapping from type to level is configurable per type.
- Pre-releases: `--pre rc` gives `1.3.0-rc.1`, then `rc.2`. A later release
  without `--pre` gives `1.3.0`.

### 4.2 Version files

```toml
[[version.files]]
path = "pyproject.toml"
key  = "project.version"                       # TOML / JSON / YAML key path

[[version.files]]
path = "package.json"
key  = "version"

[[version.files]]
path    = "src/version.h"
pattern = '#define VERSION "(?<version>[^"]+)"'  # regex fallback for anything else
```

- Edits to structured files keep the formatting (`toml_edit` for TOML; targeted
  span replacement for JSON and YAML).
- All version files must contain the same version. If they disagree, vers
  reports which files differ and stops.
- The version files may be at the changelog's latest version or newer (§4.1),
  but never older. An older version usually means a release was made without
  vers, so vers stops and reports it. `--force` skips both checks.

---

## 5. Monorepo support (modelled on ruff)

### 5.1 How ruff does it

1. **Closest config wins.** For each file, ruff uses the nearest `.ruff.toml`,
   `ruff.toml` or `pyproject.toml` with a `[tool.ruff]` section in an ancestor
   directory. Within one directory, they take precedence in that order.
2. **No implicit merging.** A nested config fully replaces its parent's.
   Inheritance only happens through `extend = "../ruff.toml"`.
3. **Paths in a config resolve relative to that config file.**
4. **Discovery walks the tree** and respects `.gitignore`. Running from the root
   covers everything, and running in a subdirectory covers only that subtree.
5. `--config` overrides discovery.

### 5.2 How vers applies it

```
repo/
  vers.toml                  # shared defaults; no [package] table → not releasable
  packages/
    api/
      vers.toml              # extend = "../../vers.toml"; [package] name = "api"
      CHANGELOG.md
      changes/123.fixed.md
    web/
      vers.toml
      CHANGELOG.md
      changes/
```

- vers reads only `.vers.toml` and `vers.toml`, with `.vers.toml` taking
  precedence. It deliberately doesn't read language manifests, so no ecosystem is
  the default one.
- **A package is a directory whose config has a `[package]` table.** A config
  without one only holds shared settings for other configs to `extend`. A
  single-package repo is just a root `vers.toml` with `[package]`, so the same
  model covers both cases.
- **A fragment belongs to the package with the nearest config** above it.
  Fragments under a config that isn't a package are an error.
- **`extend` is explicit and can be chained:** `extend = "../../vers.toml"`. The
  extending config's keys override the base config's. Tables are merged, and
  arrays are replaced rather than concatenated. Paths are resolved relative to
  the file they're written in.
- **Discovery** of packages below the current directory uses the `ignore` crate,
  the same one ruff uses, so `.gitignore` is respected.
- **Cross-package changes** get a fragment in each affected package. Each
  package's changelog then describes the change in terms that matter to *that*
  package's users.
- There is **one `CHANGELOG.md` per package**, with no combined root changelog.
- Tagging is off by default (`[version] tag = false`). When it's on,
  `tag_format = "{name}@v{version}"` is the sensible monorepo setting
  (`v{version}` for single packages). `tag_format` is also used for the compare
  links in the changelog, whether or not vers creates the tags itself.

---

## 6. CLI

| Command | Purpose |
|---|---|
| `vers init` | Create `vers.toml` and `changes/` (with a `.gitkeep`). If `CHANGELOG.md` exists, add `[Unreleased]` when it's missing; otherwise create a new changelog. |
| `vers new <type> [ref-or-slug] [--ref R]… [-m msg] [--breaking]` | Create a fragment in the package for the current directory. Opens `$EDITOR` if `-m` isn't given. `--package` overrides the package. |
| `vers status` | Pending fragments and the suggested next version for every package at or below the current directory. |
| `vers draft [pkg]` | Print the section that would be released, to stdout only. |
| `vers release [pkg…] [--all] [--bump L \| --version X] [--pre P] [--tag \| --no-tag] [--dry-run]` | Release (see below). `--tag`/`--no-tag` override the `tag` config for one run. |
| `vers check [--base REF]` | CI check (see below). |
| `vers version [pkg]` | Print the current version. |

All commands accept `--format json` so CI and other tools can read the output, and
`--config PATH` to override discovery.

### 6.1 `vers release` target selection

- **No arguments:** release the package that owns the current directory (nearest
  config).
- **`vers release api web`:** release the named packages.
- **`--all`:** release every package at or below the current directory.

A package is only released if it has pending fragments, so the version always
increases. A release never writes an empty section or bumps a version without
changes. With `--all`, packages without fragments are skipped and listed in the
summary. When a package is named explicitly or chosen from the current
directory, having no fragments is an error.

A release does the following for each package, in order:

1. Collect and validate fragments (types, references, frontmatter).
2. Read and check the version files (§4.2).
3. Choose the release version: CLI argument, then version files if newer, then
   inference (§4.1).
4. Render the section and insert it into `CHANGELOG.md`.
5. Update the version files, unless they already contain the release version.
6. Delete the fragments that were used. This is done with `git rm` if the files
   are tracked, so the deletions are staged along with the other changes.
7. If `[version] tag = true`, commit **only** the files vers changed in steps
   4–6, using `commit_message` (default `"Release {name} {version}"`), and tag
   that commit. With `--all`, all released packages go into one commit, which
   gets one tag per package. vers never pushes.

   A tag has to point at a commit that contains the release, which is why
   tagging also commits. With `tag = false` (the default), vers leaves all
   changes uncommitted so you can review them and commit yourself.

Steps 4–6 are prepared in memory for **all** selected packages before anything
is written. If any package fails validation, no files are changed.
`--dry-run` prints the diff.

### 6.2 `vers check`

Intended for CI. It runs `git diff --name-only <base>...HEAD`, maps each changed
file to its package using nearest-config lookup, and fails if a changed package
has no new fragment in the diff. It passes in any of these cases:

- `VERS_SKIP=1` is set (for example, from a `skip-changelog` label in CI).
- Every changed file in a package matches `[check] ignore = ["docs/**", "*.md"]`.
- The package config sets `[check] require_fragment = false`.

It also validates every fragment in the diff, failing on unknown types,
unmatched references in filenames and malformed frontmatter.

---

## 7. Full config reference (defaults shown)

```toml
# extend = "../vers.toml"

[package]                       # presence of this table makes the dir a package
name = "api"                    # default: directory name

[changelog]
path         = "CHANGELOG.md"
repo         = "github"         # "github" | "gitlab" | "none"
repo_url     = "https://github.com/org/repo"
compare_url  = "{repo_url}/compare/{from}...{to}"   # set by the repo preset
ref_position = "end"
breaking_prefix = "**Breaking:** "

[fragments]
directory = "changes"

[[types]]                       # only needed to customise; defaults per §1.2
name = "added"
heading = "Added"
aliases = ["feat", "feature"]
bump = "minor"

[refs.issue]                    # defaults come from the repo preset
match   = '^(?<id>\d+)$'
display = "#{id}"
url     = "{repo_url}/issues/{id}"

[version]
zero_major_shift = true
tag            = false          # commit the release and tag it (§6.1)
tag_format     = "v{version}"   # also used for compare links
commit_message = "Release {name} {version}"

[[version.files]]
path = "Cargo.toml"
key  = "package.version"

[check]
require_fragment = true
ignore = []
```

---

## 8. Implementation (Rust)

| Concern            | Crate(s)                                   |
|--------------------|--------------------------------------------|
| CLI                | `clap` (derive)                            |
| Config / TOML edit | `serde`, `toml`, `toml_edit`               |
| JSON / YAML        | `serde_json`, `serde_yaml`                 |
| Discovery          | `ignore`                                   |
| Git                | shell out to `git` (simpler and faster to build than `gix`; revisit later) |
| Versions           | `semver`                                   |
| Regex              | `regex`                                    |
| Templating         | none for v1; output is fixed Keep a Changelog Markdown |
| Errors / output    | `anyhow`/`thiserror`, `miette` for diagnostics |
| Tests              | `insta` snapshots, `assert_cmd`, `tempfile` |

Planned modules: `config` (discovery, extend, resolution), `fragment` (parsing,
refs, ordering), `changelog` (parse, render, insert), `version` (inference,
version files), `git`, `cli`.

## 9. Installation and distribution

Installing vers has to be easy for users of every ecosystem, without making any
ecosystem's runtime a requirement. The binary has no runtime dependencies, and
every channel delivers that same binary.

| Channel | How | Built with |
|---|---|---|
| Shell / PowerShell installer | `curl … \| sh`, `irm … \| iex` | [`dist`](https://opensource.axo.dev/cargo-dist/) (cargo-dist) |
| Self-update | `vers self update` (installer installs only) | `axoupdater` |
| PyPI | `uv tool install vers-changelog`, `pipx`, `uvx --from vers-changelog vers` | `maturin`, `bindings = "bin"` (as ruff and uv do) |
| npm | `npm i -D vers-changelog`, `npx vers-changelog` | `dist` npm installer, or per-platform optional dependencies |
| crates.io | `cargo install vers-changelog --locked`, `cargo binstall vers-changelog` | `cargo publish` + `dist` binstall metadata |

**Naming:** `vers` is taken on PyPI, npm and crates.io, so the package is
published as `vers-changelog` on all three. The binary and command are always
`vers`, the same way ripgrep's package installs `rg`. The GitHub repository
(`kwint/vers`) uses `vers`.

Not planned for now: a Homebrew tap, winget and a dedicated GitHub Action. Each
needs a separate repository or an external submission; the installer script
covers those cases.

Targets: Linux x86_64/aarch64 (glibc and musl), macOS x86_64/aarch64 and
Windows x86_64/aarch64.

Projects can pin a compatible version with top-level `required_version` in
`vers.toml` (as ruff does), so any install channel is safe to use.

## 10. Additions made while writing the docs

The user docs in `docs/` are the detailed specification of user-facing
behaviour. Writing them led to these additions:

- **`vers init`** detects common version files (`Cargo.toml`, `package.json`,
  `pyproject.toml`, `VERSION`, …) and the repository host. Inside a tree that
  already has a config, it adds `extend` automatically. `--shared` creates a
  config without `[package]`.
- **`repo = "auto"`** (the default) detects github.com or gitlab.com from the
  `origin` remote. `repo_url` defaults to the remote, with SSH URLs converted to
  HTTPS.
- **`vers new`** derives the slug from the first six words of the message when
  no name is given, adds a counter on collisions, and prompts for the type when
  it's left out in a terminal.
- **Version files** without `key` or `pattern` are plain files containing only
  the version (`VERSION`).
- **`[version] initial = "0.1.0"`** is used when there's no changelog release,
  tag or version file.
- **Pre-releases** (`--pre rc`) get their own sections. The final release folds
  them into one section (`fold_prereleases = true`) and doesn't need new
  fragments.
- **`hidden = true` types** are accepted by `check` and removed on release, but
  not rendered.
- **`vers check`** gets its base from `--base`, then `GITHUB_BASE_REF`, then
  `CI_MERGE_REQUEST_DIFF_BASE_SHA`, then `origin/HEAD`. Files under a config
  without `[package]` are ignored.
- **`vers migrate towncrier`** and **`vers init --from-towncrier`** cover
  migration from towncrier.
- **`vers version [--next]`** prints the current or next version, and
  **`vers completions <shell>`** prints shell completions.
- **Exit codes:** 0 success, 1 check or validation failure or nothing to
  release, 2 usage or config error.
- **The changelog's current version** is the highest SemVer heading, not simply
  the first heading.

## 11. Out of scope for v1

- Generating changelogs from commit messages (use git-cliff or release-please).
- Publishing packages to registries.
- Propagating version bumps between dependent packages (Changesets-style).
- Custom output templates.
