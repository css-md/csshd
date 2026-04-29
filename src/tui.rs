//! Phase 2 — interactive terminal UI.
//!
//! Two-pane layout: ticket list on the left, detail on the right. Vim-style
//! keys (j/k navigate, Enter open, r reply, c claim, x close, /search, q quit).
//! Polls the API every 30 seconds for fresh data.
//!
//! Architecture is unapologetically simple: one App struct, a poll-redraw loop,
//! a keymap. Async work happens via tokio::spawn with results funneled back as
//! AppEvent variants over an mpsc channel.

use std::io;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use crossterm::{
    event::{
        self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyEventKind,
        KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
    },
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap},
    Frame, Terminal,
};
use tokio::sync::mpsc;

use crate::client::{Client, PartyRef, Ticket, TicketQuery, TicketSummary};

const REFRESH_INTERVAL: Duration = Duration::from_secs(30);

pub async fn run(client: Client) -> Result<()> {
    let mut terminal = setup_terminal()?;
    let result = run_app(&mut terminal, client).await;
    restore_terminal(&mut terminal)?;
    result
}

fn setup_terminal() -> Result<Terminal<CrosstermBackend<io::Stdout>>> {
    enable_raw_mode().context("enable_raw_mode")?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture).context("enter alt screen")?;
    Terminal::new(CrosstermBackend::new(stdout)).context("terminal init")
}

fn restore_terminal(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> Result<()> {
    disable_raw_mode().context("disable_raw_mode")?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen, DisableMouseCapture)?;
    terminal.show_cursor()?;
    Ok(())
}

// ── App state ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pane {
    List,
    Detail,
    Search,
    Help,
}

#[derive(Debug)]
enum AppEvent {
    Tick,
    Key(KeyEvent),
    Mouse(MouseEvent),
    TicketsLoaded(Vec<TicketSummary>),
    TicketLoaded(Box<Ticket>),
    Status(String),
    Error(String),
}

struct App {
    client: Arc<Client>,
    tx: mpsc::UnboundedSender<AppEvent>,
    pane: Pane,
    tickets: Vec<TicketSummary>,
    list_state: ListState,
    search: String,
    status_filter: Option<String>,
    detail: Option<Ticket>,
    detail_scroll: u16,
    status_msg: Option<(String, Instant)>,
    last_refresh: Option<Instant>,
    quit: bool,
    /// Last-rendered geometry — used to translate mouse coords into
    /// list rows / detail clicks. Updated each draw().
    list_area: Option<Rect>,
    detail_area: Option<Rect>,
    /// First-visible-ticket-index in the list pane (set during draw()).
    list_offset: usize,
}

impl App {
    fn new(client: Arc<Client>, tx: mpsc::UnboundedSender<AppEvent>) -> Self {
        let mut s = Self {
            client,
            tx,
            pane: Pane::List,
            tickets: vec![],
            list_state: ListState::default(),
            search: String::new(),
            status_filter: None,
            detail: None,
            detail_scroll: 0,
            status_msg: None,
            last_refresh: None,
            quit: false,
            list_area: None,
            detail_area: None,
            list_offset: 0,
        };
        s.list_state.select(Some(0));
        s
    }

    fn flash(&mut self, msg: impl Into<String>) {
        self.status_msg = Some((msg.into(), Instant::now()));
    }

    fn flash_visible(&self) -> Option<&str> {
        self.status_msg
            .as_ref()
            .filter(|(_, t)| t.elapsed() < Duration::from_secs(4))
            .map(|(s, _)| s.as_str())
    }

    fn selected_ticket(&self) -> Option<&TicketSummary> {
        self.list_state.selected().and_then(|i| self.tickets.get(i))
    }

    fn move_selection(&mut self, delta: i32) {
        if self.tickets.is_empty() {
            return;
        }
        let len = self.tickets.len() as i32;
        let cur = self.list_state.selected().unwrap_or(0) as i32;
        let next = ((cur + delta).rem_euclid(len)) as usize;
        self.list_state.select(Some(next));
    }
}

