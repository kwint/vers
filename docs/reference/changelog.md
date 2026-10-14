# Changelog format reference

vers writes changelogs in the [Keep a Changelog 1.0.0](https://keepachangelog.com/en/1.0.0/)
format. This page describes exactly which parts of the file vers reads and
writes.

## Structure

```markdown
# Changelog                                    ← preamble: never touched

All notable changes to this project ...

## [Unreleased]                                ← insertion point

## [1.3.0] - 2026-10-14                        ← release heading

### Added                                      ← type heading

- Log in with OAuth. ([!45](…))                ← entry

## [1.2.0] - 2026-09-30
...

[Unreleased]: https://…/compare/v1.3.0...HEAD  ← link references
[1.3.0]: https://…/compare/v1.2.0...v1.3.0
[1.2.0]: https://…/releases/tag/v1.2.0
```

## What vers reads

| Element | Pattern |
|---|---|
| Unreleased heading | `## [Unreleased]` (case-insensitive) |
| Release heading | `## [<semver>] - <YYYY-MM-DD>`, optionally followed by ` [YANKED]` |
| Link reference | `[<label>]: <url>` lines at the end of the file |

The current version is the highest SemVer version among the release headings.
It's not simply the first heading, because a changelog may list releases from
several branches out of order.

Anything else, including old releases that don't match the pattern, is kept
byte-for-byte.

## What vers writes on release

1. **New section** directly below `## [Unreleased]`. If there's no Unreleased
   heading, vers inserts one above the first release heading (or at the end of
   the preamble) and says so.
2. **Section contents:**
   - one `### <Heading>` per type that has entries, in configured type order
   - one entry per fragment, as described in the
     [fragment format](fragments.md#rendering)
   - a blank line between all blocks
3. **Link references** (unless `repo = "none"`):
   - `[Unreleased]` is updated to compare the new tag with `HEAD`
   - a link for the new version is added: a compare link from the previous tag,
     or `release_url` for the first release
   - existing link references are kept, and new ones go at the top of the block

Tag names in links come from `tag_format`.

## Pre-releases

Pre-release sections look like any other release (`## [1.3.0-rc.1] - …`). When
`fold_prereleases` is on, the final release:

1. collects the entries from every `1.3.0-*` section
2. adds the entries from any new fragments
3. writes them as one `## [1.3.0]` section
4. removes the pre-release sections and their link references

When `fold_prereleases` is off, the pre-release sections are kept and the final
release contains only the new fragments. If there are none, it repeats the
entries of its pre-release sections.

## Line endings and encoding

The changelog must be UTF-8. vers keeps the file's existing line endings (LF or
CRLF), and new files use LF.
