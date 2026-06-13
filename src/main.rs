mod api;
mod app;
mod config;
mod ui;

use anyhow::Result;
use app::{App, ChatMessage, Role, Screen, SetupField};
use api::{list_models, stream_chat, test_connection, StreamEvent};
use config::{normalize_base_url, save};
use crossterm::event::{Event, EventStream, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use futures_util::StreamExt;
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use std::time::Duration;

#[derive(Debug)]
enum AppEvent {
    Key(KeyEvent),
    Api(ApiResponse),
    Stream(StreamEvent),
    Tick,
}

#[derive(Debug)]
enum ApiResponse {
    Models(Result<Vec<api::Model>, String>),
    Test(Result<String, String>),
}

#[tokio::main]
async fn main() -> Result<()> {
    // load config
    let cfg = config::load().unwrap_or_default();
    let mut app = App::new(cfg);

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
                AppEvent::Stream(se) => handle_stream(app, se),
                AppEvent::Tick => {}
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
                SetupField::Nick => SetupField::BaseUrl,
            };
        }
        KeyCode::BackTab => {
            app.setup_field = match app.setup_field {
                SetupField::BaseUrl => SetupField::Nick,
                SetupField::ApiKey => SetupField::BaseUrl,
                SetupField::Nick => SetupField::ApiKey,
            };
        }
        KeyCode::Enter => {
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
            app.streaming = false;
        }
        return Ok(());
    }
    match k.code {
        KeyCode::Esc => {
            app.screen = Screen::ModelSelect;
        }
        KeyCode::Enter => {
            let text = app.input.trim().to_string();
            if text.is_empty() {
                return Ok(());
            }
            if text.starts_with('/') {
                handle_command(app, &text);
                app.input.clear();
                return Ok(());
            }
            app.input_history.push(text.clone());
            app.history_idx = None;
            app.messages.push(ChatMessage {
                role: Role::You,
                content: text.clone(),
                time: app::now(),
            });
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

fn handle_command(app: &mut App, text: &str) {
    let mut parts = text.splitn(2, ' ');
    let cmd = parts.next().unwrap_or("");
    let _arg = parts.next().unwrap_or("").trim();
    match cmd {
        "/help" => {
            app.messages.push(ChatMessage {
                role: Role::System,
                content: "Commands: /help /clear /model /setup /quit".to_string(),
                time: app::now(),
            });
        }
        "/clear" => app.messages.clear(),
        "/model" => {
            app.screen = Screen::ModelSelect;
        }
        "/setup" => {
            app.screen = Screen::Setup;
        }
        "/quit" | "/exit" => {
            std::process::exit(0);
        }
        _ => {
            app.messages.push(ChatMessage {
                role: Role::Error,
                content: format!("Unknown command: {}", cmd),
                time: app::now(),
            });
        }
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
    let mut history: Vec<(String, String)> = app
        .messages
        .iter()
        .filter(|m| matches!(m.role, Role::You | Role::Bot))
        .map(|m| {
            let role = match m.role {
                Role::You => "user".to_string(),
                Role::Bot => "assistant".to_string(),
                _ => "user".to_string(),
            };
            (role, m.content.clone())
        })
        .collect();
    history.push(("user".to_string(), text));

    app.streaming = true;
    app.stream_buf.clear();

    let base = app.cfg.base_url.clone();
    let key = app.cfg.api_key.clone();
    let model = app.cfg.model.clone();
    let stream_tx = tx.clone();

    tokio::spawn(async move {
        match stream_chat(base, key, model, history).await {
            Ok(mut rx) => {
                while let Some(ev) = rx.recv().await {
                    if stream_tx.send(AppEvent::Stream(ev)).is_err() {
                        break;
                    }
                }
            }
            Err(e) => {
                let _ = stream_tx
                    .send(AppEvent::Stream(StreamEvent::Error(e.to_string())));
            }
        }
    });
}

fn handle_stream(app: &mut App, se: StreamEvent) {
    match se {
        StreamEvent::Delta(s) => {
            app.stream_buf.push_str(&s);
        }
        StreamEvent::Done => {
            if !app.stream_buf.is_empty() {
                app.messages.push(ChatMessage {
                    role: Role::Bot,
                    content: std::mem::take(&mut app.stream_buf),
                    time: app::now(),
                });
            }
            app.streaming = false;
        }
        StreamEvent::Error(e) => {
            app.messages.push(ChatMessage {
                role: Role::Error,
                content: format!("Stream error: {}", e),
                time: app::now(),
            });
            app.streaming = false;
            app.stream_buf.clear();
        }
    }
}
