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
        "OPEN" => status
            .bold()
            .red()
            .if_supports_color(Stdout, |s| s.to_string())
            .to_string(),
        "IN_PROGRESS" => "IN PROGRESS"
            .bold()
            .yellow()
            .if_supports_color(Stdout, |s| s.to_string())
            .to_string(),
        "PENDING" => status
            .cyan()
            .if_supports_color(Stdout, |s| s.to_string())
            .to_string(),
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

/// Convert the helpdesk's stored HTML bodies to plain text for the terminal.
///
/// Ticket descriptions and comments come back as HTML (historically prefixed
/// with an `<!--html-->` marker). This is deliberately not a real parser — it
/// just needs to be readable in a terminal — but it does have to turn block
/// elements into line breaks. Without that, `<p>one</p><p>two</p>` renders as
/// "onetwo" and every web-composed reply reads as one run-on paragraph.
pub fn html_to_text(input: &str) -> String {
    let body = input.strip_prefix("<!--html-->").unwrap_or(input);

    // Tags that should produce a line break where they appear.
    const BREAK_BEFORE: [&str; 8] = ["p", "div", "br", "li", "tr", "h1", "h2", "h3"];

    let mut out = String::with_capacity(body.len());
    let mut chars = body.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch != '<' {
            out.push(ch);
            continue;
        }
        // Consume up to the matching '>', capturing the tag name.
        let mut tag = String::new();
        for c in chars.by_ref() {
            if c == '>' {
                break;
            }
            tag.push(c);
        }
        let name: String = tag
            .trim_start_matches('/')
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric())
            .collect::<String>()
            .to_ascii_lowercase();
        if BREAK_BEFORE.contains(&name.as_str()) && !out.ends_with('\n') {
            out.push('\n');
        }
    }

    let decoded = out
        .replace("&nbsp;", " ")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        // &amp; last, so "&amp;lt;" doesn't become "<".
        .replace("&amp;", "&");

    // Collapse the runs of blank lines that <p></p> pairs leave behind.
    let mut lines: Vec<&str> = Vec::new();
    for line in decoded.lines() {
        let trimmed = line.trim_end();
        if trimmed.trim().is_empty() && lines.last().map(|l: &&str| l.is_empty()).unwrap_or(true) {
            continue;
        }
        lines.push(if trimmed.trim().is_empty() {
            ""
        } else {
            trimmed
        });
    }
    while lines.last().map(|l| l.is_empty()).unwrap_or(false) {
        lines.pop();
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paragraphs_become_separate_lines() {
        assert_eq!(html_to_text("<p>one</p><p>two</p>"), "one\ntwo");
    }

    #[test]
    fn strips_the_html_marker() {
        assert_eq!(html_to_text("<!--html--><p>hi</p>"), "hi");
    }

    #[test]
    fn breaks_and_list_items() {
        assert_eq!(html_to_text("a<br>b<br/>c"), "a\nb\nc");
        assert_eq!(html_to_text("<ul><li>x</li><li>y</li></ul>"), "x\ny");
    }

    #[test]
    fn inline_tags_do_not_break() {
        assert_eq!(
            html_to_text("<p>hello <b>bold</b> and <i>italic</i></p>"),
            "hello bold and italic"
        );
    }

    #[test]
    fn decodes_entities_without_double_decoding() {
        assert_eq!(html_to_text("a &amp; b"), "a & b");
        assert_eq!(html_to_text("&amp;lt;"), "&lt;");
        assert_eq!(
            html_to_text("&lt;tag&gt; &quot;q&quot; &#39;s&#39;"),
            "<tag> \"q\" 's'"
        );
    }

    #[test]
    fn plain_text_passes_through() {
        assert_eq!(html_to_text("just text"), "just text");
    }

    #[test]
    fn collapses_blank_runs_and_trims() {
        assert_eq!(
            html_to_text("<p>a</p><p></p><p></p><p>b</p><p></p>"),
            "a\nb"
        );
    }
}
