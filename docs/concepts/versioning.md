# How vers chooses a version

vers deals with two versions: the **current** version and the **release**
version.

## The current version

The current version is the latest version that was released. vers looks for it
in this order:

1. **The changelog:** the highest version among its `## [x.y.z]` headings.
2. **Git tags** matching `tag_format`, if the changelog has no releases yet.
3. **None**, for a project that has never been released.

The changelog comes first because it's the one record that vers itself
maintains, and it's correct by construction. Tags may never have been pushed or
may follow a different scheme, and version files can be edited by anyone.

## The release version

`vers release` uses the first of these that applies:

1. **The command line:** `--version X.Y.Z` or `--bump <level>`.
2. **The version files, if they're newer than the current version.** This
   respects a bump you or another tool already made, such as `npm version` or
   `cargo set-version`. vers releases that version and leaves the files alone.
3. **Inference from the fragments,** applied to the current version, or
   `[version] initial` (`0.1.0`) if there is no current version.

The release version must always be newer than the current version. vers never
releases the same version twice and never goes backwards.

### Inference

Each type has a bump level: `removed` is major, `added` and `deprecated` are
minor, and the rest are patch. A fragment with `breaking: true` is always
major. The highest level among the pending fragments wins.

| Pending fragments | 1.4.2 → | 0.4.2 → |
|---|---|---|
| fixed | 1.4.3 | 0.4.3 |
| fixed, added | 1.5.0 | 0.4.3 |
| added, removed | 2.0.0 | 0.5.0 |
| changed (breaking) | 2.0.0 | 0.5.0 |

The `0.x` column shows `zero_major_shift`. SemVer says anything may change
before 1.0, and most `0.x` projects use the minor number for breaking changes.
So on `0.x` each bump moves down one level. Leaving `0.x` is always a deliberate
step: `vers release --version 1.0.0`.

### When the version files and fragments disagree

If your files say `1.4.3` but a pending fragment is `added`, inference would
give `1.5.0`. vers releases `1.4.3`, because that's what you asked for by
editing the file, and prints a warning that the fragments suggest a minor bump.

### Consistency checks

Before releasing, vers checks that:

- **all version files agree with each other.** If they don't, some tool bumped
  only part of the project.
- **no version file is older than the current version.** If one is, a release
  was probably made without vers, or a merge went wrong.

Either problem stops the release. `--force` skips these checks.

## Releases require changes

A package is released only when it has pending fragments. That way:

- every version in the changelog has content
- `vers release --all` in a monorepo releases only the packages that changed

The one exception is finalising a pre-release (`1.3.0-rc.2` → `1.3.0`), where
the content comes from the pre-release sections.
