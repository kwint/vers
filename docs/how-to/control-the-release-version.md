# Control the release version

By default vers works out the next version from your fragments. This guide shows
the other ways to set it. For the full rules, see
[How vers chooses a version](../concepts/versioning.md).

## See which version will be released

```sh
vers version --next
vers status            # also says where the version comes from
```

## Set the version on the command line

```sh
vers release --version 2.0.0
vers release --bump major
```

The version must be newer than the latest release in the changelog.

## Set the version in your files first

Bump the version in your version files, for example with `npm version minor
--no-git-tag-version`. vers sees that it's newer than the changelog and releases
it as it is:

```console
$ vers status
web  0.3.1 → 0.4.0 (from package.json)  2 fragments: 2 fixed
```

## Mark a breaking change

Pass `--breaking` when you create the fragment:

```sh
vers new changed --breaking -m "The \`timeout\` setting is now in seconds."
```

A breaking fragment always means a major bump. The entry gets a `**Breaking:**`
prefix in the changelog, which you can change with `[changelog] breaking_prefix`.

On `0.x` versions a breaking change bumps the minor version instead (`0.4.2` →
`0.5.0`). To bump to `1.0.0`, use `--version 1.0.0`.

## Make a pre-release

```sh
vers release --pre rc       # 1.2.0 → 1.3.0-rc.1
vers release --pre rc       # 1.3.0-rc.1 → 1.3.0-rc.2
vers release                # 1.3.0-rc.2 → 1.3.0
```

Each pre-release gets its own changelog section. The final release combines all
of its pre-release sections into one `## [1.3.0]` section and removes the
`-rc` sections. To keep them, set `[version] fold_prereleases = false`.

A final release after a pre-release doesn't need new fragments.

## Preview without changing anything

```sh
vers release --dry-run
```

This prints the changes to every file as a diff and exits without writing.
