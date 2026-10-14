# Link issues and merge requests

vers can turn a fragment's name into a link to an issue, a merge request or a
ticket. This guide shows how to set that up.

For how references are matched, see the
[fragment format reference](../reference/fragments.md#references).

## GitHub or GitLab

If `origin` points to github.com or gitlab.com, there's nothing to configure.
vers detects the host and uses that platform's preset:

| You name the fragment | GitHub renders | GitLab renders |
|---|---|---|
| `123.fixed.md`  | `#123` → `/issues/123` | `#123` → `/-/issues/123` |
| `pr-45.added.md` | `#45` → `/pull/45` | n/a |
| `mr-45.added.md` | n/a | `!45` → `/-/merge_requests/45` |

For a self-hosted GitLab or GitHub Enterprise, vers can't tell the platform from
the hostname, so set it yourself:

```toml
[changelog]
repo = "gitlab"
repo_url = "https://gitlab.example.com/team/project"
```

## Create a fragment with a reference

```sh
vers new fixed 123 -m "Fix the login redirect."          # → changes/123.fixed.md
vers new added mr-45 -m "Add dark mode."                 # → changes/mr-45.added.md
```

## Add more than one reference

Put the main reference in the filename and add the others with `--ref`:

```sh
vers new fixed 123 --ref mr-130 -m "Fix the login redirect."
```

This writes the extra references to the fragment's frontmatter:

```markdown
---
refs: ["mr-130"]
---
Fix the login redirect.
```

It renders as `- Fix the login redirect. ([#123](…), [!130](…))`.

## Add more than one fragment for the same reference

Add a counter before `.md`:

```
changes/123.fixed.md
changes/123.fixed.2.md
```

`vers new` adds the counter for you if the file already exists.

## Link Jira (or another tracker)

Add a reference kind to `vers.toml`. It's merged with the preset's kinds:

```toml
[refs.jira]
match   = '^(?<id>[A-Z][A-Z0-9]+-\d+)$'
display = "{id}"
url     = "https://your-company.atlassian.net/browse/{id}"
```

Now `PROJ-42.changed.md` renders as `([PROJ-42](https://your-company.atlassian.net/browse/PROJ-42))`.

The order of kinds matters: vers tries them in the order they appear in the
config, with the preset's kinds first, and uses the first match.

## Put references before the text

```toml
[changelog]
ref_position = "start"
```

This renders `- [#123](…) Fix the login redirect.`

## Turn references off

```toml
[changelog]
repo = "none"
```

With no reference kinds configured, every fragment name is treated as a plain
slug.
