use crate::cli::StatusArgs;
use crate::context::Context;
use crate::error::{Error, Result};

pub fn run(ctx: &Context, args: &StatusArgs) -> Result<()> {
    let _ = (ctx, args);
    Err(Error::Failed("`vers status` is not implemented yet".to_string()))
}
