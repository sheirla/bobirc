mod api;
mod app;
mod config;
mod history;
mod osc52;
mod ui;

use anyhow::Result;
use app::{
    filtered_commands, menu_is_open, strip_think_chunk, App, ChatMessage, Role, Screen,
    SetupField, ThinkState, TokenUsage, COMMANDS,
};
use api::{list_models, stream_chat, test_connection, StreamEvent};
use config::{normalize_base_url, save};
use crossterm::event::{Event, EventStream, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use futures_util::StreamExt;
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

#[derive(Debug)]
enum AppEvent {
    Key(KeyEvent),
    Api(ApiResponse),
    Stream { epoch: u64, ev: StreamEvent },
    Tick,
}

#[derive(Debug)]
enum ApiResponse {
    Models(Result<Vec<api::Model>, String>),
    Test(Result<String, String>),
}

#[tokio::main]
async fn main() -> Result<()> {
    // CLI flags: print and exit before touching config or terminal.
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("bobric {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!(
            "bobric — mIRC-style TUI chat for OpenAI-compatible LLM APIs\n\
             \n\
             USAGE:\n    \
                 bobric\n\
             \n\
             OPTIONS:\n    \
                 -h, --help     print this help and exit\n    \
                 -V, --version  print version and exit\n\
             \n\
             CONFIG:    ~/.config/bobric/config.json\n  \
             HISTORY:   ~/.config/bobric/history.jsonl\n  \
             KEYS:      see README.md"
        );
        return Ok(());
    }

    // load config
    let cfg = config::load().unwrap_or_default();
    let mut app = App::new(cfg);
    // load persistent message history (best effort)
    if let Ok(saved) = history::load() {
        if !saved.is_empty() {
            app.messages.extend(saved);
        }
    }

    // terminal setup
    let mut stdout = std::io::stdout();
    crossterm::terminal::enable_raw_mode()?;
    crossterm::execute!(
        stdout,
        crossterm::event::EnableMouseCapture,
        crossterm::terminal::EnterAlternateScreen,
    )?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    terminal.hide_cursor()?;

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<AppEvent>();
    let mut events = EventStream::new();

    // tick task
    let tick_tx = tx.clone();
    tokio::spawn(async move {
        let mut iv = tokio::time::interval(Duration::from_millis(80));
        loop {
            iv.tick().await;
            if tick_tx.send(AppEvent::Tick).is_err() {
                break;
            }
        }
    });

    // initial draw
    terminal.draw(|f| ui::draw(f, &app))?;

    let res = run(&mut terminal, &mut app, &tx, &mut events, &mut rx).await;

    // restore terminal
    crossterm::terminal::disable_raw_mode()?;
    crossterm::execute!(
        terminal.backend_mut(),
        crossterm::event::DisableMouseCapture,
        crossterm::terminal::LeaveAlternateScreen,
    )?;
    terminal.show_cursor()?;

    res
}

async fn run<B>(
    terminal: &mut Terminal<CrosstermBackend<B>>,
    app: &mut App,
    tx: &tokio::sync::mpsc::UnboundedSender<AppEvent>,
    events: &mut EventStream,
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<AppEvent>,
) -> Result<()>
where
    B: std::io::Write,
{
    loop {
        terminal.draw(|f| ui::draw(f, app))?;

        let evt = tokio::select! {
            Some(e) = events.next() => {
                match e {
                    Ok(Event::Key(k)) => Some(AppEvent::Key(k)),
                    Ok(_) => None,
                    Err(_) => None,
                }
            }
            Some(a) = rx.recv() => Some(a),
            else => None,
        };

        if let Some(e) = evt {
            match e {
                AppEvent::Key(k) => {
                    if handle_key(app, tx, k).await? {
                        break;
                    }
                }
                AppEvent::Api(resp) => handle_api_resp(app, resp),
                AppEvent::Stream { epoch, ev } => handle_stream(app, epoch, ev),
                AppEvent::Tick => {
                    app.spinner_frame = app.spinner_frame.wrapping_add(1);
                }
            }
        }
    }
    Ok(())
}

