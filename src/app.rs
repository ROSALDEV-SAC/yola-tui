use anyhow::Context;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use futures::StreamExt;
use ratatui::{
    layout::{Constraint, Direction, Layout},
    style::{Color, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};
use std::io;
use tokio::sync::mpsc;

use crate::daemon;

// ── State ──────────────────────────────────────────────────────────

pub struct App {
    pub input: String,
    pub messages: Vec<Message>,
    pub cursor_pos: usize,
    pub connected: bool,
    pub port: u16,
    pub session_id: Option<String>,
    pub status: String,
    pub is_streaming: bool,
}

pub struct Message {
    pub role: String, // "user", "assistant", "system"
    pub content: String,
}

#[derive(Debug)]
pub enum AppEvent {
    Token(String),
    Done,
    SessionCreated(String),
    Error(String),
    Quit,
}

// ── Entry point ────────────────────────────────────────────────────

pub async fn run(port: u16) -> anyhow::Result<()> {
    // --- terminal setup ---
    enable_raw_mode().context("Failed to enable raw mode")?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)
        .context("Failed to enter alternate screen")?;

    let backend = ratatui::backend::CrosstermBackend::new(stdout);
    let mut terminal = ratatui::Terminal::new(backend).context("Failed to create terminal")?;

    // --- daemon health ---
    let connected = daemon::check_health(port).await;
    let status_line = if connected {
        format!(
            "Connected to daemon on :{}. Type a prompt and press Enter. Esc to quit.",
            port
        )
    } else {
        format!(
            "Daemon not reachable on :{}. Will retry on first prompt.",
            port
        )
    };

    let mut app = App {
        input: String::new(),
        messages: vec![Message {
            role: "system".into(),
            content: status_line.clone(),
        }],
        cursor_pos: 0,
        connected,
        port,
        session_id: None,
        status: status_line,
        is_streaming: false,
    };

    let (tx, mut rx) = mpsc::unbounded_channel::<AppEvent>();

    let result = event_loop(&mut terminal, &mut app, tx, &mut rx).await;

    // --- restore terminal ---
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    result
}

// ── Event loop ─────────────────────────────────────────────────────

async fn event_loop<B: ratatui::backend::Backend>(
    terminal: &mut ratatui::Terminal<B>,
    app: &mut App,
    tx: mpsc::UnboundedSender<AppEvent>,
    rx: &mut mpsc::UnboundedReceiver<AppEvent>,
) -> anyhow::Result<()> {
    loop {
        terminal.draw(|f| ui(f, app))?;

        // ── keyboard ───────────────────────────────────────────
        if event::poll(std::time::Duration::from_millis(50))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    match key.code {
                        KeyCode::Esc => return Ok(()),
                        KeyCode::Char('c')
                            if key.modifiers.contains(KeyModifiers::CONTROL) =>
                        {
                            return Ok(())
                        }
                        KeyCode::Enter => handle_enter(app, &tx),
                        KeyCode::Char(c) => {
                            app.input.insert(app.cursor_pos, c);
                            app.cursor_pos += 1;
                        }
                        KeyCode::Backspace => {
                            if app.cursor_pos > 0 {
                                app.cursor_pos -= 1;
                                app.input.remove(app.cursor_pos);
                            }
                        }
                        KeyCode::Delete => {
                            if app.cursor_pos < app.input.len() {
                                app.input.remove(app.cursor_pos);
                            }
                        }
                        KeyCode::Left => {
                            if app.cursor_pos > 0 {
                                app.cursor_pos -= 1;
                            }
                        }
                        KeyCode::Right => {
                            if app.cursor_pos < app.input.len() {
                                app.cursor_pos += 1;
                            }
                        }
                        KeyCode::Home => app.cursor_pos = 0,
                        KeyCode::End => app.cursor_pos = app.input.len(),
                        _ => {}
                    }
                }
            }
        }

        // ── async events ───────────────────────────────────────
        while let Ok(event) = rx.try_recv() {
            match event {
                AppEvent::SessionCreated(id) => {
                    app.session_id = Some(id);
                }
                AppEvent::Token(token) => {
                    if let Some(msg) = app.messages.last_mut() {
                        if msg.role == "assistant" {
                            msg.content.push_str(&token);
                        }
                    }
                }
                AppEvent::Done => {
                    app.is_streaming = false;
                    app.status = "Ready. Type a prompt.".into();
                }
                AppEvent::Error(e) => {
                    // Append to current assistant bubble if streaming was in progress
                    if let Some(msg) = app.messages.last_mut() {
                        if msg.role == "assistant" {
                            if !msg.content.is_empty() {
                                msg.content.push('\n');
                            }
                            msg.content.push_str(&format!("[{}]", e));
                        }
                    }
                    // Always add a standalone error line
                    app.messages.push(Message {
                        role: "system".into(),
                        content: format!("Error: {}", e),
                    });
                    app.is_streaming = false;
                    app.status = format!("Error: {}", e);
                }
                AppEvent::Quit => return Ok(()),
            }
        }
    }
}

// ── Enter handler ──────────────────────────────────────────────────

fn handle_enter(app: &mut App, tx: &mpsc::UnboundedSender<AppEvent>) {
    if app.is_streaming || app.input.trim().is_empty() {
        return;
    }

    let prompt = std::mem::take(&mut app.input);
    app.cursor_pos = 0;

    app.messages.push(Message {
        role: "user".into(),
        content: prompt.clone(),
    });
    app.messages.push(Message {
        role: "assistant".into(),
        content: String::new(),
    });

    app.is_streaming = true;
    app.status = "Streaming…".into();

    let tx_clone = tx.clone();
    let port = app.port;
    let session_id = app.session_id.clone();

    tokio::spawn(async move {
        stream_prompt(port, session_id, prompt, tx_clone).await;
    });
}