async fn run_app(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    client: Client,
) -> Result<()> {
    let client = Arc::new(client);
    let (tx, mut rx) = mpsc::unbounded_channel::<AppEvent>();

    let mut app = App::new(client.clone(), tx.clone());

    // Initial load + periodic refresh.
    spawn_refresh(&app);
    spawn_ticker(tx.clone());
    spawn_keys(tx.clone());

    while !app.quit {
        terminal.draw(|f| draw(f, &mut app))?;

        // We block here; tokio's mpsc + spawned tasks make this a real event loop.
        let Some(ev) = rx.recv().await else { break };
        handle(&mut app, ev);
    }

    Ok(())
}

// ── Spawned tasks ───────────────────────────────────────────────────────

fn spawn_ticker(tx: mpsc::UnboundedSender<AppEvent>) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_millis(250));
        loop {
            interval.tick().await;
            if tx.send(AppEvent::Tick).is_err() {
                break;
            }
        }
    });
}

fn spawn_keys(tx: mpsc::UnboundedSender<AppEvent>) {
    tokio::task::spawn_blocking(move || loop {
        // Block for up to 200ms waiting for input. Timeout returns Ok(false)
        // and we loop again — this lets us exit promptly when the channel closes.
        match event::poll(Duration::from_millis(200)) {
            Ok(true) => match event::read() {
                Ok(Event::Key(k)) if k.kind == KeyEventKind::Press => {
                    if tx.send(AppEvent::Key(k)).is_err() {
                        break;
                    }
                }
                Ok(Event::Mouse(m)) => {
                    if tx.send(AppEvent::Mouse(m)).is_err() {
                        break;
                    }
                }
                Ok(_) => {}
                Err(_) => break,
            },
            Ok(false) => {
                // Heartbeat — if the channel's gone, bail.
                if tx.send(AppEvent::Tick).is_err() {
                    break;
                }
            }
            Err(_) => break,
        }
    });
}

fn spawn_refresh(app: &App) {
    let client = app.client.clone();
    let tx = app.tx.clone();
    let q = TicketQuery {
        status: app.status_filter.clone(),
        search: if app.search.is_empty() { None } else { Some(app.search.clone()) },
        page: Some(1),
        page_size: Some(100),
        ..Default::default()
    };
    tokio::spawn(async move {
        match client.list_tickets(q).await {
            Ok(page) => {
                let _ = tx.send(AppEvent::TicketsLoaded(page.tickets));
            }
            Err(e) => {
                let _ = tx.send(AppEvent::Error(format!("list failed: {e}")));
            }
        }
    });
}

fn spawn_load_detail(app: &App, id: String) {
    let client = app.client.clone();
    let tx = app.tx.clone();
    tokio::spawn(async move {
        match client.get_ticket(&id).await {
            Ok(t) => {
                let _ = tx.send(AppEvent::TicketLoaded(Box::new(t)));
            }
            Err(e) => {
                let _ = tx.send(AppEvent::Error(format!("load failed: {e}")));
            }
        }
    });
}

fn spawn_patch(app: &App, id: String, patch: serde_json::Value, success: String) {
    let client = app.client.clone();
    let tx = app.tx.clone();
    tokio::spawn(async move {
        match client.patch_ticket(&id, patch).await {
            Ok(_) => {
                let _ = tx.send(AppEvent::Status(success));
            }
            Err(e) => {
                let _ = tx.send(AppEvent::Error(format!("patch failed: {e}")));
            }
        }
    });
}

// ── Event handling ──────────────────────────────────────────────────────

