//! OS-keychain wrapper for the csshd bearer token.
//!
//! The `keyring` crate gives us a single API across macOS Keychain, Windows
//! Credential Manager, and the Linux Secret Service (libsecret). We key by
//! helpdesk URL so a user with multiple installs (unlikely, but possible)
//! can keep tokens distinct.
//!
//! The keychain is a hard requirement. If it's unavailable (e.g. headless
//! Linux without dbus-secret-service), the user gets a helpful error rather
//! than a plaintext fallback. **Never** add a file-based fallback — that
//! would be a real security regression.

use anyhow::{Context, Result};
use keyring::Entry;

const SERVICE: &str = "csshd";

fn entry(helpdesk: &str) -> Result<Entry> {
    Entry::new(SERVICE, helpdesk)
        .with_context(|| format!("keyring open ({SERVICE} / {helpdesk})"))
}

pub fn store_token(helpdesk: &str, token: &str) -> Result<()> {
    entry(helpdesk)?
        .set_password(token)
        .with_context(|| "keyring write failed")?;
    Ok(())
}

pub fn load_token(helpdesk: &str) -> Result<Option<String>> {
    match entry(helpdesk)?.get_password() {
        Ok(t) => Ok(Some(t)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(e).context("keyring read failed"),
    }
}

pub fn clear_token(helpdesk: &str) -> Result<()> {
    match entry(helpdesk)?.delete_credential() {
        Ok(()) => Ok(()),
        Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(e).context("keyring delete failed"),
    }
}
