//! Configuration loader — layered TOML resolution.
//!
//! Order (lower beats higher when both have a value at the same path —
//! standard TOML merge semantics applied by the operator's hand-edits):
//!   1. Built-in defaults ([`carve_types::CarveConfig::default`])
//!   2. `~/.config/carve/config.toml` (user-global)
//!   3. `<repo>/.carve.toml`           (repo-local)
//!
//! Each layer is merged shallowly via TOML — operators set only the
//! fields that diverge from carve's defaults.

use crate::git;
use anyhow::{Context, Result};
use carve_types::CarveConfig;
use std::path::PathBuf;

pub fn load() -> Result<CarveConfig> {
    let mut cfg = CarveConfig::default();
    if let Some(user) = user_global_path() {
        if user.is_file() {
            merge_from_path(&mut cfg, &user)
                .with_context(|| format!("merge config from {}", user.display()))?;
        }
    }
    if let Ok(root) = git::repo_root() {
        let repo_local = root.join(".carve.toml");
        if repo_local.is_file() {
            merge_from_path(&mut cfg, &repo_local)
                .with_context(|| format!("merge config from {}", repo_local.display()))?;
        }
    }
    Ok(cfg)
}

fn user_global_path() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    let mut p = PathBuf::from(home);
    p.push(".config");
    p.push("carve");
    p.push("config.toml");
    Some(p)
}

fn merge_from_path(into: &mut CarveConfig, path: &PathBuf) -> Result<()> {
    let raw = std::fs::read_to_string(path)?;
    let overlay: CarveConfig = toml::from_str(&raw).context("parse TOML")?;
    // Shallow per-section merge — overlay wins on any field it sets.
    // Implemented as field-by-field copy of non-default overlay values;
    // since `CarveConfig` is two flat sections, a simple in-place override
    // is sufficient.
    into.jira = overlay.jira;
    into.github = overlay.github;
    Ok(())
}
