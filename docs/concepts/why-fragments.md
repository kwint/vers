# Why fragments?

There are three common ways to keep a changelog. vers uses the third.

## 1. Edit `CHANGELOG.md` directly

Each merge request adds a line under `## [Unreleased]`.

This is simple, but every branch edits the same few lines of the same file, so
every pair of open merge requests is likely to conflict. Teams end up
postponing the changelog until release time, and by then nobody remembers what
changed.

## 2. Generate it from commit messages

Tools such as git-cliff and release-please build the changelog from commit
messages, usually in the Conventional Commits format.

This needs no extra files, but it means:

- **Commit messages serve two audiences.** A good commit message explains a code
  change to a developer. A good changelog entry explains a behaviour change to a
  user. These are rarely the same sentence.
- **Mistakes are hard to fix.** Once a commit is merged, its message can't be
  changed without rewriting history.
- **Squash and rebase workflows change the result.**

## 3. Fragments

Each change adds its own small file. At release time the files are combined
into the changelog and deleted. This approach comes from
[towncrier](https://towncrier.readthedocs.io/).

- **No conflicts.** Every branch adds a new file with a unique name.
- **Reviewed with the code.** The changelog entry is part of the diff, so
  reviewers can check that it describes the change correctly.
- **Easy to fix.** Before a release, a fragment is just a file. Edit it in any
  later merge request.
- **Written for users.** A fragment has one job: to tell users what changed.
- **Enforceable.** `vers check` fails CI when a change has no fragment. Because
  each fragment belongs to a package, a monorepo gets that check per package.

The cost is one extra file per change. `vers new` makes that a single command.

## Why Keep a Changelog?

[Keep a Changelog](https://keepachangelog.com/en/1.0.0/) is the most widely
recognised changelog format. It has a fixed set of change types, newest release
first, ISO dates and links between versions. Following it by default means:

- readers know where to look
- vers can read the changelog back reliably, which is what lets it use the
  changelog as the record of released versions (see
  [How vers chooses a version](versioning.md))
- you don't have to design a format yourself

towncrier's defaults (types like `feature`, `bugfix` and `misc`; reStructuredText
output) predate Keep a Changelog. vers starts from the standard instead.
