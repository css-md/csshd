//! Device-code login flow against the helpdesk.
//!
//! Calls /api/v1/cli/auth/init, prints a code + URL, opens the user's
//! browser to the verification URL, then polls /api/v1/cli/auth/poll until
//! the user clicks Approve in the browser. On success, returns the bearer
//! token and lets the caller decide where to store it (we keep this layer
//! free of side effects).

use anyhow::{anyhow, bail, Result};
use owo_colors::{OwoColorize, Stream::Stdout};
use std::time::{Duration, Instant};
use tokio::time::sleep;

use crate::client::{Client, TokenResponse};

pub async fn login(client: &Client) -> Result<TokenResponse> {
    let init = client.auth_init(Some("csshd CLI")).await?;
    let deadline = Instant::now() + Duration::from_secs(init.expires_in as u64);

    println!();
    println!(
        "  Open this URL in your browser:\n    {}",
        init.verification_uri_complete
            .if_supports_color(Stdout, |s| s.bold().underline().to_string())
    );
    println!();
    println!(
        "  Enter the code:  {}",
        init.user_code
            .if_supports_color(Stdout, |s| s.bold().to_string())
    );
    println!();
    println!("  Waiting for approval... (Ctrl+C to cancel)");

    // Best-effort browser open. If it fails (headless box, ssh session) the
    // user can copy/paste — we already printed the URL.
    let _ = webbrowser::open(&init.verification_uri_complete);

    let interval = Duration::from_secs(init.interval.max(2) as u64);
    loop {
        if Instant::now() >= deadline {
            bail!("Timed out waiting for approval. Run `csshd login` to try again.");
        }
        sleep(interval).await;
        match client.auth_poll(&init.device_code).await {
            Ok(Some(token)) => {
                println!(
                    "  {} CLI session approved.",
                    "✓".if_supports_color(Stdout, |s| s.green().bold().to_string())
                );
                return Ok(token);
            }
            Ok(None) => {} // pending; loop
            Err(e) => return Err(e).map_err(|e| anyhow!("login failed: {e}")),
        }
    }
}
