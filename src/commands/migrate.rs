use crate::cli::{InitArgs, MigrateTowncrierArgs};
use crate::context::Context;
use crate::error::{Error, Result};

pub fn run(ctx: &Context, args: &MigrateTowncrierArgs) -> Result<()> {
    let _ = (ctx, args);
    Err(Error::Failed("`vers migrate towncrier` is not implemented yet".to_string()))
}

/// `vers init --from-towncrier`: called by the init command.
pub fn init_from_towncrier(ctx: &Context, args: &InitArgs) -> Result<()> {
    let _ = (ctx, args);
    Err(Error::Failed("`vers init --from-towncrier` is not implemented yet".to_string()))
}