async fn handle_key(
    app: &mut App,
    tx: &tokio::sync::mpsc::UnboundedSender<AppEvent>,
    k: KeyEvent,
) -> Result<bool> {
    if k.kind != KeyEventKind::Press {
        return Ok(false);
    }
    // global quit
    if k.code == KeyCode::Char('c') && k.modifiers.contains(KeyModifiers::CONTROL) {
        return Ok(true);
    }

    match app.screen {
        Screen::Setup => handle_setup_key(app, k).await?,
        Screen::ModelSelect => handle_model_key(app, tx, k).await?,
        Screen::Chat => handle_chat_key(app, tx, k).await?,
    }
    Ok(false)
}

async fn handle_setup_key(app: &mut App, k: KeyEvent) -> Result<()> {
    match k.code {
        KeyCode::Esc => {
            if app.cfg.is_configured() {
                app.screen = Screen::Chat;
            } else {
                std::process::exit(0);
            }
        }
        KeyCode::Tab => {
            app.setup_field = match app.setup_field {
                SetupField::BaseUrl => SetupField::ApiKey,
                SetupField::ApiKey => SetupField::Nick,
                SetupField::Nick => SetupField::SystemPrompt,
                SetupField::SystemPrompt => SetupField::BaseUrl,
            };
        }
        KeyCode::BackTab => {
            app.setup_field = match app.setup_field {
                SetupField::BaseUrl => SetupField::SystemPrompt,
                SetupField::ApiKey => SetupField::BaseUrl,
                SetupField::Nick => SetupField::ApiKey,
                SetupField::SystemPrompt => SetupField::Nick,
            };
        }
        KeyCode::Enter => {
            // Shift+Enter inserts a newline in the system prompt field
            if k.modifiers.contains(KeyModifiers::SHIFT)
                && app.setup_field == SetupField::SystemPrompt
            {
                app.setup_system.push('\n');
                return Ok(());
            }
            let base = normalize_base_url(&app.setup_base);
            if base.is_empty() || app.setup_key.is_empty() {
                app.setup_status = Some("ERROR: base URL and API key are required".to_string());
                return Ok(());
            }
            app.cfg.base_url = base;
            app.cfg.api_key = app.setup_key.clone();
            app.cfg.nick = if app.setup_nick.trim().is_empty() {
                "bob".to_string()
            } else {
                app.setup_nick.trim().to_string()
            };
            app.cfg.system_prompt = app.setup_system.clone();
            save(&app.cfg)?;
            app.setup_status = Some(format!(
                "OK · saved to {}",
                config::config_path()?.display()
            ));
            app.cfg_dirty = true;
            app.screen = Screen::ModelSelect;
            app.model_status = Some("Saved. Press F to fetch models.".to_string());
        }
        KeyCode::Backspace => {
            let target = match app.setup_field {
                SetupField::BaseUrl => &mut app.setup_base,
                SetupField::ApiKey => &mut app.setup_key,
                SetupField::Nick => &mut app.setup_nick,
                SetupField::SystemPrompt => &mut app.setup_system,
            };
            target.pop();
        }
        KeyCode::Char(c) => {
            if k.modifiers.contains(KeyModifiers::CONTROL) {
                return Ok(());
            }
            let target = match app.setup_field {
                SetupField::BaseUrl => &mut app.setup_base,
                SetupField::ApiKey => &mut app.setup_key,
                SetupField::Nick => &mut app.setup_nick,
                SetupField::SystemPrompt => &mut app.setup_system,
            };
            target.push(c);
        }
        _ => {}
    }
    Ok(())
}

async fn handle_model_key(
    app: &mut App,
    tx: &tokio::sync::mpsc::UnboundedSender<AppEvent>,
    k: KeyEvent,
) -> Result<()> {
    if app.fetching || app.testing {
        return Ok(());
    }
    match k.code {
        KeyCode::Esc => {
            app.screen = Screen::Setup;
        }
        KeyCode::Char('f') | KeyCode::Char('F') => {
            do_fetch(app, tx.clone());
        }
        KeyCode::Char('t') | KeyCode::Char('T') => {
            do_test(app, tx.clone());
        }
        KeyCode::Up => {
            if app.model_idx > 0 {
                app.model_idx -= 1;
            }
        }
        KeyCode::Down => {
            if app.model_idx + 1 < app.models.len() {
                app.model_idx += 1;
            }
        }
        KeyCode::PageUp => {
            app.model_idx = app.model_idx.saturating_sub(5);
        }
        KeyCode::PageDown => {
            if app.models.is_empty() {
                app.model_idx = 0;
            } else {
                app.model_idx = (app.model_idx + 5).min(app.models.len() - 1);
            }
        }
        KeyCode::Enter => {
            if let Some(m) = app.models.get(app.model_idx) {
                app.cfg.model = m.id.clone();
                save(&app.cfg)?;
                app.screen = Screen::Chat;
                app.messages.push(ChatMessage {
                    role: Role::System,
                    content: format!("Model set to: {}", m.id),
                    time: app::now(),
                });
            } else {
                app.model_status = Some("ERROR: no model selected. Press F first.".to_string());
            }
        }
        _ => {}
    }
    Ok(())
}

