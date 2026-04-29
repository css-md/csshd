use anyhow::Result;

use crate::client::Client;

pub async fn run(client: &Client, json: bool) -> Result<()> {
    let me = client.whoami().await?;
    if json {
        println!("{}", serde_json::to_string_pretty(&serde_json::json!({
            "id": me.id,
            "email": me.email,
            "name": me.name,
            "displayName": me.display_name,
            "role": me.role,
            "isActive": me.is_active,
            "team": me.team.as_ref().map(|t| serde_json::json!({"id": t.id, "name": t.name})),
            "oooStart": me.ooo_start,
            "oooEnd": me.ooo_end,
        }))?);
        return Ok(());
    }
    println!("{} <{}>", me.name.as_deref().unwrap_or(me.email.as_str()), me.email);
    println!("  Role: {}", me.role);
    if let Some(team) = &me.team {
        println!("  Team: {}", team.name);
    }
    if let (Some(start), Some(end)) = (me.ooo_start, me.ooo_end) {
        let now = chrono::Utc::now();
        if start <= now && now < end {
            println!(
                "  Out of office until {}",
                end.format("%b %d, %Y %H:%M UTC")
            );
        }
    }
    Ok(())
}
