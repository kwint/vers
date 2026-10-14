# Customise change types

The default types are the six Keep a Changelog categories. Stick with them if
you can, because readers of your changelog will know them. If you need something
else, this guide shows how.

## Add a type

Defining `[[types]]` **replaces** the whole default list, so repeat the defaults
you want to keep. The order in the config is the order of the sections in the
changelog.

```toml
[[types]]
name = "added"
heading = "Added"
aliases = ["feat", "feature"]
bump = "minor"

[[types]]
name = "changed"
heading = "Changed"
bump = "patch"

[[types]]
name = "deprecated"
heading = "Deprecated"
bump = "minor"

[[types]]
name = "removed"
heading = "Removed"
bump = "major"

[[types]]
name = "fixed"
heading = "Fixed"
aliases = ["fix", "bugfix"]
bump = "patch"

[[types]]
name = "security"
heading = "Security"
bump = "patch"

[[types]]
name = "performance"
heading = "Performance"
aliases = ["perf"]
bump = "patch"
```

## A type that's recorded but not released

Use `bump = "none"` together with `hidden = true` for changes that shouldn't
appear in the changelog, such as internal refactors you still want reviewers to
document. `vers check` accepts them, and `vers release` deletes them without
rendering them.

```toml
[[types]]
name = "internal"
heading = "Internal"
bump = "none"
hidden = true
```

A release that contains only hidden fragments doesn't happen. vers says
"nothing to release" instead.

## Share types across a monorepo

Define the types in the root `vers.toml` and `extend` it from each package. See
[Monorepos and config discovery](../concepts/monorepos.md).