async fn handle_chat_key(
    app: &mut App,
    tx: &tokio::sync::mpsc::UnboundedSender<AppEvent>,
    k: KeyEvent,
) -> Result<()> {
    if app.streaming {
        if k.code == KeyCode::Esc {
            // cancel the in-flight stream: signal task + bump epoch
            // so any in-flight events from it get filtered out
            if let Some(c) = app.stream_cancel.take() {
                c.store(false, Ordering::SeqCst);
            }
            app.stream_epoch = app.stream_epoch.wrapping_add(1);
            app.streaming = false;
            app.stream_buf.clear();
            app.think_state = ThinkState::Normal;
            app.think_pending.clear();
        }
        return Ok(());
    }

    // Slash menu intercepts. Only keys that exclusively belong to the
    // menu are handled here; everything else falls through to the normal
    // chat handler so typing keeps working.
    if menu_is_open(app) {
        match k.code {
            KeyCode::Up => {
                if app.menu_idx > 0 {
                    app.menu_idx -= 1;
                }
                return Ok(());
            }
            KeyCode::Down => {
                let count = filtered_commands(&app.input).len();
                if count > 0 && app.menu_idx + 1 < count {
                    app.menu_idx += 1;
                }
                return Ok(());
            }
            KeyCode::Tab => {
                let list = filtered_commands(&app.input);
                let idx = app.menu_idx.min(list.len().saturating_sub(1));
                if let Some(cmd) = list.get(idx) {
                    app.input = format!("{} ", cmd);
                }
                return Ok(());
            }
            KeyCode::Esc => {
                // close menu by clearing the in-progress command
                app.input.clear();
                return Ok(());
            }
            _ => {}
        }
    }

    match k.code {
        KeyCode::Esc => {
            app.screen = Screen::ModelSelect;
        }
        KeyCode::Enter => {
            // Shift+Enter inserts a newline (multi-line input)
            if k.modifiers.contains(KeyModifiers::SHIFT) {
                app.input.push('\n');
                return Ok(());
            }
            // If the slash menu is visible, run the highlighted command
            // rather than whatever the user has typed so far. This means
            // "/h" + Enter runs /help, not "unknown command /h".
            let text = if menu_is_open(app) {
                let list = filtered_commands(&app.input);
                let idx = app.menu_idx.min(list.len().saturating_sub(1));
                list.get(idx).cloned().unwrap_or_else(|| app.input.trim().to_string())
            } else {
                app.input.trim().to_string()
            };
            if text.is_empty() {
                return Ok(());
            }
            if text.starts_with('/') {
                handle_command(app, tx, &text);
                app.input.clear();
                return Ok(());
            }
            app.input_history.push(text.clone());
            app.history_idx = None;
            let user_msg = ChatMessage {
                role: Role::You,
                content: text.clone(),
                time: app::now(),
            };
            app.messages.push(user_msg.clone());
            let _ = history::append_message(&user_msg);
            app.input.clear();
            app.scroll = 0;
            do_stream(app, tx.clone(), text);
        }
        KeyCode::Backspace => {
            app.input.pop();
        }
        KeyCode::Up => {
            if app.input_history.is_empty() {
                return Ok(());
            }
            let idx = match app.history_idx {
                None => app.input_history.len() - 1,
                Some(i) => i.saturating_sub(1),
            };
            app.history_idx = Some(idx);
            app.input = app.input_history[idx].clone();
        }
        KeyCode::Down => {
            if let Some(i) = app.history_idx {
                if i + 1 < app.input_history.len() {
                    app.history_idx = Some(i + 1);
                    app.input = app.input_history[i + 1].clone();
                } else {
                    app.history_idx = None;
                    app.input.clear();
                }
            }
        }
        KeyCode::PageUp => {
            app.scroll = app.scroll.saturating_add(5);
        }
        KeyCode::PageDown => {
            app.scroll = app.scroll.saturating_sub(5);
        }
        KeyCode::Char(c) => {
            if k.modifiers.contains(KeyModifiers::CONTROL) {
                return Ok(());
            }
            app.input.push(c);
        }
        _ => {}
    }
    Ok(())
}

