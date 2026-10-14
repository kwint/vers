use crate::cli::MigrateTowncrierArgs;
use crate::context::Context;
use crate::error::{Error, Result};

pub fn run(ctx: &Context, args: &MigrateTowncrierArgs) -> Result<()> {
    let _ = (ctx, args);
    Err(Error::Failed("`vers migrate towncrier` is not implemented yet".to_string()))
}
