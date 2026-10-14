//! `vers self update`: replace a vers installed by the standalone installer
//! with the latest release from GitHub.
//!
//! Spec: `docs/reference/cli.md#vers-self-update`, `DESIGN.md` §9.
//!
//! The shell and PowerShell installers built by `dist` write an install
//! receipt (`~/.config/vers-changelog/vers-changelog-receipt.json`, or
//! `%LOCALAPPDATA%\vers-changelog\` on Windows). `axoupdater` reads it to find
//! the release source and install location, then runs the latest installer.
//! Without a receipt for this executable, vers was installed some other way and
//! has to be updated through that package manager.
//!
//! The updater is behind the default-on `self-update` cargo feature, so
//! packagers can build without it.

use crate::context::Context;
use crate::error::{Error, Result};

/// The name the installers use for the receipt: the package name, not the
/// binary name.
#[cfg(feature = "self-update")]
const RECEIPT_APP_NAME: &str = env!("CARGO_PKG_NAME");

fn not_installer_install() -> Error {
    Error::Failed(
        "`vers self update` is only available when vers was installed with the standalone installer; \
         update it through your package manager instead"
            .to_string(),
    )
}

#[cfg(not(feature = "self-update"))]
pub fn run(ctx: &Context) -> Result<()> {
    let _ = ctx;
    Err(not_installer_install())
}

#[cfg(feature = "self-update")]
pub fn run(ctx: &Context) -> Result<()> {
    use axoupdater::{AxoUpdater, AxoupdateError};
    use serde::Serialize;

    #[derive(Serialize)]
    struct SelfUpdateJson {
        updated: bool,
        old_version: String,
        new_version: String,
    }

    let mut updater = AxoUpdater::new_for(RECEIPT_APP_NAME);
    match updater.load_receipt() {
        Ok(_) => {}
        Err(AxoupdateError::NoReceipt { .. }) => return Err(not_installer_install()),
        Err(e) => return Err(Error::Failed(format!("can't read the install receipt: {e}"))),
    }
    // A receipt from an installer install can exist while this binary came
    // from a package manager.
    if !updater.check_receipt_is_for_this_executable().unwrap_or(false) {
        return Err(not_installer_install());
    }
    if ctx.quiet || ctx.json_output() {
        updater.disable_installer_output();
    }

    let current = env!("CARGO_PKG_VERSION").to_string();
    ctx.verbose("checking GitHub releases of kwint/vers for a newer version");
    let result = updater.run_sync().map_err(|e| Error::Failed(format!("self update failed: {e}")))?;

    let (updated, new_version) = match &result {
        Some(r) => (true, r.new_version.to_string()),
        None => (false, current.clone()),
    };
    if ctx.json_output() {
        ctx.json(&SelfUpdateJson { updated, old_version: current, new_version });
    } else if updated {
        ctx.line(format!("Updated vers {current} -> {new_version}"));
    } else {
        ctx.line(format!("vers {current} is already the latest version"));
    }
    Ok(())
}
