use crate::cli::ReleaseArgs;
use crate::context::Context;
use crate::error::{Error, Result};

pub fn run(ctx: &Context, args: &ReleaseArgs) -> Result<()> {
    let _ = (ctx, args);
    Err(Error::Failed("`vers release` is not implemented yet".to_string()))
}
