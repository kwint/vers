# Commit and tag releases

By default `vers release` only changes files. It doesn't commit, tag or push, so
you can review the result and commit it however your project prefers.

## Let vers commit and tag

```toml
[version]
tag = true
```

With this setting, `vers release`:

1. Writes the changelog and version files and removes the fragments, as usual.
2. Commits **only** the files it changed, with the message from
   `commit_message`.
3. Tags that commit using `tag_format`.

The output ends with the commit and the tag:

```console
$ vers release
Releasing app 1.3.0 (inferred: minor)
  Updated CHANGELOG.md
  Removed 2 fragments
  Committed 1a2b3c4
  Tagged v1.3.0
```

vers never pushes. Push when you're ready:

```sh
git push --follow-tags
```

Tagging always comes with a commit, because a tag has to point at a commit that
contains the release.

## Turn tagging on or off for one release

```sh
vers release --tag
vers release --no-tag
```

## Choose the tag name

```toml
[version]
tag_format = "v{version}"          # v1.3.0 (default)
# tag_format = "{name}@v{version}" # api@v1.3.0, the usual choice in a monorepo
```

`tag_format` is also used for the compare links at the bottom of the changelog.
Set it to match your existing tags even if vers doesn't create them.

## Change the commit message

```toml
[version]
commit_message = "chore(release): {name} {version}"
```

When `vers release --all` releases several packages, they share one commit. The
subject is `Release <n> packages`, and the body has one `commit_message` line per
package. Each package still gets its own tag.

## Sign tags and commits

vers runs `git`, so your git config applies. Set `tag.gpgSign` and
`commit.gpgSign` (or the SSH-signing equivalents) as usual.