fn handle_command(
    app: &mut App,
    tx: &tokio::sync::mpsc::UnboundedSender<AppEvent>,
    text: &str,
) {
    let mut parts = text.splitn(2, ' ');
    let cmd = parts.next().unwrap_or("");
    let arg = parts.next().unwrap_or("").trim();
    match cmd {
        "/help" => {
            let help = COMMANDS
                .iter()
                .map(|(c, d)| format!("  {} — {}", c, d))
                .collect::<Vec<_>>()
                .join("\n");
            let hint = "Shift+Enter = newline · Tab in /menu = complete · ↑/↓ = navigate";
            let body = format!("{}\n{}", help, hint);
            app.messages.push(ChatMessage::system(body));
        }
        "/clear" => {
            app.messages.clear();
            let _ = history::clear();
        }
        "/copy" => {
            // copy the most recent bot message to the system clipboard
            // via OSC52 escape. Works in most modern terminals (kitty,
            // wezterm, alacritty >= 0.13, foot, iTerm2, Windows Terminal,
            // recent gnome-terminal, etc).
            let last_bot = app
                .messages
                .iter()
                .rev()
                .find(|m| matches!(m.role, Role::Bot))
                .map(|m| m.content.clone());
            match last_bot {
                Some(text) => {
                    let ok = osc52::copy_to_clipboard(&text);
                    if ok {
                        let preview = shorten(&text, 60);
                        app.messages
                            .push(ChatMessage::system(format!("Copied to clipboard: {}", preview)));
                    } else {
                        app.messages.push(ChatMessage::error(
                            "Clipboard write rejected by terminal (OSC52 not supported)".to_string(),
                        ));
                    }
                }
                None => {
                    app.messages
                        .push(ChatMessage::error("Nothing to copy yet".to_string()));
                }
            }
        }
        "/export" => {
            let path = if arg.is_empty() {
                // default to <config_dir>/bobric-export-<timestamp>.txt
                let stamp = app::now().replace(':', "");
                let dir = match config::config_path().and_then(|p| {
                    p.parent()
                        .map(|p| p.to_path_buf())
                        .ok_or_else(|| anyhow::anyhow!("no parent dir"))
                }) {
                    Ok(d) => d,
                    Err(e) => {
                        app.messages
                            .push(ChatMessage::error(format!("Cannot resolve export dir: {}", e)));
                        return;
                    }
                };
                dir.join(format!("bobric-export-{}.txt", stamp))
            } else {
                std::path::PathBuf::from(arg)
            };
            let text = format_chat_for_export(app);
            match std::fs::write(&path, &text) {
                Ok(_) => app.messages.push(ChatMessage::system(format!(
                    "Exported {} messages to {}",
                    app.messages.len(),
                    path.display()
                ))),
                Err(e) => app.messages
                    .push(ChatMessage::error(format!("Export failed: {}", e))),
            }
        }
        "/model" => {
            app.screen = Screen::ModelSelect;
            // auto-refresh when entering via /model and we have no list
            if app.models.is_empty()
                && !app.fetching
                && !app.testing
                && app.cfg.is_configured()
            {
                do_fetch(app, tx.clone());
            }
        }
        "/setup" => {
            app.screen = Screen::Setup;
        }
        "/system" => {
            app.setup_field = SetupField::SystemPrompt;
            app.screen = Screen::Setup;
        }
        "/quit" | "/exit" => {
            std::process::exit(0);
        }
        _ => {
            app.messages
                .push(ChatMessage::error(format!("Unknown command: {}", cmd)));
        }
    }
}