fn handle(app: &mut App, ev: AppEvent) {
    match ev {
        AppEvent::Tick => {
            if app
                .last_refresh
                .map(|t| t.elapsed() >= REFRESH_INTERVAL)
                .unwrap_or(true)
            {
                app.last_refresh = Some(Instant::now());
                spawn_refresh(app);
            }
        }
        AppEvent::Key(k) => handle_key(app, k),
        AppEvent::Mouse(m) => handle_mouse(app, m),
        AppEvent::TicketsLoaded(ts) => {
            app.tickets = ts;
            // Keep the cursor stable but in-range.
            let new_len = app.tickets.len();
            let cur = app.list_state.selected().unwrap_or(0).min(new_len.saturating_sub(1));
            app.list_state.select(if new_len == 0 { None } else { Some(cur) });
        }
        AppEvent::TicketLoaded(t) => {
            app.detail = Some(*t);
            app.detail_scroll = 0;
        }
        AppEvent::Status(s) => app.flash(s),
        AppEvent::Error(s) => app.flash(format!("error: {s}")),
    }
}

fn handle_key(app: &mut App, k: KeyEvent) {
    // Ctrl-C is a global quit even from within Search/Help.
    if k.modifiers.contains(KeyModifiers::CONTROL) && matches!(k.code, KeyCode::Char('c')) {
        app.quit = true;
        return;
    }

    match app.pane {
        Pane::Search => handle_key_search(app, k),
        Pane::Help => {
            if matches!(k.code, KeyCode::Esc | KeyCode::Char('?') | KeyCode::Char('q')) {
                app.pane = Pane::List;
            }
        }
        Pane::List => handle_key_list(app, k),
        Pane::Detail => handle_key_detail(app, k),
    }
}

fn handle_key_list(app: &mut App, k: KeyEvent) {
    match k.code {
        KeyCode::Char('q') => app.quit = true,
        KeyCode::Char('?') => app.pane = Pane::Help,
        KeyCode::Char('j') | KeyCode::Down => app.move_selection(1),
        KeyCode::Char('k') | KeyCode::Up => app.move_selection(-1),
        KeyCode::PageDown => app.move_selection(10),
        KeyCode::PageUp => app.move_selection(-10),
        KeyCode::Char('g') => app.list_state.select(Some(0)),
        KeyCode::Char('G') => {
            if !app.tickets.is_empty() {
                app.list_state.select(Some(app.tickets.len() - 1));
            }
        }
        KeyCode::Enter => {
            if let Some(t) = app.selected_ticket() {
                let id = t.id.clone();
                app.detail = None;
                app.pane = Pane::Detail;
                spawn_load_detail(app, id);
            }
        }
        KeyCode::Char('/') => {
            app.pane = Pane::Search;
        }
        KeyCode::Char('r') => {
            if let Some(t) = app.selected_ticket() {
                app.flash(format!("Reply on {} — open detail (Enter) first.", t.ticket_number));
            }
        }
        KeyCode::Char('c') => {
            // Claim from list.
            if let Some(t) = app.selected_ticket() {
                let id = t.id.clone();
                let number = t.ticket_number.clone();
                let client = app.client.clone();
                let tx = app.tx.clone();
                tokio::spawn(async move {
                    let me = match client.whoami().await {
                        Ok(m) => m,
                        Err(e) => {
                            let _ = tx.send(AppEvent::Error(format!("claim failed: {e}")));
                            return;
                        }
                    };
                    match client
                        .patch_ticket(
                            &id,
                            serde_json::json!({
                                "assignedAgentId": me.id,
                                "status": "IN_PROGRESS",
                            }),
                        )
                        .await
                    {
                        Ok(_) => {
                            let _ = tx.send(AppEvent::Status(format!("Claimed {number}.")));
                        }
                        Err(e) => {
                            let _ = tx.send(AppEvent::Error(format!("claim failed: {e}")));
                        }
                    }
                });
            }
        }
        KeyCode::Char('x') => {
            if let Some(t) = app.selected_ticket() {
                let id = t.id.clone();
                let number = t.ticket_number.clone();
                spawn_patch(
                    app,
                    id,
                    serde_json::json!({ "status": "CLOSED" }),
                    format!("Closed {number}."),
                );
            }
        }
        KeyCode::Char('R') | KeyCode::F(5) => {
            app.last_refresh = None; // forces immediate refresh on next tick
            app.flash("Refreshing…");
        }
        _ => {}
    }
}

