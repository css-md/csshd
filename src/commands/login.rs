use anyhow::{anyhow, Context, Result};

use crate::{auth, client::Client, config, credentials};

pub async fn run(helpdesk_arg: Option<String>) -> Result<()> {
    // First-run case: helpdesk arg is required if config doesn't have one.
    let mut cfg = config::load().unwrap_or_default();
    let helpdesk = match helpdesk_arg {
        Some(url) => url.trim().trim_end_matches('/').to_string(),
        None => cfg
            .helpdesk
            .clone()
            .ok_or_else(|| {
                anyhow!(
                    "First login — pass --helpdesk <url> (e.g. https://helpdesk.example.com).\n\
                     Subsequent logins remember the URL."
                )
            })?,
    };

    // Validate URL parse-ability before contacting it.
    let _ = url::Url::parse(&helpdesk).with_context(|| format!("invalid URL: {helpdesk}"))?;

    let client = Client::new(&helpdesk, None)?;
    let token = auth::login(&client).await?;

    credentials::store_token(&helpdesk, &token.access_token)
        .context("storing token in OS keychain")?;
    cfg.helpdesk = Some(helpdesk.clone());
    config::save(&cfg)?;

    // Sanity-check by hitting whoami with the new token.
    let authed = Client::new(&helpdesk, Some(token.access_token.clone()))?;
    match authed.whoami().await {
        Ok(me) => {
            cfg.last_user = Some(me.email.clone());
            config::save(&cfg)?;
            println!(
                "Signed in as {} ({}). Token expires {}.",
                me.name.unwrap_or(me.email.clone()),
                me.email,
                token.expires_at.format("%b %d, %Y")
            );
        }
        Err(e) => {
            println!("Token saved, but whoami sanity-check failed: {e}");
        }
    }
    Ok(())
}
