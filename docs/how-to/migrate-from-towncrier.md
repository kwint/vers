# Migrate from towncrier

vers uses the same idea as towncrier: one fragment file per change, combined into
the changelog at release time. Most of the migration is renaming.

## 1. Generate a config

```sh
vers init --from-towncrier
```

This reads `[tool.towncrier]` from `pyproject.toml` (or `towncrier.toml`) and
writes a `vers.toml` with:

- the same fragment directory (towncrier's default if `directory` isn't set:
  `<package_dir>/<package>/newsfragments` when `package` is set, otherwise
  `newsfragments`)
- the same changelog file, if it's Markdown. towncrier's default is
  `NEWS.rst`; in that case vers uses `CHANGELOG.md` and warns
- a `[[version.files]]` entry for `pyproject.toml`, if the version is there
- custom `[[types]]`, but only if your towncrier types don't map onto the
  Keep a Changelog types (see the next step)

## 2. Rename existing fragments

```sh
vers migrate towncrier --dry-run   # shows the renames
vers migrate towncrier             # does them, using git mv
```

The default mapping is:

| towncrier type | vers type   |
|----------------|-------------|
| `feature`      | `added`     |
| `bugfix`       | `fixed`     |
| `removal`      | `removed`   |
| `deprecation`  | `deprecated`|
| `security`     | `security`  |
| `doc`          | `changed`   |
| `misc`         | dropped, because towncrier only lists their issue numbers. Keep them with `--map misc=changed`. |

Issue numbers in filenames stay the same, because `123.feature.md` already
matches vers's issue reference format. Orphan fragments (`+abc.feature.md`)
become slugs (`abc.added.md`).

## 3. Prepare the changelog

vers inserts new releases under a `## [Unreleased]` heading. If your changelog
doesn't have one, `vers init` adds it above the newest release.

vers recognises release headings in the Keep a Changelog form,
`## [1.2.0] - 2026-01-31`. If your older headings look different (for example
`project 1.2.0 (2026-01-31)`), you can:

- **leave them alone:** vers keeps unrecognised content byte-for-byte. But then
  it can't find your current version from the changelog, so make sure it's in a
  version file or a git tag. Or:
- **convert them:** `vers migrate towncrier --changelog` rewrites the headings
  it recognises and shows a diff first.

## 4. Update CI

Replace `towncrier check` with `vers check`. See
[Require fragments in CI](require-fragments-in-ci.md).

## Differences to be aware of

- vers renders Markdown only. Projects using reStructuredText changelogs need to
  convert to Markdown.
- vers also bumps your version files. Remove other version-bump steps, or let
  vers pick up the version they set (see
  [Manage version files](manage-version-files.md)).
- vers doesn't build from a template. Output is fixed Keep a Changelog Markdown.
