# vers

**Fresh changelogs for every release.**

vers turns small change notes in your repository into a
[Keep a Changelog](https://keepachangelog.com/en/1.0.0/) section and the next
[SemVer](https://semver.org/) version. It works with any language, in a
single-package repo or a monorepo.

> *vers* is short for **vers**ion, and in Dutch it means **fresh**.

> [!NOTE]
> vers is in early development. This README and the [docs](docs/index.md)
> describe the planned 1.0 behaviour.

## What it does

Each change comes with a short note, called a *fragment*, in a `changes/`
directory:

```console
$ vers new added mr-45 -m "Log in with OAuth."
Created changes/mr-45.added.md

$ vers new fixed 123 -m "Don't crash when the config file is empty."
Created changes/123.fixed.md
```

At release time, vers works out the version, writes the changelog, updates your
version files and deletes the fragments it used:

```console
$ vers release
Releasing app 1.3.0 (inferred: minor)
  Updated CHANGELOG.md
  Updated Cargo.toml (1.2.0 → 1.3.0)
  Removed 2 fragments
```

```diff
 ## [Unreleased]

+## [1.3.0] - 2026-10-14
+
+### Added
+
+- Log in with OAuth. ([!45](https://gitlab.com/acme/app/-/merge_requests/45))
+
+### Fixed
+
+- Don't crash when the config file is empty. ([#123](https://gitlab.com/acme/app/-/issues/123))
+
 ## [1.2.0] - 2026-09-30
```

Because every change is its own file, branches never conflict on
`CHANGELOG.md`, and reviewers see the changelog entry in the same merge request
as the code.

## Install

Pick whichever fits your setup. Each one gives you the same single binary.

```sh
# macOS / Linux
curl -LsSf https://github.com/kwint/vers/releases/latest/download/vers-installer.sh | sh

# Windows
powershell -ExecutionPolicy ByPass -c "irm https://github.com/kwint/vers/releases/latest/download/vers-installer.ps1 | iex"

# Homebrew
brew install kwint/tap/vers

# Python tooling (no Python needed at runtime)
uv tool install vers-changelog        # or: pipx install vers-changelog

# Node tooling
npm install --save-dev vers-changelog # or run once: npx vers-changelog

# Rust tooling
cargo binstall vers-changelog         # or: cargo install vers-changelog --locked
```

The package is called `vers-changelog` everywhere, and the command it installs
is `vers`. To try it without installing anything, run
`uvx --from vers-changelog vers` or `npx vers-changelog`. See
[Install vers](docs/how-to/install.md) for CI and for pinning a version per
project.

## Quick start

```sh
vers init                                  # creates vers.toml, changes/ and CHANGELOG.md
vers new fixed 123 -m "Fix the thing."     # add a fragment
vers draft                                 # preview the next changelog section
vers release                               # write it
```

New to vers? Follow the tutorial [Your first release](docs/tutorials/first-release.md),
which takes about 10 minutes.

## Features

- **Keep a Changelog by default.** Categories are Added, Changed, Deprecated,
  Removed, Fixed and Security, rendered in the standard format with compare
  links.
- **Semantic versioning built in.** The next version comes from your fragments,
  from a version you already bumped in your files, or from the command line.
- **Language-agnostic.** vers updates versions in TOML, JSON, YAML, plain text
  or any file you can describe with a regex.
- **Monorepos are first-class.** Each package has its own `vers.toml` and
  changelog, and config is discovered the way [ruff](https://docs.astral.sh/ruff/)
  does it: the closest config wins, and inheritance is explicit with `extend`.
- **Issue and merge request links.** Name a fragment `123.fixed.md` or
  `mr-45.added.md` and vers links it, with presets for GitHub and GitLab and
  support for Jira-style keys.
- **CI check.** `vers check` fails a merge request that changes a package
  without adding a fragment for it.
- **Safe releases.** Changes are validated for all packages before any file is
  written. Nothing is committed or tagged unless you turn that on.

## Documentation

The docs follow the [Diátaxis](https://diataxis.fr/) structure:

| | |
|---|---|
| **[Tutorials](docs/index.md#tutorials)** | Learn vers step by step. |
| **[How-to guides](docs/index.md#how-to-guides)** | Recipes for specific tasks: CI, monorepos, migrating from towncrier, and more. |
| **[Reference](docs/index.md#reference)** | Every command, config key and file format. |
| **[Concepts](docs/index.md#concepts)** | Why vers works the way it does. |

## Coming from towncrier?

vers uses the same fragment idea, with Keep a Changelog defaults, monorepo
support and no language lock-in. See
[Migrate from towncrier](docs/how-to/migrate-from-towncrier.md).

## Contributing

Contributions are welcome, including AI-assisted ones. See
[CONTRIBUTING.md](CONTRIBUTING.md).

## License

[MIT](LICENSE)
