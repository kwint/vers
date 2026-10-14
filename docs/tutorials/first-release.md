# Your first release

In this tutorial you'll set up vers in a tiny project, record a few changes and
make two releases. It takes about 10 minutes.

You need:

- vers installed (see [Install vers](../how-to/install.md))
- git

The project has no programming language at all. vers doesn't care what your
project is written in.

## 1. Create a project

```console
$ mkdir greeter && cd greeter
$ git init -q
$ git remote add origin https://github.com/example/greeter.git
$ echo "0.1.0" > VERSION
```

The remote only tells vers where your issues and merge requests live, so it can
link to them. Nothing is pushed in this tutorial.

## 2. Initialise vers

```console
$ vers init
Created vers.toml
Created changes/.gitkeep
Created CHANGELOG.md
Detected repository: github (https://github.com/example/greeter)
Detected version file: VERSION (0.1.0)
```

Look at what vers created:

```console
$ cat vers.toml
[package]
name = "greeter"

[[version.files]]
path = "VERSION"
```

That's the whole config. Everything else uses the defaults. `CHANGELOG.md` has
the standard Keep a Changelog header and an empty `## [Unreleased]` section.

Commit the setup:

```console
$ git add . && git commit -qm "Set up vers"
```

## 3. Record your first changes

Imagine you've just written a feature. Record it:

```console
$ vers new added -m "Greet people by name."
Created changes/greet-people-by-name.added.md
```

`added` is the *type* of change. vers uses the six Keep a Changelog types:
`added`, `changed`, `deprecated`, `removed`, `fixed` and `security`.
If you run `vers new` with no arguments in a terminal, it asks for everything
it needs.

Now record a bug fix that closes issue 7:

```console
$ vers new fixed 7 -m "Don't crash on an empty name."
Created changes/7.fixed.md
```

Because the name is a number, vers treats it as a reference to issue #7.

Each fragment is a plain Markdown file. Open one:

```console
$ cat changes/7.fixed.md
Don't crash on an empty name.
```

## 4. Preview the release

```console
$ vers status
greeter  (unreleased) → 0.1.0 (from VERSION)  2 fragments: 1 added, 1 fixed
```

The changelog has no releases yet and `VERSION` says `0.1.0`, so vers will
release `0.1.0`.

See exactly what will be written:

```console
$ vers draft
## [0.1.0] - 2026-10-14

### Added

- Greet people by name.

### Fixed

- Don't crash on an empty name. ([#7](https://github.com/example/greeter/issues/7))
```

## 5. Release

```console
$ vers release
Releasing greeter 0.1.0 (from VERSION)
  Updated CHANGELOG.md
  Removed 2 fragments
```

Open `CHANGELOG.md`. The new section sits under `## [Unreleased]`, and vers
added link references at the bottom of the file. The `changes/` directory is
empty again.

vers doesn't commit anything unless you tell it to, so review the changes and
commit them yourself:

```console
$ git add -A && git commit -qm "Release 0.1.0"
```

## 6. Make a second release

Fix another bug:

```console
$ vers new fixed 9 -m "Trim whitespace around names."
Created changes/9.fixed.md
$ vers status
greeter  0.1.0 → 0.1.1 (inferred: patch)  1 fragment: 1 fixed
```

This time `VERSION` isn't newer than the changelog, so vers works out the
version from the fragments. A fix only needs a patch bump.

```console
$ vers release
Releasing greeter 0.1.1 (inferred: patch)
  Updated CHANGELOG.md
  Updated VERSION (0.1.0 → 0.1.1)
  Removed 1 fragment

$ cat VERSION
0.1.1
```

## What you've learned

- `vers init` sets a project up and detects its version files.
- `vers new <type> [ref] -m "..."` records a change as a fragment.
- `vers status` and `vers draft` show what the next release will contain.
- `vers release` writes the changelog, bumps the version and removes the
  fragments.

## Next steps

- Working in a repo with several packages? Continue with
  [Set up a monorepo](monorepo.md).
- Make every merge request include a fragment:
  [Require fragments in CI](../how-to/require-fragments-in-ci.md).
- Learn how vers picks a version: [How vers chooses a version](../concepts/versioning.md).
