use anyhow::Result;
use owo_colors::{OwoColorize, Stream::Stdout};

use crate::{client::Client, format};

pub async fn run(client: &Client, ticket: &str, json: bool) -> Result<()> {
    let id = client.resolve_ticket(ticket).await?;
    let t = client.get_ticket(&id).await?;

    if json {
        println!("{}", serde_json::to_string_pretty(&serde_json::json!({
            "id": t.id,
            "ticketNumber": t.ticket_number,
            "title": t.title,
            "description": t.description,
            "status": t.status,
            "priority": t.priority,
            "createdAt": t.created_at,
            "updatedAt": t.updated_at,
            "requester": serde_json::json!({"name": t.requester.name, "email": t.requester.email}),
            "assignee": t.assigned_agent.as_ref().and_then(|a| a.name.clone()),
            "site": t.site.as_ref().map(|s| s.name.clone()),
            "comments": t.comments.iter().map(|c| serde_json::json!({
                "id": c.id,
                "body": c.body,
                "isInternal": c.is_internal,
                "createdAt": c.created_at,
                "author": c.author.name,
            })).collect::<Vec<_>>(),
        }))?);
        return Ok(());
    }

    let header = format!(
        "{}  {}",
        t.ticket_number
            .if_supports_color(Stdout, |s| s.bold().to_string()),
        t.title
    );
    println!();
    println!("{header}");
    println!(
        "  {} · {} · opened by {} · {}",
        format::status_styled(&t.status),
        format::priority_styled(&t.priority),
        t.requester.name.as_deref().unwrap_or(
            t.requester.email.as_deref().unwrap_or("?"),
        ),
        format::relative_time(t.created_at),
    );
    if let Some(agent) = &t.assigned_agent {
        println!(
            "  Assigned to {}",
            agent
                .name
                .as_deref()
                .or(agent.email.as_deref())
                .unwrap_or("?"),
        );
    }
    if let Some(site) = &t.site {
        println!("  Site: {}", site.name);
    }
    println!();

    // Description — strip the HTML marker, render as plaintext (markdown-ish
    // viewer is Phase 2's TUI job).
    let body = t
        .description
        .strip_prefix("<!--html-->")
        .map(|s| strip_html(s))
        .unwrap_or_else(|| t.description.clone());
    let trimmed = body.trim();
    if !trimmed.is_empty() {
        for line in trimmed.lines() {
            println!("  {line}");
        }
        println!();
    }

    if t.comments.is_empty() {
        println!("  (no replies)");
    } else {
        println!(
            "  {} {}",
            "─".repeat(8),
            format!("{} {}", t.comments.len(), if t.comments.len() == 1 { "reply" } else { "replies" })
                .if_supports_color(Stdout, |s| s.dimmed().to_string()),
        );
        println!();
        for c in &t.comments {
            let author = c
                .author
                .name
                .as_deref()
                .or(c.author.email.as_deref())
                .unwrap_or("?");
            let when = format::relative_time(c.created_at);
            let internal = if c.is_internal { " [internal]" } else { "" };
            println!(
                "  {} · {}{}",
                author.if_supports_color(Stdout, |s| s.bold().to_string()),
                when.if_supports_color(Stdout, |s| s.dimmed().to_string()),
                internal.if_supports_color(Stdout, |s| s.yellow().to_string()),
            );
            let cb = c
                .body
                .strip_prefix("<!--html-->")
                .map(|s| strip_html(s))
                .unwrap_or_else(|| c.body.clone());
            for line in cb.trim().lines() {
                println!("    {line}");
            }
            println!();
        }
    }

    Ok(())
}

/// Bare-bones HTML→text. Good enough for replies in a CLI; not bulletproof.
/// Phase 2 TUI can use a real renderer (pulldown-cmark or similar) for
/// markdown-ish output.
fn strip_html(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut in_tag = false;
    for ch in html.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            c if !in_tag => out.push(c),
            _ => {}
        }
    }
    out.replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
}
