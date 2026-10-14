# Monorepos and config discovery

vers handles a single-package repository and a monorepo with the same rules. A
single-package repo is just a monorepo with one package. The rules are borrowed
from [ruff](https://docs.astral.sh/ruff/configuration/#config-file-discovery),
which has shown they work well in large repositories.

## The rules

### 1. The closest config wins

For any path (a fragment, a changed file, your current directory), vers walks up
the directory tree and uses the first `.vers.toml` or `vers.toml` it finds. If
both are in the same directory, `.vers.toml` is used.

```
repo/
  vers.toml                ← config for tools/ and everything else at the root
  packages/
    api/
      vers.toml            ← config for everything under packages/api/
      src/main.rs          → api
      changes/12.fixed.md  → api
    web/
      vers.toml            ← config for everything under packages/web/
  tools/release.sh         → root config
```

### 2. No implicit inheritance

A nested config doesn't inherit from configs above it. If
`packages/api/vers.toml` doesn't set `repo_url`, it doesn't get the root's
`repo_url` either, unless it uses `extend`:

```toml
extend = "../../vers.toml"
```

This means that to know a package's settings, you read its config and follow its
`extend` chain, and nothing else. Moving a config file never silently changes
what another config means.

When extending, the extending config's keys win. Tables are merged key by key,
and arrays are replaced as a whole.

### 3. Paths are relative to the file that contains them

`changes`, `CHANGELOG.md`, `../../Cargo.toml` and `ignore` globs are resolved
against the directory of the config that sets them, not the directory you run
vers from. An inherited path is resolved relative to the file it was written in.

### 4. A package is a config with `[package]`

A config without a `[package]` table only holds settings for other configs to
`extend`. A root config like that is the usual way to share types, reference
kinds and repository settings. Fragments that fall under it are an error,
because there's no package to release them with. Other files under it, such as
`tools/release.sh` above, don't belong to any package, so `vers check` ignores
them.

### 5. Discovery walks down from the current directory

Commands that act on several packages (`status`, `release --all`, `check`) find
packages by searching *down* from the current directory. They respect
`.gitignore`, so `node_modules/` and `target/` are skipped. Running from the
root covers everything, and running from `packages/` covers only what's below
it.

## Why one changelog per package?

Users of `api` care about changes to `api`. A combined changelog makes every
reader filter out other packages' entries, and it can't map cleanly onto
per-package versions.

When a change touches several packages, add a fragment to each one. That's a
little more typing, but each fragment can describe the change from that
package's point of view, which is usually what the reader needs anyway.

## Why not read `package.json` or `pyproject.toml` for config?

vers is meant to be language-agnostic. Reading config from one ecosystem's
manifest would make that ecosystem the default and the others second-class. The
same `vers.toml` works the same way in any project.