/// Render the current in-memory chat as plain text suitable for pasting
/// into a file or email. Markdown is stripped: bold/italic markers are
/// removed, code fences flattened, headings keep the leading `#`s only.
fn format_chat_for_export(app: &App) -> String {
    use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
    let mut out = String::new();
    out.push_str(&format!("# bobric chat export — {}\n\n", app::now()));
    for m in &app.messages {
        let role = match m.role {
            Role::You => "you",
            Role::Bot => "boblabs",
            Role::System => "system",
            Role::Error => "error",
        };
        out.push_str(&format!("[{}] {}: ", m.time, role));
        // strip markdown to plain text
        let mut options = Options::empty();
        options.insert(Options::ENABLE_STRIKETHROUGH);
        let parser = Parser::new_ext(&m.content, options);
        for ev in parser {
            match ev {
                Event::Text(t) | Event::Code(t) => out.push_str(&t),
                Event::SoftBreak | Event::HardBreak => out.push('\n'),
                Event::Start(Tag::Paragraph)
                | Event::End(TagEnd::Paragraph)
                | Event::Start(Tag::Item)
                | Event::End(TagEnd::Item)
                | Event::Start(Tag::Emphasis)
                | Event::End(TagEnd::Emphasis)
                | Event::Start(Tag::Strong)
                | Event::End(TagEnd::Strong)
                | Event::Start(Tag::Link { .. })
                | Event::End(TagEnd::Link) => {}
                Event::Start(Tag::Heading { .. }) => out.push_str("\n\n# "),
                Event::End(TagEnd::Heading(_)) => out.push('\n'),
                Event::Start(Tag::CodeBlock(_)) => out.push_str("\n```\n"),
                Event::End(TagEnd::CodeBlock) => out.push_str("\n```\n"),
                Event::Start(Tag::List(_)) => out.push('\n'),
                Event::End(TagEnd::List(_)) => out.push('\n'),
                Event::Rule => out.push_str("\n---\n"),
                _ => {}
            }
        }
        out.push('\n');
    }
    out
}

fn shorten(s: &str, n: usize) -> String {
    let cleaned: String = s.chars().filter(|c| *c != '\n' && *c != '\r').collect();
    if cleaned.chars().count() <= n {
        cleaned
    } else {
        let cut: String = cleaned.chars().take(n.saturating_sub(1)).collect();
        format!("{}…", cut)
    }
}

fn do_fetch(app: &mut App, tx: tokio::sync::mpsc::UnboundedSender<AppEvent>) {
    app.fetching = true;
    app.model_status = Some("Fetching models...".to_string());
    let base = app.cfg.base_url.clone();
    let key = app.cfg.api_key.clone();
    tokio::spawn(async move {
        let res = list_models(&base, &key).await.map_err(|e| e.to_string());
        let _ = tx.send(AppEvent::Api(ApiResponse::Models(res)));
    });
}

fn do_test(app: &mut App, tx: tokio::sync::mpsc::UnboundedSender<AppEvent>) {
    app.testing = true;
    app.model_status = Some("Testing connection...".to_string());
    let base = app.cfg.base_url.clone();
    let key = app.cfg.api_key.clone();
    tokio::spawn(async move {
        let res = test_connection(&base, &key).await.map_err(|e| e.to_string());
        let _ = tx.send(AppEvent::Api(ApiResponse::Test(res)));
    });
}

fn handle_api_resp(app: &mut App, resp: ApiResponse) {
    match resp {
        ApiResponse::Models(Ok(models)) => {
            app.fetching = false;
            app.models = models.clone();
            if let Some(idx) = models.iter().position(|m| m.id == app.cfg.model) {
                app.model_idx = idx;
            } else {
                app.model_idx = 0;
            }
            app.model_status = Some(format!("OK · {} models", models.len()));
        }
        ApiResponse::Models(Err(e)) => {
            app.fetching = false;
            app.model_status = Some(format!("ERROR: {}", e));
        }
        ApiResponse::Test(Ok(msg)) => {
            app.testing = false;
            app.model_status = Some(format!("OK · {}", msg));
        }
        ApiResponse::Test(Err(e)) => {
            app.testing = false;
            app.model_status = Some(format!("ERROR: {}", e));
        }
    }
}