// ── SSE streaming ──────────────────────────────────────────────────

async fn stream_prompt(
    port: u16,
    session_id: Option<String>,
    prompt: String,
    tx: mpsc::UnboundedSender<AppEvent>,
) {
    // Obtain a session (create one lazily if needed)
    let sid = match session_id {
        Some(id) => id,
        None => match daemon::create_session(port).await {
            Ok(id) => {
                let _ = tx.send(AppEvent::SessionCreated(id.clone()));
                id
            }
            Err(e) => {
                let _ = tx.send(AppEvent::Error(format!("Session creation failed: {:#}", e)));
                return;
            }
        },
    };

    // POST the prompt and get the SSE stream
    let response = match daemon::send_prompt(port, &sid, &prompt).await {
        Ok(r) => r,
        Err(e) => {
            let _ = tx.send(AppEvent::Error(format!("Request failed: {:#}", e)));
            return;
        }
    };

    // Drain the byte stream, parsing SSE lines
    let mut stream = response.bytes_stream();
    let mut buf = String::new();

    while let Some(chunk) = stream.next().await {
        match chunk {
            Ok(bytes) => {
                buf.push_str(&String::from_utf8_lossy(&bytes));
                while let Some(nl) = buf.find('\n') {
                    let line = buf[..nl].trim().to_string();
                    buf = buf[nl + 1..].to_string();

                    if line.is_empty() {
                        continue;
                    }
                    if let Some(data) = line.strip_prefix("data: ") {
                        process_sse_data(data, &tx);
                    }
                }
            }
            Err(e) => {
                let _ = tx.send(AppEvent::Error(format!("Stream error: {:#}", e)));
                return;
            }
        }
    }

    let _ = tx.send(AppEvent::Done);
}

/// Parse a single SSE `data:` value (the bit after `data: `).
fn process_sse_data(data: &str, tx: &mpsc::UnboundedSender<AppEvent>) {
    if data == "[DONE]" {
        return; // stream end marker — Done event is sent after the loop
    }

    // Try JSON with multiple known shapes
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(data) {
        // OpenAI / chat-completion shape: choices[0].delta.content
        if let Some(content) = v["choices"][0]["delta"]["content"].as_str() {
            let _ = tx.send(AppEvent::Token(content.to_string()));
            return;
        }
        // Simple { "token": "…" }
        if let Some(token) = v["token"].as_str() {
            let _ = tx.send(AppEvent::Token(token.to_string()));
            return;
        }
        // { "content": "…" }
        if let Some(content) = v["content"].as_str() {
            let _ = tx.send(AppEvent::Token(content.to_string()));
            return;
        }
        // { "text": "…" }
        if let Some(text) = v["text"].as_str() {
            let _ = tx.send(AppEvent::Token(text.to_string()));
            return;
        }
        // { "error": "…" }
        if let Some(error) = v["error"].as_str() {
            let _ = tx.send(AppEvent::Error(error.to_string()));
            return;
        }
    }

    // Non-JSON fallback — emit raw data as a token
    if !data.is_empty() && data != "[DONE]" {
        let _ = tx.send(AppEvent::Token(data.to_string()));
    }
}

// ── UI ─────────────────────────────────────────────────────────────

fn ui(f: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(1),      // messages
            Constraint::Length(3),   // input
            Constraint::Length(1),   // status bar
        ])
        .split(f.size());

    // ── Messages panel ─────────────────────────────────────────
    let lines: Vec<Line> = app
        .messages
        .iter()
        .map(|msg| {
            let color = match msg.role.as_str() {
                "user" => Color::Green,
                "assistant" => Color::White,
                _ => Color::DarkGray,
            };
            let prefix = if msg.role == "user" { "> " } else { "" };

            if prefix.is_empty() {
                Line::from(Span::styled(&msg.content, Style::default().fg(color)))
            } else {
                Line::from(vec![
                    Span::styled(prefix, Style::default().fg(color)),
                    Span::styled(&msg.content, Style::default().fg(color)),
                ])
            }
        })
        .collect();

    let messages_widget = Paragraph::new(Text::from(lines))
        .block(
            Block::default()
                .title(format!(" YOLA Chat :{} ", app.port))
                .borders(Borders::ALL),
        )
        .wrap(Wrap { trim: true });
    f.render_widget(messages_widget, chunks[0]);

    // ── Input panel ────────────────────────────────────────────
    let input_style = if app.is_streaming {
        Style::default().fg(Color::DarkGray)
    } else {
        Style::default().fg(Color::Yellow)
    };
    let input_widget = Paragraph::new(app.input.as_str())
        .block(Block::default().title(" Prompt ").borders(Borders::ALL))
        .style(input_style);
    f.render_widget(input_widget, chunks[1]);

    // ── Status bar ─────────────────────────────────────────────
    let status_widget =
        Paragraph::new(Span::styled(&app.status, Style::default().fg(Color::DarkGray)));
    f.render_widget(status_widget, chunks[2]);

    // ── Cursor ─────────────────────────────────────────────────
    f.set_cursor(
        chunks[1].x + app.cursor_pos as u16 + 1,
        chunks[1].y + 1,
    );
}
