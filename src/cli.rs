//! Command-line interface definition. Spec: `docs/reference/cli.md`.

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};
use semver::Version;

use crate::version::Bump;

#[derive(Debug, Parser)]
#[command(name = "vers", version, about = "Fresh changelogs for every release.")]
pub struct Cli {
    #[command(flatten)]
    pub global: GlobalArgs,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Args, Clone)]
pub struct GlobalArgs {
    /// Use this config file instead of discovering one.
    #[arg(long, global = true, value_name = "PATH")]
    pub config: Option<PathBuf>,

    /// Output format.
    #[arg(long, global = true, value_enum, default_value_t = Format::Text)]
    pub format: Format,

    /// Coloured output.
    #[arg(long, global = true, value_enum, default_value_t = ColorChoice::Auto)]
    pub color: ColorChoice,

    /// Print errors only.
    #[arg(short, long, global = true, conflicts_with = "verbose")]
    pub quiet: bool,

    /// Print more detail. Repeat for debug output.
    #[arg(short, long, global = true, action = clap::ArgAction::Count)]
    pub verbose: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Format {
    Text,
    Json,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum ColorChoice {
    Auto,
    Always,
    Never,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Create a vers.toml in the current directory.
    Init(InitArgs),
    /// Create a fragment.
    New(NewArgs),
    /// Show pending fragments and the next version for each package.
    Status(StatusArgs),
    /// Print the changelog section the next release would add.
    Draft(DraftArgs),
    /// Release packages.
    Release(ReleaseArgs),
    /// Check a branch's changes for missing or invalid fragments.
    Check(CheckArgs),
    /// Print a package's current version.
    Version(VersionArgs),
    /// Migrate from another tool.
    #[command(subcommand)]
    Migrate(MigrateCommand),
    /// Print a shell completion script.
    Completions(CompletionsArgs),
    /// Manage the vers installation.
    #[command(name = "self", subcommand)]
    SelfCmd(SelfCommand),
}

#[derive(Debug, Args)]
pub struct InitArgs {
    /// Package name. Default: the directory name.
    #[arg(long)]
    pub name: Option<String>,
    /// Create a config without a [package] table, for other configs to extend.
    #[arg(long, conflicts_with = "name")]
    pub shared: bool,
    /// Don't detect version files or the repository host.
    #[arg(long)]
    pub no_detect: bool,
    /// Build the config from [tool.towncrier].
    #[arg(long)]
    pub from_towncrier: bool,
}

#[derive(Debug, Args)]
pub struct NewArgs {
    /// Change type or alias. Asked for in a terminal if left out.
    pub r#type: Option<String>,
    /// Fragment name: a reference (123, mr-45, PROJ-42) or a slug.
    pub name: Option<String>,
    /// An extra reference. Can be repeated.
    #[arg(long = "ref", value_name = "REF")]
    pub refs: Vec<String>,
    /// Fragment content. Without it, $VISUAL or $EDITOR is opened, or vers asks for it in a terminal.
    #[arg(short, long)]
    pub message: Option<String>,
    /// Mark the change as breaking.
    #[arg(long)]
    pub breaking: bool,
    /// Create the fragment in this package instead of the current one.
    #[arg(long, value_name = "NAME")]
    pub package: Option<String>,
}

#[derive(Debug, Args)]
pub struct StatusArgs {
    /// Packages to show. Default: every package below the current directory.
    pub packages: Vec<String>,
}

#[derive(Debug, Args, Clone, Default)]
pub struct VersionChoiceArgs {
    /// Release exactly this version.
    #[arg(long, value_name = "X.Y.Z", conflicts_with = "bump")]
    pub version: Option<Version>,
    /// Bump by this level instead of inferring it.
    #[arg(long, value_name = "LEVEL", value_parser = parse_bump)]
    pub bump: Option<Bump>,
    /// Make a pre-release with this identifier, e.g. rc.
    #[arg(long, value_name = "ID")]
    pub pre: Option<String>,
}

fn parse_bump(s: &str) -> Result<Bump, String> {
    match s.parse()? {
        Bump::None => Err("bump level must be major, minor or patch".to_string()),
        b => Ok(b),
    }
}

#[derive(Debug, Args)]
pub struct DraftArgs {
    /// Package. Default: the current package.
    pub package: Option<String>,
    #[command(flatten)]
    pub choice: VersionChoiceArgs,
}

#[derive(Debug, Args)]
pub struct ReleaseArgs {
    /// Packages to release. Default: the current package.
    pub packages: Vec<String>,
    /// Release every package at or below the current directory that has pending fragments.
    #[arg(long, conflicts_with = "packages")]
    pub all: bool,
    #[command(flatten)]
    pub choice: VersionChoiceArgs,
    /// Commit and tag the release (overrides [version] tag).
    #[arg(long, overrides_with = "no_tag")]
    pub tag: bool,
    /// Don't commit or tag (overrides [version] tag).
    #[arg(long, overrides_with = "tag")]
    pub no_tag: bool,
    /// Print the changes as a diff without writing anything.
    #[arg(long)]
    pub dry_run: bool,
    /// Skip the version file consistency checks.
    #[arg(long)]
    pub force: bool,
}

#[derive(Debug, Args)]
pub struct CheckArgs {
    /// Compare against this ref.
    #[arg(long, value_name = "REF")]
    pub base: Option<String>,
}

#[derive(Debug, Args)]
pub struct VersionArgs {
    /// Package. Default: the current package.
    pub package: Option<String>,
    /// Print the version `vers release` would release instead.
    #[arg(long)]
    pub next: bool,
}

#[derive(Debug, Subcommand)]
pub enum MigrateCommand {
    /// Rename towncrier fragments to vers types.
    Towncrier(MigrateTowncrierArgs),
}

#[derive(Debug, Args)]
pub struct MigrateTowncrierArgs {
    /// Show what would change without changing anything.
    #[arg(long)]
    pub dry_run: bool,
    /// Override the type mapping, e.g. misc=changed.
    #[arg(long = "map", value_name = "FROM=TO")]
    pub map: Vec<String>,
    /// Also rewrite towncrier-style release headings.
    #[arg(long)]
    pub changelog: bool,
}

#[derive(Debug, Args)]
pub struct CompletionsArgs {
    pub shell: clap_complete::Shell,
}

#[derive(Debug, Subcommand)]
pub enum SelfCommand {
    /// Update vers to the latest release.
    Update,
}

#[cfg(test)]
mod tests {
    use clap::CommandFactory;

    #[test]
    fn cli_definition_is_valid() {
        super::Cli::command().debug_assert();
    }
}
