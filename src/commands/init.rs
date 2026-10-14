use crate::cli::InitArgs;
use crate::context::Context;
use crate::error::{Error, Result};

pub fn run(ctx: &Context, args: &InitArgs) -> Result<()> {
    let _ = (ctx, args);
    Err(Error::Failed("`vers init` is not implemented yet".to_string()))
}
