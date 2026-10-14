use clap::CommandFactory;

use crate::cli::{Cli, CompletionsArgs};
use crate::context::Context;
use crate::error::Result;

pub fn run(_ctx: &Context, args: &CompletionsArgs) -> Result<()> {
    clap_complete::generate(args.shell, &mut Cli::command(), "vers", &mut std::io::stdout());
    Ok(())
}
