//! csshd — terminal client for the CSS IT Helpdesk.
//!
//! This is the v0.1 scaffold. Commands print "not yet implemented" until
//! Phase 1 wires up authentication and the API client. See README.md for
//! roadmap.

use anyhow::Result;
use clap::{Parser, Subcommand};

const ABOUT: &str = "Terminal client for the CSS IT Helpdesk.";

#[derive(Parser)]
#[command(name = "csshd", version, about = ABOUT, long_about = None)]
struct Cli {
    /// Helpdesk base URL. On first run, set with `csshd login --helpdesk <url>`;
    /// subsequent runs read it from the local config. The CLI talks only to
    /// this URL — identity is brokered server-side, no IdP-specific identifiers
    /// are baked into this binary.
    #[arg(long, env = "CSSHD_HELPDESK", global = true)]
    helpdesk: Option<String>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Authenticate. On first run, pass `--helpdesk <url>` to bind this CLI
    /// to a helpdesk. The helpdesk shows an approval page in your browser;
    /// once you click Approve, the CLI gets a token and stores it in the
    /// OS keychain.
    Login,
    /// Forget stored credentials.
    Logout,
    /// Show the currently signed-in user.
    Whoami,
    /// List tickets.
    List {
        /// Filter by status (open, in_progress, pending, resolved, closed).
        #[arg(long)]
        status: Option<String>,
        /// Only show tickets assigned to me.
        #[arg(long)]
        mine: bool,
        /// Filter by assignee user ID or email.
        #[arg(long)]
        assignee: Option<String>,
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
    /// Open the interactive TUI (Phase 2).
    Tui,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Login => not_yet("login"),
        Command::Logout => not_yet("logout"),
        Command::Whoami => not_yet("whoami"),
        Command::List { .. } => not_yet("list"),
        Command::View { .. } => not_yet("view"),
        Command::Claim { .. } => not_yet("claim"),
        Command::Close { .. } => not_yet("close"),
        Command::Comment { .. } => not_yet("comment"),
        Command::Tui => not_yet("tui"),
    }
}

fn not_yet(cmd: &str) -> Result<()> {
    eprintln!("csshd: `{cmd}` is not yet implemented (v0.1 scaffold). See https://github.com/css-md/csshd for the roadmap.");
    std::process::exit(2);
}
