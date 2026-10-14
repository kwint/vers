# vers documentation

vers turns small change notes, called *fragments*, into a
[Keep a Changelog](https://keepachangelog.com/en/1.0.0/) section and the next
[SemVer](https://semver.org/) version. It works with any language, in a single
repo or a monorepo.

The docs are split into four kinds, following [Diátaxis](https://diataxis.fr/).
Pick the one that matches what you need right now.

## Tutorials

*Learning by doing.* Start here if you're new to vers.

- [Your first release](tutorials/first-release.md): set up vers in a small
  project, add fragments and make two releases.
- [Set up a monorepo](tutorials/monorepo.md): share config between packages and
  release them independently.

## How-to guides

*Recipes for a specific task.* These assume you know the basics.

- [Install vers](how-to/install.md): on a laptop, in CI, pinned per project.
- [Link issues and merge requests](how-to/link-issues-and-merge-requests.md):
  GitHub, GitLab, Jira and custom trackers.
- [Manage version files](how-to/manage-version-files.md): Cargo.toml,
  package.json, pyproject.toml, plain files and regexes.
- [Control the release version](how-to/control-the-release-version.md): bump
  manually, pre-releases, breaking changes.
- [Commit and tag releases](how-to/commit-and-tag-releases.md).
- [Require fragments in CI](how-to/require-fragments-in-ci.md): GitHub Actions
  and GitLab CI.
- [Customise change types](how-to/customise-change-types.md).
- [Migrate from towncrier](how-to/migrate-from-towncrier.md).

## Reference

*Exact, complete descriptions.* Look things up here.

- [Command line](reference/cli.md)
- [Configuration (`vers.toml`)](reference/configuration.md)
- [Fragment format](reference/fragments.md)
- [Changelog format](reference/changelog.md)

## Concepts

*Background and reasoning.* Read these to understand why vers works the way it
does.

- [Why fragments?](concepts/why-fragments.md)
- [How vers chooses a version](concepts/versioning.md)
- [Monorepos and config discovery](concepts/monorepos.md)