fn handle_key_detail(app: &mut App, k: KeyEvent) {
    match k.code {
        KeyCode::Char('q') | KeyCode::Esc => {
            app.pane = Pane::List;
            app.detail = None;
        }
        KeyCode::Char('j') | KeyCode::Down => app.detail_scroll = app.detail_scroll.saturating_add(1),
        KeyCode::Char('k') | KeyCode::Up => app.detail_scroll = app.detail_scroll.saturating_sub(1),
        KeyCode::PageDown => app.detail_scroll = app.detail_scroll.saturating_add(10),
        KeyCode::PageUp => app.detail_scroll = app.detail_scroll.saturating_sub(10),
        KeyCode::Char('?') => app.pane = Pane::Help,
        KeyCode::Char('c') => {
            if let Some(t) = app.detail.as_ref() {
                let id = t.id.clone();
                let number = t.ticket_number.clone();
                let client = app.client.clone();
                let tx = app.tx.clone();
                tokio::spawn(async move {
                    let me = match client.whoami().await {
                        Ok(m) => m,
                        Err(e) => {
                            let _ = tx.send(AppEvent::Error(format!("claim failed: {e}")));
                            return;
                        }
                    };
                    match client
                        .patch_ticket(
                            &id,
                            serde_json::json!({
                                "assignedAgentId": me.id,
                                "status": "IN_PROGRESS",
                            }),
                        )
                        .await
                    {
                        Ok(_) => {
                            let _ = tx.send(AppEvent::Status(format!("Claimed {number}.")));
                            let _ = tx.send(AppEvent::Tick); // trigger refresh next tick
                        }
                        Err(e) => {
                            let _ = tx.send(AppEvent::Error(format!("claim failed: {e}")));
                        }
                    }
                });
            }
        }
        KeyCode::Char('x') => {
            if let Some(t) = app.detail.as_ref() {
                let id = t.id.clone();
                let number = t.ticket_number.clone();
                spawn_patch(
                    app,
                    id,
                    serde_json::json!({ "status": "CLOSED" }),
                    format!("Closed {number}."),
                );
                // Bounce back to list.
                app.pane = Pane::List;
                app.detail = None;
            }
        }
        KeyCode::Char('r') => {
            // Drop out of the alt-screen, run $EDITOR for the comment, restore.
            // Quick-and-dirty: we have to take the terminal back to cooked mode
            // first or vim will misbehave. Done in an async block so the event
            // loop's redraw doesn't fight us.
            if let Some(t) = app.detail.as_ref() {
                let id = t.id.clone();
                let number = t.ticket_number.clone();
                let client = app.client.clone();
                let tx = app.tx.clone();
                tokio::spawn(async move {
                    match crate::commands::comment::run(&client, &id, None, false).await {
                        Ok(()) => {
                            let _ = tx.send(AppEvent::Status(format!("Comment posted on {number}.")));
                        }
                        Err(e) => {
                            let _ = tx.send(AppEvent::Error(format!("comment failed: {e}")));
                        }
                    }
                });
            }
        }
        _ => {}
    }
}

fn handle_key_search(app: &mut App, k: KeyEvent) {
    match k.code {
        KeyCode::Esc => {
            app.search.clear();
            app.pane = Pane::List;
            app.last_refresh = None;
        }
        KeyCode::Enter => {
            app.pane = Pane::List;
            app.last_refresh = None;
        }
        KeyCode::Backspace => {
            app.search.pop();
        }
        KeyCode::Char(c) => {
            app.search.push(c);
        }
        _ => {}
    }
}

