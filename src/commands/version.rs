use crate::cli::VersionArgs;
use crate::context::Context;
use crate::error::{Error, Result};

pub fn run(ctx: &Context, args: &VersionArgs) -> Result<()> {
    let _ = (ctx, args);
    Err(Error::Failed("`vers version` is not implemented yet".to_string()))
}
