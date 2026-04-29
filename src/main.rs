//! csshd — terminal client for the CSS IT Helpdesk.
//!
//! See README.md for the design overview. Phase 1 covers the plumbing
//! commands below; Phase 2 will add `csshd tui` (an interactive ratatui
//! app) on top of the same client + auth modules.

use anyhow::Result;
use clap::{Parser, Subcommand};
use owo_colors::{OwoColorize, Stream::Stderr};

mod auth;
mod client;
mod commands;
mod config;
mod credentials;
mod format;
mod tui;

const ABOUT: &str = "Terminal client for the CSS IT Helpdesk.";

#[derive(Parser)]
#[command(name = "csshd", version, about = ABOUT, long_about = None)]
struct Cli {
    /// Helpdesk base URL. On first run, set with `csshd login --helpdesk <url>`;
    /// subsequent runs read it from the local config.
    #[arg(long, env = "CSSHD_HELPDESK", global = true)]
    helpdesk: Option<String>,

    /// Output JSON instead of human-friendly text. Pipe-friendly.
    #[arg(long, global = true)]
    json: bool,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Authenticate. Pass --helpdesk on first run; subsequent runs remember.
    Login,
    /// Forget stored credentials.
    Logout,
    /// Show the currently signed-in user.
    Whoami,
    /// List tickets.
    List {
        /// Filter by status (OPEN, IN_PROGRESS, PENDING, RESOLVED, CLOSED). Case-insensitive.
        #[arg(long)]
        status: Option<String>,
        /// Only show tickets assigned to me.
        #[arg(long)]
        mine: bool,
        /// Filter by assignee user id or "me".
        #[arg(long)]
        assignee: Option<String>,
        /// Search query (matches title/description).
        #[arg(long, short = 'q')]
        search: Option<String>,
        /// Page size (default: 50).
        #[arg(long)]
        limit: Option<u32>,
    },
    /// Show a single ticket in detail.
    View {
        /// Ticket number, e.g. CSS-04234 (or just 4234).
        ticket: String,
    },
    /// Claim a ticket (assign to yourself, set status to IN_PROGRESS).
    Claim {
        ticket: String,
    },
    /// Close a ticket.
    Close {
        ticket: String,
    },
    /// Add a comment to a ticket.
    Comment {
        ticket: String,
        /// Comment body. Pass "-" to read from stdin, or omit to open $EDITOR.
        body: Option<String>,
        /// Mark as an internal note (agents only).
        #[arg(long)]
        internal: bool,
    },
    /// Open the interactive TUI (Phase 2 — not yet implemented).
    Tui,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    if let Err(e) = dispatch(cli).await {
        eprintln!(
            "{} {e}",
            "error:"
                .if_supports_color(Stderr, |s| s.bold().red().to_string())
        );
        // Print the chain in dimmed text so users see *why*.
        let mut src = e.source();
        while let Some(s) = src {
            eprintln!("  {} {s}", "↳".if_supports_color(Stderr, |s| s.dimmed().to_string()));
            src = s.source();
        }
        std::process::exit(1);
    }
    Ok(())
}

async fn dispatch(cli: Cli) -> Result<()> {
    // login is special: it writes the helpdesk URL to config, doesn't need
    // credentials yet, and must NOT pre-resolve a token.
    if let Command::Login = cli.command {
        return commands::login::run(cli.helpdesk).await;
    }
    if let Command::Logout = cli.command {
        return commands::logout::run().await;
    }

    // Everything else needs a configured helpdesk + a stored token.
    let cfg = config::load().unwrap_or_default();
    let helpdesk = config::resolve_helpdesk(cli.helpdesk.clone(), &cfg)?;
    let token = credentials::load_token(&helpdesk)?
        .ok_or_else(|| anyhow::anyhow!("Not signed in. Run `csshd login`."))?;
    let client = client::Client::new(&helpdesk, Some(token))?;

    match cli.command {
        Command::Login | Command::Logout => unreachable!(),
        Command::Whoami => commands::whoami::run(&client, cli.json).await,
        Command::List {
            status,
            mine,
            assignee,
            search,
            limit,
        } => {
            commands::list::run(
                &client,
                commands::list::ListOpts {
                    status,
                    mine,
                    assignee,
                    search,
                    page_size: limit,
                    json: cli.json,
                },
            )
            .await
        }
        Command::View { ticket } => commands::view::run(&client, &ticket, cli.json).await,
        Command::Claim { ticket } => commands::claim::run(&client, &ticket).await,
        Command::Close { ticket } => commands::close::run(&client, &ticket).await,
        Command::Comment {
            ticket,
            body,
            internal,
        } => commands::comment::run(&client, &ticket, body, internal).await,
        Command::Tui => tui::run(client).await,
    }
}
