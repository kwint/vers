use crate::cli::CheckArgs;
use crate::context::Context;
use crate::error::{Error, Result};

pub fn run(ctx: &Context, args: &CheckArgs) -> Result<()> {
    let _ = (ctx, args);
    Err(Error::Failed("`vers check` is not implemented yet".to_string()))
}