fn handle_mouse(app: &mut App, m: MouseEvent) {
    match m.kind {
        MouseEventKind::Down(MouseButton::Left) => {
            // Click on a list row → select. Inside the detail pane → focus
            // detail (same effect as pressing Enter on the current row when
            // we're still in list mode and have a detail loaded).
            if let Some(idx) = list_row_at(app, m.column, m.row) {
                app.list_state.select(Some(idx));
                // Lazy-load detail so the right pane updates immediately.
                if let Some(t) = app.tickets.get(idx) {
                    let id = t.id.clone();
                    app.detail = None;
                    spawn_load_detail(app, id);
                }
            } else if app
                .detail_area
                .map(|r| contains(r, m.column, m.row))
                .unwrap_or(false)
            {
                app.pane = Pane::Detail;
            }
        }
        MouseEventKind::ScrollDown => {
            if app
                .detail_area
                .map(|r| contains(r, m.column, m.row))
                .unwrap_or(false)
            {
                app.detail_scroll = app.detail_scroll.saturating_add(3);
            } else {
                app.move_selection(3);
            }
        }
        MouseEventKind::ScrollUp => {
            if app
                .detail_area
                .map(|r| contains(r, m.column, m.row))
                .unwrap_or(false)
            {
                app.detail_scroll = app.detail_scroll.saturating_sub(3);
            } else {
                app.move_selection(-3);
            }
        }
        _ => {}
    }
}

fn contains(r: Rect, x: u16, y: u16) -> bool {
    x >= r.x && x < r.x.saturating_add(r.width) && y >= r.y && y < r.y.saturating_add(r.height)
}

fn list_row_at(app: &App, col: u16, row: u16) -> Option<usize> {
    let area = app.list_area?;
    if !contains(area, col, row) {
        return None;
    }
    // List has a 1-cell border on top + bottom. The first row of items is
    // at area.y + 1.
    let inner_top = area.y.checked_add(1)?;
    let inner_bottom = area.y.saturating_add(area.height).saturating_sub(1);
    if row < inner_top || row >= inner_bottom {
        return None;
    }
    let visible_idx = (row - inner_top) as usize;
    let absolute = app.list_offset.checked_add(visible_idx)?;
    if absolute < app.tickets.len() {
        Some(absolute)
    } else {
        None
    }
}

// ── Rendering ────────────────────────────────────────────────────────────

fn draw(f: &mut Frame, app: &mut App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(0),     // body
            Constraint::Length(1),  // status / help line
        ])
        .split(f.area());

    let body = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
        .split(chunks[0]);

    app.list_area = Some(body[0]);
    app.detail_area = Some(body[1]);

    draw_list(f, body[0], app);
    draw_detail(f, body[1], app);
    draw_status_bar(f, chunks[1], app);

    app.list_offset = app.list_state.offset();

    if app.pane == Pane::Help {
        draw_help_overlay(f, app);
    }
}

fn draw_list(f: &mut Frame, area: Rect, app: &mut App) {
    let title = if app.search.is_empty() {
        "Tickets".to_string()
    } else {
        format!("Tickets — search: {}", app.search)
    };

    let items: Vec<ListItem> = app
        .tickets
        .iter()
        .map(|t| {
            let priority_style = match t.priority.as_str() {
                "CRITICAL" => Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                "HIGH" => Style::default().fg(Color::Red),
                "LOW" => Style::default().fg(Color::DarkGray),
                _ => Style::default(),
            };
            let status_style = match t.status.as_str() {
                "OPEN" => Style::default().fg(Color::Red),
                "IN_PROGRESS" => Style::default().fg(Color::Yellow),
                "PENDING" => Style::default().fg(Color::Cyan),
                "RESOLVED" => Style::default().fg(Color::Green),
                _ => Style::default().fg(Color::DarkGray),
            };
            let line = Line::from(vec![
                Span::styled(format!("{:<10} ", t.ticket_number), Style::default().fg(Color::DarkGray)),
                Span::styled(short_status(&t.status), status_style),
                Span::raw(" "),
                Span::styled(short_priority(&t.priority), priority_style),
                Span::raw(" "),
                Span::raw(t.title.clone()),
            ]);
            ListItem::new(line)
        })
        .collect();

    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title(title))
        .highlight_style(Style::default().bg(Color::Blue).fg(Color::White).add_modifier(Modifier::BOLD))
        .highlight_symbol("▶ ");

    f.render_stateful_widget(list, area, &mut app.list_state);
}

