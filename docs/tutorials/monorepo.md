# Set up a monorepo

In this tutorial you'll set up vers in a repository with two packages: a Rust
API and a JavaScript web app. You'll share config between them and release them
independently. It takes about 15 minutes.

It assumes you've done [Your first release](first-release.md), or are familiar
with fragments and `vers release`.

## 1. Create the repository

```console
$ mkdir shop && cd shop
$ git init -q -b main
$ git remote add origin https://gitlab.com/example/shop.git
$ mkdir -p packages/api packages/web
$ printf '[package]\nname = "api"\nversion = "1.4.0"\n' > packages/api/Cargo.toml
$ echo '{ "name": "web", "version": "0.3.0" }' > packages/web/package.json
$ git add -A && git commit -qm "Existing code"
$ git tag api@v1.4.0 && git tag web@v0.3.0
```

Both packages were released before you started using vers, and those releases
are tagged. vers uses the tags as the current version until the changelogs
have releases of their own.

## 2. Add a shared root config

Settings that are the same for every package go in a config at the root:

```console
$ vers init --shared
Created vers.toml (shared config, not a package)
Detected repository: gitlab (https://gitlab.com/example/shop)
```

```toml
# vers.toml
[changelog]
repo = "gitlab"
repo_url = "https://gitlab.com/example/shop"

[version]
tag_format = "{name}@v{version}"
```

This config has no `[package]` table, so it isn't a package itself. It only
holds defaults for the packages to inherit.

## 3. Initialise each package

```console
$ cd packages/api && vers init && cd ../..
Created vers.toml (extends ../../vers.toml)
Created changes/.gitkeep
Created CHANGELOG.md
Detected version file: Cargo.toml (package.version = 1.4.0)

$ cd packages/web && vers init && cd ../..
Created vers.toml (extends ../../vers.toml)
Created changes/.gitkeep
Created CHANGELOG.md
Detected version file: package.json (version = 0.3.0)
```

Because there's already a config above them, vers made each package config
`extend` the root config:

```toml
# packages/api/vers.toml
extend = "../../vers.toml"

[package]
name = "api"

[[version.files]]
path = "Cargo.toml"
key = "package.version"
```

Nested configs never inherit anything implicitly. `extend` is the only way a
package picks up settings from another config, so you can always read a
package's settings by following its `extend` chain.

## 4. Add fragments to each package

vers works out the package from your current directory:

```console
$ cd packages/api
$ vers new added mr-12 -m "Add the /orders endpoint."
Created changes/mr-12.added.md
$ cd ../web
$ vers new fixed 30 -m "Fix the cart total rounding."
Created changes/30.fixed.md
$ cd ../..
```

From the root, `--package` does the same without changing directory:

```console
$ vers new changed --package api -m "Return 404 instead of 400 for unknown orders."
Created packages/api/changes/return-404-instead-of-400-for.changed.md
```

## 5. Check the status of every package

Run from the root, `vers status` shows every package below the current
directory:

```console
$ vers status
api  1.4.0 → 1.5.0 (inferred: minor)  2 fragments: 1 added, 1 changed
web  0.3.0 → 0.3.1 (inferred: patch)  1 fragment: 1 fixed
```

## 6. Release one package

`vers release` with no arguments releases the package you're standing in:

```console
$ cd packages/web
$ vers release
Releasing web 0.3.1 (inferred: patch)
  Updated CHANGELOG.md
  Updated package.json (0.3.0 → 0.3.1)
  Removed 1 fragment
$ cd ../..
```

From the root, you can also name the package: `vers release web`.

## 7. Release everything that's pending

```console
$ vers release --all
Releasing api 1.5.0 (inferred: minor)
  Updated packages/api/CHANGELOG.md
  Updated packages/api/Cargo.toml (1.4.0 → 1.5.0)
  Removed 2 fragments
Skipped web (no fragments)
```

`--all` only releases packages with pending fragments. A version is never bumped
without changes to describe.

## 8. Check that a branch has fragments

Make a change in `web` without a fragment:

```console
$ git add -A && git commit -qm "Release api 1.5.0"
$ git switch -q -c tweak-styles
$ echo "/* tweak */" > packages/web/style.css
$ git add -A && git commit -qm "Tweak styles"
$ vers check --base main
error: package `web` has changes but no new fragment
  changed: packages/web/style.css
  hint: add one with `vers new <type> --package web`, or set VERS_SKIP=1 to skip this check
```

vers mapped `style.css` to the `web` package because `packages/web/vers.toml` is
the closest config above it. This is what makes `vers check` precise in a
monorepo. To run it on every merge request, see
[Require fragments in CI](../how-to/require-fragments-in-ci.md).

## What you've learned

- A config with a `[package]` table is a package. A config without one only
  holds shared settings.
- Packages inherit settings only through `extend`.
- vers picks the package from your current directory, or from `--package`.
- `vers release` releases the current package, `vers release <name>` releases
  named packages, and `vers release --all` releases every package with pending
  fragments.
- `vers check` maps changed files to packages and requires a fragment for each
  one.

## Next steps

- [Monorepos and config discovery](../concepts/monorepos.md) explains the rules
  in detail.
- [Commit and tag releases](../how-to/commit-and-tag-releases.md) covers
  per-package tags such as `api@v1.5.0`.
