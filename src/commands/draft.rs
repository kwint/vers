use crate::cli::DraftArgs;
use crate::context::Context;
use crate::error::{Error, Result};

pub fn run(ctx: &Context, args: &DraftArgs) -> Result<()> {
    let _ = (ctx, args);
    Err(Error::Failed("`vers draft` is not implemented yet".to_string()))
}

/// The release section `plan` would add, or `None` when there is nothing to
/// release. Fragments of hidden types are left out; with `fold_prereleases`,
/// a final version also takes the entries of its pre-release sections.
pub fn build_release_for(plan: &crate::plan::PackagePlan) -> Result<Option<crate::changelog::Release>> {
    let Some(next) = &plan.next else { return Ok(None) };
    let visible: Vec<crate::fragment::Fragment> = plan.visible_fragments().cloned().collect();
    let config = &plan.config;
    let mut release = crate::changelog::build_release(
        next.next.clone(),
        crate::context::release_date(),
        &visible,
        &config.types,
        config,
    );
    if config.version.fold_prereleases
        && next.next.pre.is_empty()
        && let Some(changelog) = &plan.changelog
    {
        release.merge_earlier(changelog.prerelease_sections(&next.next), &config.types);
    }
    Ok(Some(release))
}
