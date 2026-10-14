use crate::cli::NewArgs;
use crate::context::Context;
use crate::error::{Error, Result};

pub fn run(ctx: &Context, args: &NewArgs) -> Result<()> {
    let _ = (ctx, args);
    Err(Error::Failed("`vers new` is not implemented yet".to_string()))
}
