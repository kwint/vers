//! vers: fresh changelogs for every release.
//!
//! The library is organised as core modules (`config`, `fragment`,
//! `changelog`, `version`, `git`) and one module per command under
//! `commands`. User-facing behaviour is specified by the docs in `docs/`;
//! see `DESIGN.md` for the architecture.

pub mod changelog;
pub mod cli;
pub mod commands;
pub mod config;
pub mod context;
pub mod error;
pub mod fragment;
pub mod git;
pub mod version;

pub use error::{Error, Result};
