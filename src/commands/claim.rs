use anyhow::Result;

use crate::client::Client;

/// Claim assigns the ticket to the current user and bumps to IN_PROGRESS.
/// The helpdesk's API does this atomically when both fields are sent.
pub async fn run(client: &Client, ticket: &str) -> Result<()> {
    let id = client.resolve_ticket(ticket).await?;
    let me = client.whoami().await?;
    client
        .patch_ticket(
            &id,
            serde_json::json!({
                "assignedAgentId": me.id,
                "status": "IN_PROGRESS",
            }),
        )
        .await?;
    println!("Claimed {ticket} → assigned to you, status IN_PROGRESS.");
    Ok(())
}
