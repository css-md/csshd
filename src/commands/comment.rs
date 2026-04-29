use anyhow::{anyhow, bail, Context, Result};
use std::io::Read;
use std::process::Command;

use crate::client::Client;

pub async fn run(
    client: &Client,
    ticket: &str,
    body: Option<String>,
    is_internal: bool,
) -> Result<()> {
    let id = client.resolve_ticket(ticket).await?;

    // Resolve body source:
    //   - Some("-") → stdin
    //   - Some(s)   → literal
    //   - None      → $EDITOR
    let body = match body.as_deref() {
        Some("-") => {
            let mut s = String::new();
            std::io::stdin()
                .read_to_string(&mut s)
                .context("reading stdin")?;
            s
        }
        Some(s) => s.to_string(),
        None => open_editor(is_internal)?,
    };

    let trimmed = body.trim();
    if trimmed.is_empty() {
        bail!("Empty comment — aborting.");
    }

    client.comment(&id, trimmed, is_internal).await?;
    println!(
        "Comment posted on {ticket}{}.",
        if is_internal { " (internal)" } else { "" }
    );
    Ok(())
}

fn open_editor(is_internal: bool) -> Result<String> {
    let editor = std::env::var("VISUAL")
        .or_else(|_| std::env::var("EDITOR"))
        .unwrap_or_else(|_| {
            if cfg!(windows) {
                "notepad".to_string()
            } else {
                "vi".to_string()
            }
        });

    let dir = std::env::temp_dir();
    let file = dir.join(format!(
        "csshd-comment-{}.md",
        chrono::Utc::now().timestamp()
    ));

    let template = format!(
        "# Type your comment for the ticket below. Lines starting with '#' are\n\
         # ignored. Save and exit to post; quit without saving to abort.\n\
         {}\n",
        if is_internal {
            "# This comment is INTERNAL — visible to agents only.\n"
        } else {
            ""
        },
    );
    std::fs::write(&file, template).with_context(|| format!("writing {}", file.display()))?;

    // Best-effort: split the editor command on whitespace so VISUAL='code -w'
    // works. Doesn't handle quoted args.
    let mut parts = editor.split_whitespace();
    let cmd = parts.next().ok_or_else(|| anyhow!("EDITOR is empty"))?;
    let extra: Vec<&str> = parts.collect();

    let status = Command::new(cmd)
        .args(&extra)
        .arg(&file)
        .status()
        .with_context(|| format!("launching editor: {editor}"))?;
    if !status.success() {
        bail!("editor exited non-zero");
    }

    let raw = std::fs::read_to_string(&file)
        .with_context(|| format!("reading {}", file.display()))?;
    // Best-effort cleanup
    let _ = std::fs::remove_file(&file);

    let body: String = raw
        .lines()
        .filter(|l| !l.trim_start().starts_with('#'))
        .collect::<Vec<_>>()
        .join("\n");
    Ok(body)
}
