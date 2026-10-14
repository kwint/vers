use crate::context::Context;
use crate::error::{Error, Result};

pub fn run(ctx: &Context) -> Result<()> {
    let _ = ctx;
    Err(Error::Failed(
        "`vers self update` is only available when vers was installed with the standalone installer; \
         update it through your package manager instead"
            .to_string(),
    ))
}
