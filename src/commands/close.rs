use anyhow::Result;

use crate::client::Client;

pub async fn run(client: &Client, ticket: &str) -> Result<()> {
    let id = client.resolve_ticket(ticket).await?;
    client
        .patch_ticket(&id, serde_json::json!({ "status": "CLOSED" }))
        .await?;
    println!("Closed {ticket}.");
    Ok(())
}
