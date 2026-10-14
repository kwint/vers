//! One module per subcommand. Each exposes `run(ctx, args) -> Result<()>`.

pub mod check;
pub mod completions;
pub mod draft;
pub mod init;
pub mod migrate;
pub mod new;
pub mod release;
pub mod self_update;
pub mod status;
pub mod version;

use crate::cli::{Command, MigrateCommand, SelfCommand};
use crate::context::Context;
use crate::error::Result;

pub fn run(ctx: &Context, command: &Command) -> Result<()> {
    match command {
        Command::Init(args) => init::run(ctx, args),
        Command::New(args) => new::run(ctx, args),
        Command::Status(args) => status::run(ctx, args),
        Command::Draft(args) => draft::run(ctx, args),
        Command::Release(args) => release::run(ctx, args),
        Command::Check(args) => check::run(ctx, args),
        Command::Version(args) => version::run(ctx, args),
        Command::Migrate(MigrateCommand::Towncrier(args)) => migrate::run(ctx, args),
        Command::Completions(args) => completions::run(ctx, args),
        Command::SelfCmd(SelfCommand::Update) => self_update::run(ctx),
    }
}