fn do_stream(app: &mut App, tx: tokio::sync::mpsc::UnboundedSender<AppEvent>, text: String) {
    if app.cfg.model.is_empty() {
        app.messages.push(ChatMessage {
            role: Role::Error,
            content: "No model selected. Use /model".to_string(),
            time: app::now(),
        });
        return;
    }
    // cancel any prior stream first
    if let Some(c) = app.stream_cancel.take() {
        c.store(false, Ordering::SeqCst);
    }
    app.stream_epoch = app.stream_epoch.wrapping_add(1);
    let epoch = app.stream_epoch;

    let mut history: Vec<(String, String)> = Vec::new();
    // system prompt: always send the effective one. App::new preloads the
    // default if the user has not customised it, so this is never empty
    // here, but we still trim defensively.
    let sp = app.cfg.system_prompt.trim();
    if !sp.is_empty() {
        history.push(("system".to_string(), sp.to_string()));
    }
    history.extend(
        app.messages
            .iter()
            .filter(|m| matches!(m.role, Role::You | Role::Bot))
            .map(|m| {
                let role = match m.role {
                    Role::You => "user".to_string(),
                    Role::Bot => "assistant".to_string(),
                    _ => "user".to_string(),
                };
                (role, m.content.clone())
            }),
    );
    history.push(("user".to_string(), text));

    app.streaming = true;
    app.stream_buf.clear();
    app.think_state = ThinkState::Normal;
    app.think_pending.clear();

    let base = app.cfg.base_url.clone();
    let key = app.cfg.api_key.clone();
    let model = app.cfg.model.clone();
    let stream_tx = tx.clone();
    let cancel = Arc::new(std::sync::atomic::AtomicBool::new(true));
    let cancel_for_task = cancel.clone();
    app.stream_cancel = Some(cancel);

    tokio::spawn(async move {
        match stream_chat(base, key, model, history).await {
            Ok(mut rx) => {
                while let Some(ev) = rx.recv().await {
                    if !cancel_for_task.load(Ordering::SeqCst) {
                        break;
                    }
                    if stream_tx
                        .send(AppEvent::Stream { epoch, ev })
                        .is_err()
                    {
                        break;
                    }
                }
            }
            Err(e) => {
                if cancel_for_task.load(Ordering::SeqCst) {
                    let _ = stream_tx.send(AppEvent::Stream {
                        epoch,
                        ev: StreamEvent::Error(e.to_string()),
                    });
                }
            }
        }
    });
}

fn handle_stream(app: &mut App, epoch: u64, se: StreamEvent) {
    // filter out events from cancelled/old streams
    if app.stream_epoch != epoch || !app.streaming {
        return;
    }
    match se {
        StreamEvent::Delta(s) => {
            // strip <think>...</think> content before it hits the buffer
            let clean = strip_think_chunk(&s, &mut app.think_state, &mut app.think_pending);
            if !clean.is_empty() {
                app.stream_buf.push_str(&clean);
            }
        }
        StreamEvent::Done => {
            // drain any safe text left in the think-strip buffer
            if app.think_state == ThinkState::Normal && !app.think_pending.is_empty() {
                let remaining = std::mem::take(&mut app.think_pending);
                app.stream_buf.push_str(&remaining);
            } else {
                // stream ended mid-think (unclosed tag) -- drop the buffer
                app.think_pending.clear();
            }
            app.think_state = ThinkState::Normal;
            if !app.stream_buf.is_empty() {
                let bot_msg = ChatMessage {
                    role: Role::Bot,
                    content: std::mem::take(&mut app.stream_buf),
                    time: app::now(),
                };
                let _ = history::append_message(&bot_msg);
                app.messages.push(bot_msg);
            }
            app.streaming = false;
            app.stream_cancel = None;
        }
        StreamEvent::Error(e) => {
            app.messages.push(ChatMessage {
                role: Role::Error,
                content: format!("Stream error: {}", e),
                time: app::now(),
            });
            app.streaming = false;
            app.stream_buf.clear();
            app.stream_cancel = None;
        }
        StreamEvent::Usage(u) => {
            app.last_usage = Some(TokenUsage {
                prompt: u.prompt_tokens,
                completion: u.completion_tokens,
                total: u.total_tokens,
            });
        }
    }
}
