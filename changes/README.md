# Changes

Every pull request that changes how vers behaves adds a *fragment* here: a small
Markdown file describing the change for users. vers turns them into
`CHANGELOG.md` at release time.

```sh
vers new fixed 123 -m "Don't crash when the config file is empty."
vers new added -m "Support YAML frontmatter."
```

See [Your first release](https://kwint.github.io/vers/tutorials/first-release/)
for how it works, and the
[fragment reference](https://kwint.github.io/vers/reference/fragments/) for the
format.
