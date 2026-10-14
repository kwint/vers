# Fragment format reference

A fragment is a Markdown file in a package's fragment directory (`changes/` by
default) that describes one change.

## Filename

```
<name>.<type>[.<counter>].md
```

| Part | Rules |
|---|---|
| `name` | Letters, digits, `-` and `_`. Either a [reference](#references) or a free-form slug. |
| `type` | A change type name or alias (see [configuration](configuration.md#types)). Case-insensitive. |
| `counter` | Optional positive integer, so you can have several fragments with the same name and type. |

Files in the fragment directory that don't end in `.md` are ignored, including
`.gitkeep`. Subdirectories aren't searched.

## References

vers tries each configured reference kind's `match` regex against `name`, in
order. If one matches, the fragment is linked to that reference.

If none matches:

- if `name` looks like a reference (it's all digits, or it has a `<letters>-<digits>` shape such as `mr-45` or `ABC-12`), vers reports an error, because it's probably a typo or a missing reference kind
- otherwise `name` is a slug and isn't rendered

## Content

```markdown
---
breaking: true
refs: ["mr-130", "PROJ-7"]
---
The `timeout` setting is now in seconds instead of milliseconds.

Update your config: `timeout = 30000` becomes `timeout = 30`.
```

### Frontmatter

Optional. Use YAML between `---` lines or TOML between `+++` lines.

| Key | Type | Description |
|---|---|---|
| `breaking` | bool | Marks a breaking change: implies a major bump and adds `breaking_prefix`. |
| `refs` | string array | Extra references. Ones that match no reference kind are rendered as plain text, with a warning. |

Unknown keys are an error, so typos get caught.

### Body

- Markdown. It must not be empty.
- Leading and trailing blank lines are removed.
- A leading `- ` or `* ` is removed, so you don't end up with a bullet inside a
  bullet.
- The first line becomes the bullet's text. Following lines are indented by two
  spaces so they stay inside the bullet's list item.

## Rendering

Each fragment becomes one list item under its type's heading:

```markdown
### Changed

- **Breaking:** The `timeout` setting is now in seconds instead of milliseconds.

  Update your config: `timeout = 30000` becomes `timeout = 30`. ([#88](…), [!130](…), [PROJ-7](…))
```

References are added to the end of the last paragraph (or to the start of the
first, with `ref_position = "start"`). Their order is: the filename reference
first, then `refs` in the order written.

Fragments of the same type with identical bodies after normalisation are merged
into one entry, with their references combined in fragment order.

## Order

Within a type, entries are sorted by when each fragment was added:

1. The author time of the commit that first added the file. If the repository is
   a shallow clone, vers prints a warning.
2. For files with no commit yet (untracked or only staged): the file's creation
   time on disk.
3. If neither is known: alphabetically by filename.

Ties are broken alphabetically by filename.
