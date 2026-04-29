use anyhow::Result;

use crate::{
    client::{Client, TicketQuery},
    format,
};

pub struct ListOpts {
    pub status: Option<String>,
    pub mine: bool,
    pub assignee: Option<String>,
    pub search: Option<String>,
    pub page_size: Option<u32>,
    pub json: bool,
}

pub async fn run(client: &Client, opts: ListOpts) -> Result<()> {
    let assignee = if opts.mine {
        Some("me".to_string())
    } else {
        opts.assignee
    };

    // Default to OPEN+IN_PROGRESS if no status filter — matches the daily
    // working set, not the entire archive.
    let status = opts.status.map(|s| s.to_uppercase());

    let q = TicketQuery {
        status,
        assignee,
        search: opts.search,
        page: Some(1),
        page_size: opts.page_size.or(Some(50)),
    };
    let page = client.list_tickets(q).await?;

    if opts.json {
        println!("{}", serde_json::to_string_pretty(&serde_json::json!({
            "total": page.total,
            "page": page.page,
            "pageSize": page.page_size,
            "tickets": page.tickets.iter().map(|t| serde_json::json!({
                "id": t.id,
                "ticketNumber": t.ticket_number,
                "title": t.title,
                "status": t.status,
                "priority": t.priority,
                "assignee": t.assigned_agent.as_ref().and_then(|a| a.name.clone()),
                "site": t.site.as_ref().map(|s| s.name.clone()),
                "createdAt": t.created_at,
                "updatedAt": t.updated_at,
            })).collect::<Vec<_>>(),
        }))?);
        return Ok(());
    }

    if page.tickets.is_empty() {
        println!("No tickets match.");
        return Ok(());
    }

    let rows: Vec<format::TicketRow> = page
        .tickets
        .iter()
        .map(|t| format::TicketRow {
            number: t.ticket_number.clone(),
            title: t.title.clone(),
            status: t.status.clone(),
            priority: t.priority.clone(),
            assignee: t
                .assigned_agent
                .as_ref()
                .and_then(|a| a.name.clone().or_else(|| a.email.clone())),
            updated_at: t.updated_at,
        })
        .collect();

    println!("{}", format::ticket_table(&rows));
    if page.total > page.tickets.len() as u32 {
        println!(
            "Showing {} of {} matching tickets — narrow with --status, --mine, or --search.",
            page.tickets.len(),
            page.total
        );
    }
    Ok(())
}
