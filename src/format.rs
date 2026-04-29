//! Terminal output helpers — color-aware, NO_COLOR-respecting, JSON mode.
//!
//! No deps on the rest of the crate so we can unit-test in isolation.

use chrono::{DateTime, Utc};
use comfy_table::{presets::UTF8_FULL_CONDENSED, Cell, ContentArrangement, Table};
use owo_colors::{OwoColorize, Stream::Stdout};

/// Format a UTC timestamp as a relative human string ("just now", "5m ago").
/// Shorter than `chrono-humanize` and without the `_or_in_future` ambiguity.
pub fn relative_time(t: DateTime<Utc>) -> String {
    let now = Utc::now();
    let secs = (now - t).num_seconds();
    if secs < 60 {
        "just now".into()
    } else if secs < 3600 {
        format!("{}m ago", secs / 60)
    } else if secs < 86_400 {
        format!("{}h ago", secs / 3600)
    } else if secs < 86_400 * 14 {
        format!("{}d ago", secs / 86_400)
    } else {
        t.format("%b %d, %Y").to_string()
    }
}

pub fn status_styled(status: &str) -> String {
    match status {
        "OPEN" => status.bold().red().if_supports_color(Stdout, |s| s.to_string()).to_string(),
        "IN_PROGRESS" => "IN PROGRESS"
            .bold()
            .yellow()
            .if_supports_color(Stdout, |s| s.to_string())
            .to_string(),
        "PENDING" => status.cyan().if_supports_color(Stdout, |s| s.to_string()).to_string(),
        "RESOLVED" => status
            .green()
            .if_supports_color(Stdout, |s| s.to_string())
            .to_string(),
        "CLOSED" => status
            .dimmed()
            .if_supports_color(Stdout, |s| s.to_string())
            .to_string(),
        "MERGED" => status
            .dimmed()
            .if_supports_color(Stdout, |s| s.to_string())
            .to_string(),
        s => s.to_string(),
    }
}

pub fn priority_styled(priority: &str) -> String {
    match priority {
        "CRITICAL" => priority
            .bold()
            .on_red()
            .if_supports_color(Stdout, |s| s.to_string())
            .to_string(),
        "HIGH" => priority
            .bold()
            .red()
            .if_supports_color(Stdout, |s| s.to_string())
            .to_string(),
        "NORMAL" => priority
            .if_supports_color(Stdout, |s| s.to_string())
            .to_string(),
        "LOW" => priority
            .dimmed()
            .if_supports_color(Stdout, |s| s.to_string())
            .to_string(),
        p => p.to_string(),
    }
}

/// Build a tickets table — used by `csshd list`.
pub fn ticket_table(rows: &[TicketRow]) -> Table {
    let mut t = Table::new();
    t.load_preset(UTF8_FULL_CONDENSED)
        .set_content_arrangement(ContentArrangement::Dynamic)
        .set_header(vec!["#", "Title", "Status", "Pri", "Assignee", "Updated"]);
    for r in rows {
        t.add_row(vec![
            Cell::new(&r.number),
            Cell::new(&r.title),
            Cell::new(status_styled(&r.status)),
            Cell::new(priority_styled(&r.priority)),
            Cell::new(r.assignee.as_deref().unwrap_or("—")),
            Cell::new(relative_time(r.updated_at)),
        ]);
    }
    t
}

pub struct TicketRow {
    pub number: String,
    pub title: String,
    pub status: String,
    pub priority: String,
    pub assignee: Option<String>,
    pub updated_at: DateTime<Utc>,
}
