//! `vers version`: print a package's current or next version.
//!
//! Spec: `docs/reference/cli.md#vers-version`.

use serde::Serialize;

use crate::cli::{VersionArgs, VersionChoiceArgs};
use crate::commands::draft::{nothing_to_release, plan_with_warnings, select_one};
use crate::context::Context;
use crate::error::{Error, Result};

pub fn run(ctx: &Context, args: &VersionArgs) -> Result<()> {
    let package = select_one(ctx, args.package.as_deref())?;
    let name = package.name();
    let plan = plan_with_warnings(ctx, &package, &VersionChoiceArgs::default())?;
    let current = plan.current.as_ref().map(ToString::to_string);
    let next = plan.next.as_ref().map(|n| n.next.to_string());

    let shown = if args.next {
        next.clone().ok_or_else(|| nothing_to_release(name))?
    } else {
        current.clone().ok_or_else(|| Error::Failed(format!("`{name}` has no released version yet")))?
    };

    if ctx.json_output() {
        ctx.json(&VersionJson { package: name, current, next });
    } else {
        ctx.line(shown);
    }
    Ok(())
}

#[derive(Serialize)]
struct VersionJson<'a> {
    package: &'a str,
    current: Option<String>,
    next: Option<String>,
}
