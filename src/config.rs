//! Persistent CLI config — just the helpdesk URL and an optional last-used
//! username for display. Lives at the platform's standard config dir:
//!
//!   Linux / BSD:  $XDG_CONFIG_HOME/csshd/config.toml
//!                 (default: ~/.config/csshd/config.toml)
//!   macOS:        ~/Library/Application Support/csshd/config.toml
//!   Windows:      %APPDATA%\csshd\config.toml
//!
//! Tokens never live here — those go in the OS keychain via `crate::credentials`.

use anyhow::{anyhow, Context, Result};
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Helpdesk base URL, e.g. https://helpdesk.example.com (no trailing slash).
    pub helpdesk: Option<String>,
    /// Last-known signed-in user — purely cosmetic, refreshed by `whoami`.
    pub last_user: Option<String>,
}

fn project_dirs() -> Result<ProjectDirs> {
    ProjectDirs::from("org", "css-md", "csshd")
        .ok_or_else(|| anyhow!("Could not resolve a config directory for this user"))
}

pub fn config_path() -> Result<PathBuf> {
    Ok(project_dirs()?.config_dir().join("config.toml"))
}

pub fn load() -> Result<Config> {
    let path = config_path()?;
    if !path.exists() {
        return Ok(Config::default());
    }
    let contents = fs::read_to_string(&path)
        .with_context(|| format!("reading {}", path.display()))?;
    let cfg: Config = toml::from_str(&contents)
        .with_context(|| format!("parsing {}", path.display()))?;
    Ok(cfg)
}

pub fn save(cfg: &Config) -> Result<()> {
    let path = config_path()?;
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    }
    let contents = toml::to_string_pretty(cfg).context("serializing config")?;
    fs::write(&path, contents).with_context(|| format!("writing {}", path.display()))?;
    Ok(())
}

/// Resolve the helpdesk URL from CLI flag → env → config. Strips any trailing
/// slash so URL-joining doesn't double up.
pub fn resolve_helpdesk(cli: Option<String>, cfg: &Config) -> Result<String> {
    let raw = cli
        .or_else(|| std::env::var("CSSHD_HELPDESK").ok())
        .or_else(|| cfg.helpdesk.clone())
        .ok_or_else(|| {
            anyhow!(
                "No helpdesk URL configured. Run `csshd login --helpdesk <url>` to set one."
            )
        })?;
    let trimmed = raw.trim().trim_end_matches('/').to_string();
    // Validate URL parse-ability.
    let _ = url::Url::parse(&trimmed)
        .with_context(|| format!("invalid helpdesk URL: {trimmed}"))?;
    Ok(trimmed)
}
