# Configuration reference

vers is configured with `vers.toml` or `.vers.toml`. If both exist in the same
directory, `.vers.toml` is used. For how configs are found and combined, see
[Monorepos and config discovery](../concepts/monorepos.md).

Relative paths are always resolved against the directory of the file that
contains them.

Every key is optional. The defaults are shown below.

## Top-level keys

### `extend`

Path to another config to inherit from. The current config's keys override the
inherited ones: tables are merged key by key, and arrays (such as
`[[version.files]]` or `[[types]]`) are replaced as a whole. Chains of `extend`
are allowed. Cycles are an error.

```toml
extend = "../../vers.toml"
```

### `required_version`

A [SemVer requirement](https://docs.rs/semver/latest/semver/struct.VersionReq.html)
that the running vers must meet, otherwise vers exits with code 2.

```toml
required_version = ">=1.2, <2"
```

---

## `[package]`

**This table's presence makes the directory a package.** Without it, the config
only provides settings for others to `extend`, and fragments under it are an
error. An empty `[package]` table is enough. The table belongs to the file itself: it is never
inherited through `extend`.

| Key | Type | Default | Description |
|---|---|---|---|
| `name` | string | directory name | Package name, used in CLI arguments, tags and output. Must be unique in the repository. |

---

## `[changelog]`

| Key | Type | Default | Description |
|---|---|---|---|
| `path` | path | `"CHANGELOG.md"` | The changelog file. |
| `repo` | `"auto"` \| `"github"` \| `"gitlab"` \| `"none"` | `"auto"` | Preset for references and links. `auto` detects the host from the `origin` remote, and falls back to `none` if it's not github.com or gitlab.com. |
| `repo_url` | string | from `origin` | Web URL of the repository. SSH remotes are converted (`git@host:a/b.git` → `https://host/a/b`). |
| `compare_url` | string | from preset | Link for a version, with `{from}` and `{to}` (tags) as placeholders. |
| `release_url` | string | from preset | Link for the first release, which has nothing to compare against, with `{tag}` as a placeholder. |
| `ref_position` | `"end"` \| `"start"` | `"end"` | Where references go in each entry. |
| `breaking_prefix` | string | `"**Breaking:** "` | Prefix for entries from breaking fragments. |

Preset values:

| | `compare_url` | `release_url` |
|---|---|---|
| `github` | `{repo_url}/compare/{from}...{to}` | `{repo_url}/releases/tag/{tag}` |
| `gitlab` | `{repo_url}/-/compare/{from}...{to}` | `{repo_url}/-/tags/{tag}` |
| `none`   | no links | no links |

---

## `[fragments]`

| Key | Type | Default | Description |
|---|---|---|---|
| `directory` | path | `"changes"` | Where fragments live. |

---

## `[[types]]`

The change types. Defining any `[[types]]` replaces the default list entirely.
The order here is the order of sections in the changelog.

| Key | Type | Default | Description |
|---|---|---|---|
| `name` | string | required | Canonical name, used in filenames. |
| `heading` | string | required | Section heading in the changelog. |
| `aliases` | string array | `[]` | Other names accepted in filenames and by `vers new`. |
| `bump` | `"major"` \| `"minor"` \| `"patch"` \| `"none"` | `"patch"` | The bump this type implies. |
| `hidden` | bool | `false` | Accept the fragment and remove it on release, but don't render it. |

Defaults:

| `name` | `heading` | `aliases` | `bump` |
|---|---|---|---|
| `added` | Added | `feat`, `feature` | minor |
| `changed` | Changed | `change` | patch |
| `deprecated` | Deprecated | `deprecation` | minor |
| `removed` | Removed | `removal` | major |
| `fixed` | Fixed | `fix`, `bugfix` | patch |
| `security` | Security | `sec` | patch |

---

## `[refs.<kind>]`

Reference kinds, tried in order: the preset's kinds first, then the ones defined
here. A kind with the same name as a preset kind replaces it.

| Key | Type | Description |
|---|---|---|
| `match` | regex | Must match the whole fragment name and contain a named group `id`. |
| `display` | string | Link text, with `{id}` as a placeholder. |
| `url` | string | Link target, with `{id}` and `{repo_url}` as placeholders. |

Preset kinds:

| Preset | Kind | `match` | `display` | `url` |
|---|---|---|---|---|
| github | `issue` | `^(?<id>\d+)$` | `#{id}` | `{repo_url}/issues/{id}` |
| github | `pr` | `^pr-(?<id>\d+)$` | `#{id}` | `{repo_url}/pull/{id}` |
| gitlab | `issue` | `^(?<id>\d+)$` | `#{id}` | `{repo_url}/-/issues/{id}` |
| gitlab | `mr` | `^mr-(?<id>\d+)$` | `!{id}` | `{repo_url}/-/merge_requests/{id}` |

---

## `[version]`

| Key | Type | Default | Description |
|---|---|---|---|
| `initial` | version | `"0.1.0"` | First version when there's no changelog release, tag or version file. |
| `zero_major_shift` | bool | `true` | On `0.x`, a major bump becomes minor and a minor bump becomes patch. |
| `fold_prereleases` | bool | `true` | A final release combines its pre-release sections into one. |
| `tag` | bool | `false` | Commit the release and tag it. See [Commit and tag releases](../how-to/commit-and-tag-releases.md). |
| `tag_format` | string | `"v{version}"` | Tag name, with `{name}` and `{version}` as placeholders. Also used for compare links. |
| `commit_message` | string | `"Release {name} {version}"` | Commit message when `tag = true`. |

### `[[version.files]]`

| Key | Type | Description |
|---|---|---|
| `path` | path | The file. Required. |
| `key` | string | Dotted key path in a TOML, JSON or YAML file, such as `package.version`. |
| `pattern` | regex | Regex with a named group `version`. Must match exactly once. |

Give `key` or `pattern`, not both. With neither, the whole file (trimmed) is the
version.

---

## `[check]`

| Key | Type | Default | Description |
|---|---|---|---|
| `require_fragment` | bool | `true` | Require a fragment when this package changes. |
| `ignore` | glob array | `[]` | Changed files matching these globs don't count as package changes. Relative to this config. |

---

## Complete example

```toml
extend = "../../vers.toml"
required_version = ">=1.0"

[package]
name = "api"

[changelog]
repo = "gitlab"
repo_url = "https://gitlab.example.com/shop/shop"

[refs.jira]
match   = '^(?<id>SHOP-\d+)$'
display = "{id}"
url     = "https://example.atlassian.net/browse/{id}"

[version]
tag = true
tag_format = "{name}@v{version}"

[[version.files]]
path = "Cargo.toml"
key  = "package.version"

[check]
ignore = ["docs/**", "benches/**"]
```