fn short_status(s: &str) -> String {
    match s {
        "OPEN" => "OPEN".to_string(),
        "IN_PROGRESS" => "WORK".to_string(),
        "PENDING" => "PEND".to_string(),
        "RESOLVED" => "DONE".to_string(),
        "CLOSED" => "CLSD".to_string(),
        s => s.chars().take(4).collect(),
    }
}

fn short_priority(p: &str) -> &'static str {
    match p {
        "CRITICAL" => "CRIT",
        "HIGH" => "HIGH",
        "NORMAL" => "NORM",
        "LOW" => "LOW ",
        _ => "????",
    }
}

fn party_label(p: &PartyRef) -> String {
    p.name
        .clone()
        .or_else(|| p.email.clone())
        .unwrap_or_else(|| "?".to_string())
}

fn draw_detail(f: &mut Frame, area: Rect, app: &mut App) {
    if app.pane != Pane::Detail && app.detail.is_none() {
        let preview = app
            .selected_ticket()
            .map(|t| {
                Paragraph::new(vec![
                    Line::from(Span::styled(
                        t.ticket_number.clone(),
                        Style::default().add_modifier(Modifier::BOLD),
                    )),
                    Line::from(t.title.clone()),
                    Line::raw(""),
                    Line::from(format!(
                        "{} · {} · opened by {}",
                        t.status,
                        t.priority,
                        party_label(&t.requester)
                    )),
                    Line::raw(""),
                    Line::from(Span::styled(
                        "Press Enter to open detail.",
                        Style::default().fg(Color::DarkGray),
                    )),
                ])
            })
            .unwrap_or_else(|| Paragraph::new("No tickets."));
        f.render_widget(preview.block(Block::default().borders(Borders::ALL).title("Detail")).wrap(Wrap { trim: false }), area);
        return;
    }

    let Some(t) = app.detail.as_ref() else {
        let p = Paragraph::new(Span::styled("Loading…", Style::default().fg(Color::DarkGray)))
            .block(Block::default().borders(Borders::ALL).title("Detail"));
        f.render_widget(p, area);
        return;
    };

    let mut lines: Vec<Line> = vec![
        Line::from(vec![
            Span::styled(t.ticket_number.clone(), Style::default().add_modifier(Modifier::BOLD)),
            Span::raw("  "),
            Span::raw(t.title.clone()),
        ]),
        Line::from(vec![
            Span::styled(format!("[{}]", t.status), status_color(&t.status)),
            Span::raw(" "),
            Span::styled(format!("[{}]", t.priority), priority_color(&t.priority)),
            Span::raw(format!("  opened by {}", party_label(&t.requester))),
        ]),
    ];
    if let Some(a) = &t.assigned_agent {
        lines.push(Line::from(format!("  assigned to {}", party_label(a))));
    }
    if let Some(s) = &t.site {
        lines.push(Line::from(format!("  site {}", s.name)));
    }
    lines.push(Line::raw(""));

    let body = strip_html(&t.description);
    for l in body.lines() {
        lines.push(Line::raw(l.to_string()));
    }
    lines.push(Line::raw(""));
    lines.push(Line::from(Span::styled(
        format!(
            "── {} {} ──",
            t.comments.len(),
            if t.comments.len() == 1 { "reply" } else { "replies" }
        ),
        Style::default().fg(Color::DarkGray),
    )));
    lines.push(Line::raw(""));

    for c in &t.comments {
        let internal = if c.is_internal { " [internal]" } else { "" };
        lines.push(Line::from(vec![
            Span::styled(party_label(&c.author), Style::default().add_modifier(Modifier::BOLD)),
            Span::styled(internal.to_string(), Style::default().fg(Color::Yellow)),
            Span::styled(format!("  {}", c.created_at.format("%Y-%m-%d %H:%M")), Style::default().fg(Color::DarkGray)),
        ]));
        for line in strip_html(&c.body).lines() {
            lines.push(Line::raw(format!("  {line}")));
        }
        lines.push(Line::raw(""));
    }

    let p = Paragraph::new(lines)
        .block(Block::default().borders(Borders::ALL).title(format!("Detail — {}", t.ticket_number)))
        .wrap(Wrap { trim: false })
        .scroll((app.detail_scroll, 0));
    f.render_widget(p, area);
}

