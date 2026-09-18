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
    let contents =
        fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    let cfg: Config =
        toml::from_str(&contents).with_context(|| format!("parsing {}", path.display()))?;
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

/// Warn once on stderr if the helpdesk URL isn't HTTPS.
///
/// We carry a bearer token on every request, so plaintext hands it to anyone
/// on the path. We warn rather than refuse: a local or lab instance on
/// `http://localhost` is a legitimate thing to point at, and refusing would
/// just push people to a worse workaround.
pub fn warn_if_insecure(url: &str) {
    if url.starts_with("http://") {
        eprintln!(
            "warning: {url} is not HTTPS — your CLI token will be sent in cleartext on every request."
        );
    }
}

/// Resolve the helpdesk URL from CLI flag → env → config. Strips any trailing
/// slash so URL-joining doesn't double up.
pub fn resolve_helpdesk(cli: Option<String>, cfg: &Config) -> Result<String> {
    let raw = cli
        .or_else(|| std::env::var("CSSHD_HELPDESK").ok())
        .or_else(|| cfg.helpdesk.clone())
        .ok_or_else(|| {
            anyhow!("No helpdesk URL configured. Run `csshd login --helpdesk <url>` to set one.")
        })?;
    let trimmed = raw.trim().trim_end_matches('/').to_string();
    // Validate URL parse-ability.
    let _ =
        url::Url::parse(&trimmed).with_context(|| format!("invalid helpdesk URL: {trimmed}"))?;
    Ok(trimmed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg_with(url: Option<&str>) -> Config {
        Config {
            helpdesk: url.map(str::to_string),
            last_user: None,
        }
    }

    #[test]
    fn cli_flag_wins_and_trailing_slash_is_stripped() {
        let got = resolve_helpdesk(
            Some("https://flag.example.com/".into()),
            &cfg_with(Some("https://config.example.com")),
        )
        .unwrap();
        assert_eq!(got, "https://flag.example.com");
    }

    #[test]
    fn falls_back_to_config() {
        let got = resolve_helpdesk(None, &cfg_with(Some("https://config.example.com"))).unwrap();
        assert_eq!(got, "https://config.example.com");
    }

    #[test]
    fn missing_url_is_an_error_that_names_the_fix() {
        let err = resolve_helpdesk(None, &cfg_with(None))
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("csshd login --helpdesk"),
            "unhelpful error: {err}"
        );
    }

    #[test]
    fn garbage_url_is_rejected() {
        assert!(resolve_helpdesk(Some("not a url".into()), &cfg_with(None)).is_err());
    }

    /// TOML has no null, so a `None` field has to be omitted rather than
    /// written. Serializers differ on whether that is an error, and this
    /// crate took a major version bump (toml 0.8 → 1.1) — so pin the shape
    /// the config file actually needs to round-trip through.
    #[test]
    fn config_round_trips_through_toml() {
        let cfg = Config {
            helpdesk: Some("https://helpdesk.example.com".into()),
            last_user: Some("someone@example.com".into()),
        };
        let text = toml::to_string_pretty(&cfg).expect("serialize");
        let back: Config = toml::from_str(&text).expect("deserialize");
        assert_eq!(
            back.helpdesk.as_deref(),
            Some("https://helpdesk.example.com")
        );
        assert_eq!(back.last_user.as_deref(), Some("someone@example.com"));
    }

    /// The state `csshd login` writes on a first run: a helpdesk URL but no
    /// user yet, because `whoami` has not been called. If serializing a
    /// `None` field errors, first login fails at the point it saves.
    #[test]
    fn config_with_unset_fields_serializes() {
        let cfg = Config {
            helpdesk: Some("https://helpdesk.example.com".into()),
            last_user: None,
        };
        let text = toml::to_string_pretty(&cfg).expect("serialize with a None field");
        let back: Config = toml::from_str(&text).expect("deserialize");
        assert_eq!(
            back.helpdesk.as_deref(),
            Some("https://helpdesk.example.com")
        );
        assert_eq!(back.last_user, None);
    }

    #[test]
    fn empty_config_round_trips() {
        let text = toml::to_string_pretty(&Config::default()).expect("serialize default");
        let back: Config = toml::from_str(&text).expect("deserialize");
        assert_eq!(back.helpdesk, None);
        assert_eq!(back.last_user, None);
    }

    /// A config file written by an older csshd, or hand-edited with extra
    /// keys, must not break the client.
    #[test]
    fn unknown_keys_are_ignored() {
        let back: Config =
            toml::from_str("helpdesk = \"https://x.example.com\"\nsomething_new = 42\n")
                .expect("deserialize with an unknown key");
        assert_eq!(back.helpdesk.as_deref(), Some("https://x.example.com"));
    }
}
