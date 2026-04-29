use anyhow::Result;

use crate::{config, credentials};

pub async fn run() -> Result<()> {
    let cfg = config::load().unwrap_or_default();
    if let Some(url) = &cfg.helpdesk {
        credentials::clear_token(url)?;
        println!("Cleared CLI token for {url}.");
    } else {
        println!("No helpdesk configured — nothing to clear.");
    }
    Ok(())
}
