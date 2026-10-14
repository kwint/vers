use crate::cli::DraftArgs;
use crate::context::Context;
use crate::error::{Error, Result};

pub fn run(ctx: &Context, args: &DraftArgs) -> Result<()> {
    let _ = (ctx, args);
    Err(Error::Failed("`vers draft` is not implemented yet".to_string()))
}