fn status_color(s: &str) -> Style {
    match s {
        "OPEN" => Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        "IN_PROGRESS" => Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
        "PENDING" => Style::default().fg(Color::Cyan),
        "RESOLVED" => Style::default().fg(Color::Green),
        _ => Style::default().fg(Color::DarkGray),
    }
}

fn priority_color(p: &str) -> Style {
    match p {
        "CRITICAL" => Style::default().fg(Color::White).bg(Color::Red).add_modifier(Modifier::BOLD),
        "HIGH" => Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        "LOW" => Style::default().fg(Color::DarkGray),
        _ => Style::default(),
    }
}

fn draw_status_bar(f: &mut Frame, area: Rect, app: &App) {
    let text: String = match (app.flash_visible(), app.pane) {
        (Some(s), _) => s.to_string(),
        (None, Pane::List) => {
            "j/k:nav  Enter:open  /:search  c:claim  x:close  R:refresh  ?:help  q:quit".into()
        }
        (None, Pane::Detail) => {
            "j/k:scroll  r:reply  c:claim  x:close  Esc:back  ?:help  q:back".into()
        }
        (None, Pane::Search) => format!("/{}  Enter:apply  Esc:cancel", app.search),
        (None, Pane::Help) => "Esc / ? / q to dismiss".into(),
    };
    let bar = Paragraph::new(text).style(Style::default().fg(Color::White).bg(Color::DarkGray));
    f.render_widget(bar, area);
}

fn draw_help_overlay(f: &mut Frame, _app: &App) {
    let area = centered_rect(60, 60, f.area());
    let lines = vec![
        Line::from(Span::styled("csshd — keymap", Style::default().add_modifier(Modifier::BOLD))),
        Line::raw(""),
        Line::raw("  List view"),
        Line::raw("    j / ↓     Move selection down"),
        Line::raw("    k / ↑     Move selection up"),
        Line::raw("    g / G     First / last"),
        Line::raw("    Enter     Open ticket detail"),
        Line::raw("    /         Search"),
        Line::raw("    c         Claim selected ticket"),
        Line::raw("    x         Close selected ticket"),
        Line::raw("    R / F5    Refresh now"),
        Line::raw("    q         Quit"),
        Line::raw(""),
        Line::raw("  Detail view"),
        Line::raw("    j / k     Scroll body"),
        Line::raw("    r         Reply ($EDITOR)"),
        Line::raw("    c         Claim"),
        Line::raw("    x         Close"),
        Line::raw("    Esc / q   Back to list"),
        Line::raw(""),
        Line::raw("  Global"),
        Line::raw("    ?         Toggle this help"),
        Line::raw("    Ctrl-C    Quit"),
    ];
    let p = Paragraph::new(lines)
        .block(Block::default().borders(Borders::ALL).title(" Help "))
        .style(Style::default().bg(Color::Black));
    f.render_widget(p, area);
}

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}

// Bare-bones HTML→text. Keep behaviour identical to commands::view::strip_html
// so the same output format is used in both modes.
fn strip_html(input: &str) -> String {
    let body = input.strip_prefix("<!--html-->").unwrap_or(input);
    let mut out = String::with_capacity(body.len());
    let mut in_tag = false;
    for ch in body.chars() {
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
