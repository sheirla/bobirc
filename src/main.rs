mod api;
mod app;
mod config;
mod history;
mod osc52;
mod sessions;
mod ui;

use anyhow::Result;
use app::{
    filtered_commands, menu_is_open, strip_think_chunk, App, ChatMessage, Popup, Role, Screen,
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
    init_sessions(&mut app);

    // terminal setup
    let mut stdout = std::io::stdout();
    crossterm::terminal::enable_raw_mode()?;
    crossterm::execute!(
        stdout,
        // No mouse capture: terminal-native text selection (drag to
        // select, Ctrl+Shift+C to copy) works inside the alternate
        // screen. Scroll within the chat is via PageUp/PageDown; wheel
        // events go to the terminal scrollback instead, which is the
        // right trade-off for the use case (selecting chat output).
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
    terminal.draw(|f| ui::draw(f, &mut app))?;

    let res = run(&mut terminal, &mut app, &tx, &mut events, &mut rx).await;

    // restore terminal
    crossterm::terminal::disable_raw_mode()?;
    crossterm::execute!(
        terminal.backend_mut(),
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
        terminal.draw(|f| ui::draw(f, &mut *app))?;

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
                set_toast(app, format!("Model set to: {}", m.id));
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
    // Modal popup. When open, all keys are intercepted here except
    // scroll / close. Chat input is disabled.
    if let Some(popup) = app.popup.as_mut() {
        let popup_h = popup.body.lines().count().max(1) as u16;
        let popup_max_scroll = popup_h.saturating_sub(8); // approx visible rows
        match k.code {
            KeyCode::Esc | KeyCode::Enter | KeyCode::Char('q') | KeyCode::Char('Q') => {
                close_popup(app);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if popup.scroll < popup_max_scroll {
                    popup.scroll += 1;
                }
            }
            KeyCode::Up | KeyCode::Char('k') => {
                popup.scroll = popup.scroll.saturating_sub(1);
            }
            KeyCode::PageDown => {
                popup.scroll = (popup.scroll + 8).min(popup_max_scroll);
            }
            KeyCode::PageUp => {
                popup.scroll = popup.scroll.saturating_sub(8);
            }
            KeyCode::Char('g') => {
                popup.scroll = 0;
            }
            KeyCode::Char('G') => {
                popup.scroll = popup_max_scroll;
            }
            _ => {}
        }
        return Ok(());
    }

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

    // Session nav mode: chat input is disabled, all keys go to the
    // sessions panel until the user exits (Esc / F2).
    if app.session_nav_mode {
        match k.code {
            KeyCode::Esc | KeyCode::F(2) => {
                app.session_nav_mode = false;
                app.session_pending_delete = false;
                return Ok(());
            }
            KeyCode::Up => {
                if !app.sessions.is_empty() && app.session_panel_idx > 0 {
                    app.session_panel_idx -= 1;
                }
                return Ok(());
            }
            KeyCode::Down => {
                if !app.sessions.is_empty()
                    && app.session_panel_idx + 1 < app.sessions.len()
                {
                    app.session_panel_idx += 1;
                }
                return Ok(());
            }
            KeyCode::Enter => {
                if let Some(m) = app.sessions.get(app.session_panel_idx) {
                    let id = m.id.clone();
                    switch_session(app, &id);
                }
                app.session_nav_mode = false;
                app.session_pending_delete = false;
                return Ok(());
            }
            KeyCode::Char('n') => {
                new_session(app);
                app.session_nav_mode = false;
                app.session_pending_delete = false;
                return Ok(());
            }
            KeyCode::Char('d') => {
                if app.session_pending_delete {
                    let idx = app.session_panel_idx;
                    app.session_pending_delete = false;
                    delete_session_at(app, idx);
                } else {
                    app.session_pending_delete = true;
                    set_toast(app, "Press d again to confirm delete");
                }
                return Ok(());
            }
            _ => return Ok(()), // swallow everything else in nav mode
        }
    }

    // F2 enters session-nav mode (from chat)
    if k.code == KeyCode::F(2) {
        app.session_nav_mode = true;
        app.session_pending_delete = false;
        return Ok(());
    }

    // Alt+1..9 quick-switch. Needs the ALT modifier; plain 1..9 goes to
    // the chat input.
    if k.modifiers.contains(KeyModifiers::ALT) {
        if let KeyCode::Char(c) = k.code {
            if let Some(digit) = c.to_digit(10) {
                if (1..=9).contains(&digit) {
                    let idx = (digit - 1) as usize;
                    if let Some(m) = app.sessions.get(idx) {
                        let id = m.id.clone();
                        switch_session(app, &id);
                    }
                    return Ok(());
                }
            }
        }
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

    // `?` opens a :messages-style popup of recent system + error
    // messages. Cheap to collect (we already have them in app.messages).
    if k.code == KeyCode::Char('?') && k.modifiers.is_empty() {
        let lines: Vec<String> = app
            .messages
            .iter()
            .filter(|m| matches!(m.role, Role::System | Role::Error))
            .map(|m| {
                let prefix = match m.role {
                    Role::Error => "! ",
                    _ => "  ",
                };
                let first_line = m.content.lines().next().unwrap_or("");
                format!("[{}] {}{}", m.time, prefix, first_line)
            })
            .collect();
        let body = if lines.is_empty() {
            "(no system or error messages yet)".to_string()
        } else {
            // most recent first reads more naturally in the popup
            let mut rev = lines;
            rev.reverse();
            rev.join("\n")
        };
        open_popup(app, "messages", "Messages", &body);
        return Ok(());
    }

    // n/N search navigation. Has to live BEFORE the main `match k.code`
    // below so that pressing n/N doesn't fall into the Char arm and
    // get pushed to the input first. Only active when a /search is
    // running.
    if app.search_query.is_some() && k.modifiers.is_empty() {
        match k.code {
            KeyCode::Char('n') => {
                if !app.search_matches.is_empty() {
                    app.search_idx = (app.search_idx + 1) % app.search_matches.len();
                    jump_to_match(app, app.search_idx);
                    set_toast(app, format!(
                        "Match {}/{}",
                        app.search_idx + 1,
                        app.search_matches.len()
                    ));
                }
                return Ok(());
            }
            KeyCode::Char('N') => {
                if !app.search_matches.is_empty() {
                    if app.search_idx == 0 {
                        app.search_idx = app.search_matches.len() - 1;
                    } else {
                        app.search_idx -= 1;
                    }
                    jump_to_match(app, app.search_idx);
                    set_toast(app, format!(
                        "Match {}/{}",
                        app.search_idx + 1,
                        app.search_matches.len()
                    ));
                }
                return Ok(());
            }
            KeyCode::Esc => {
                app.search_query = None;
                app.search_matches.clear();
                app.search_idx = 0;
                set_toast(app, "Search cleared");
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
            // auto-name on first user message
            if app
                .sessions
                .iter()
                .find(|m| m.id == app.active_session_id)
                .map(|m| m.name == "New chat")
                .unwrap_or(false)
            {
                if let Some(meta) = app.sessions.iter_mut().find(|m| m.id == app.active_session_id) {
                    meta.name = sessions::auto_name(&user_msg.content);
                }
            }
            save_active_session(app);
            app.input.clear();
            app.scroll = 0;
            app.follow_tail = true;
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
            // scroll UP toward older messages: smaller offset from top
            app.follow_tail = false;
            app.scroll = app.scroll.saturating_sub(5);
        }
        KeyCode::PageDown => {
            // scroll DOWN toward newer messages: larger offset from top
            // (clamped to max_scroll in draw, which flips follow_tail back on)
            app.scroll = app.scroll.saturating_add(5);
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
            // Build a compact reference: every slash command + every
            // key binding. Kept in one popup so the user has a single
            // place to look. Press `?` for the messages log instead.
            let cmds = COMMANDS
                .iter()
                .map(|(c, d)| format!("  {:<14} {}", c, d))
                .collect::<Vec<_>>()
                .join("\n");

            let keys = "\
  F2               session nav mode (Up/Down/Enter/n/d/Esc)
  Alt+1..9         quick switch session
  ?                open :messages log popup
  Tab              autocomplete slash command (when / typed)
  Enter            send message / run command
  Shift+Enter      newline in input
  Esc              cancel stream · close popup · exit nav mode
  n / N            next / prev search match (when /search active)
  j / k            popup scroll down / up
  PageDown / PageUp  popup scroll
  g / G            popup top / bottom
  Up / Down        input history recall
  PageUp / PageDown  chat scroll
  q / Enter / Esc  close popup
  Ctrl+C           quit";

            let body = format!(
                "Boblabs — slash commands\n\n{}\n\nKeys\n\n{}\n",
                cmds, keys
            );
            open_popup(app, "help", "Help", &body);
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
                        set_toast(app, format!("Copied to clipboard: {}", preview));
                    } else {
                        set_toast(
                            app,
                            "Clipboard write rejected (OSC52 not supported)".to_string(),
                        );
                    }
                }
                None => {
                    set_toast(app, "Nothing to copy yet");
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
                Ok(_) => set_toast(
                    app,
                    format!(
                        "Exported {} messages to {}",
                        app.messages.len(),
                        path.display()
                    ),
                ),
                Err(e) => set_toast(app, format!("Export failed: {}", e)),
            }
        }
        "/search" => {
            let query = arg.to_string();
            if query.is_empty() {
                app.search_query = None;
                app.search_matches.clear();
                app.search_idx = 0;
                set_toast(app, "Search cleared");
                return;
            }
            let q_lower = query.to_lowercase();
            app.search_matches = app
                .messages
                .iter()
                .enumerate()
                .filter(|(_, m)| m.content.to_lowercase().contains(&q_lower))
                .map(|(i, _)| i)
                .collect();
            app.search_query = Some(query.clone());
            app.search_idx = 0;
            if app.search_matches.is_empty() {
                set_toast(app, format!("No matches for \"{}\"", query));
            } else {
                jump_to_match(app, 0);
                set_toast(app, format!(
                    "Match 1/{} for \"{}\"  (n=next, N=prev, Esc=clear)",
                    app.search_matches.len(),
                    query
                ));
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
        "/new" | "/newchat" => {
            new_session(app);
        }
        "/sessions" | "/list" => {
            // open a popup listing all sessions, most-recent first
            if app.sessions.is_empty() {
                set_toast(app, "No sessions");
            } else {
                let body = app
                    .sessions
                    .iter()
                    .enumerate()
                    .map(|(i, m)| {
                        let marker = if m.id == app.active_session_id {
                            "●"
                        } else {
                            " "
                        };
                        format!("{} {:>2}. {}", marker, i + 1, m.name)
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                open_popup(app, "sessions", "Sessions", &body);
            }
        }
        "/switch" => {
            // /switch <n|partial-name>
            if arg.is_empty() {
                set_toast(app, "Usage: /switch <n|name>");
            } else if let Some(idx) = arg.parse::<usize>().ok() {
                if idx == 0 || idx > app.sessions.len() {
                    set_toast(
                        app,
                        format!("Index out of range (1..{})", app.sessions.len()),
                    );
                } else {
                    let id = app.sessions[idx - 1].id.clone();
                    switch_session(app, &id);
                }
            } else {
                // substring match (case-insensitive) on name
                let needle = arg.to_lowercase();
                if let Some(m) =
                    app.sessions.iter().find(|m| m.name.to_lowercase().contains(&needle))
                {
                    let id = m.id.clone();
                    switch_session(app, &id);
                } else {
                    set_toast(app, format!("No session matching \"{}\"", arg));
                }
            }
        }
        "/delete" | "/del" => {
            if arg.is_empty() {
                set_toast(app, "Usage: /delete <n>");
            } else if let Some(idx) = arg.parse::<usize>().ok() {
                if idx == 0 || idx > app.sessions.len() {
                    set_toast(
                        app,
                        format!("Index out of range (1..{})", app.sessions.len()),
                    );
                } else {
                    delete_session_at(app, idx - 1);
                }
            } else {
                set_toast(app, format!("\"{}\" is not a number", arg));
            }
        }
        "/rename" => {
            // /rename <new name> -- rename the active session
            if arg.is_empty() {
                set_toast(app, "Usage: /rename <new name>");
            } else if let Some(meta) = app
                .sessions
                .iter_mut()
                .find(|m| m.id == app.active_session_id)
        {
                meta.name = arg.to_string();
                let id = app.active_session_id.clone();
                save_active_session(app);
                if let Some(m) = app.sessions.iter().find(|m| m.id == id) {
                    set_toast(app, format!("Renamed to \"{}\"", m.name));
                }
            } else {
                set_toast(app, "No active session");
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

/// Scroll the chat to the message at `app.search_matches[idx]`. Counts
/// the rendered line offsets of all earlier messages, sets `app.scroll`
/// to land near the target line (with a couple of lines of context
/// above so the user can see what came before), and disengages
/// follow_tail so subsequent streaming doesn't yank the view away.
fn jump_to_match(app: &mut App, idx: usize) {
    if app.search_matches.is_empty() {
        return;
    }
    let target = app.search_matches[idx.min(app.search_matches.len() - 1)];
    let mut line_offset: usize = 0;
    for (i, m) in app.messages.iter().enumerate() {
        if i == target {
            break;
        }
        // Match the line accounting in render_message: each \n is a
        // line break, content is at least 1 line, system/error msgs
        // already split on \n into separate Line objects.
        line_offset += m.content.lines().count().max(1);
    }
    app.scroll = line_offset.saturating_sub(2) as u16;
    app.follow_tail = false;
}

/// Populate the in-memory session list on startup. If disk has
/// sessions, load the most-recently-updated as active. Otherwise
/// create a default session carrying whatever messages App::new
/// already pushed (welcome / "Using model:" notices).
fn init_sessions(app: &mut App) {
    match sessions::list_sessions() {
        Ok(list) if !list.is_empty() => {
            let active_id = list[0].id.clone();
            app.sessions = list;
            app.active_session_id = active_id.clone();
            if let Ok(sess) = sessions::load_session(&active_id) {
                app.messages = sess.messages;
            }
        }
        _ => {
            // no sessions on disk -> create a default one
            let id = sessions::new_session_id();
            let now = app::now();
            let session = sessions::Session {
                meta: sessions::SessionMeta {
                    id: id.clone(),
                    name: "New chat".to_string(),
                    created_at: now.clone(),
                    updated_at: now,
                },
                messages: app.messages.clone(),
                system_prompt: app.cfg.system_prompt.clone(),
            };
            let _ = sessions::save_session(&session);
            if let Ok(list) = sessions::list_sessions() {
                app.sessions = list;
            }
            app.active_session_id = id;
        }
    }
    app.scroll = 0;
    app.follow_tail = true;
}

/// Persist the current session to disk. Best-effort: if the write
/// fails we just push an error message and keep the in-memory state.
fn save_active_session(app: &mut App) -> bool {
    let id = app.active_session_id.clone();
    let name = app
        .sessions
        .iter()
        .find(|m| m.id == id)
        .map(|m| m.name.clone())
        .unwrap_or_else(|| "Current".to_string());
    let now = app::now();
    let created = app
        .sessions
        .iter()
        .find(|m| m.id == id)
        .map(|m| m.created_at.clone())
        .unwrap_or_else(|| now.clone());
    let session = sessions::Session {
        meta: sessions::SessionMeta {
            id: id.clone(),
            name,
            created_at: created,
            updated_at: now,
        },
        messages: app.messages.clone(),
        system_prompt: app.cfg.system_prompt.clone(),
    };
    match sessions::save_session(&session) {
        Ok(()) => {
            if let Ok(list) = sessions::list_sessions() {
                app.sessions = list;
            }
            true
        }
        Err(e) => {
            set_toast(app, format!("Session save failed: {}", e));
            false
        }
    }
}

fn refresh_sessions_list(app: &mut App) {
    if let Ok(list) = sessions::list_sessions() {
        app.sessions = list;
    }
}

fn set_toast(app: &mut App, msg: impl Into<String>) {
    app.toast = Some((msg.into(), std::time::Instant::now()));
}

/// Open a popup, preserving scroll position if the same `id` was
/// open before. Pass `id` like "help" or "messages" so re-opening
/// restores the user's last scroll.
fn open_popup(app: &mut App, id: &str, title: &str, body: &str) {
    let preserved_scroll = app
        .popup
        .as_ref()
        .filter(|p| p.id == id)
        .map(|p| p.scroll)
        .unwrap_or(0);
    app.popup = Some(Popup {
        id: id.to_string(),
        title: title.to_string(),
        body: body.to_string(),
        scroll: preserved_scroll,
    });
}

fn close_popup(app: &mut App) {
    app.popup = None;
}

fn switch_session(app: &mut App, target_id: &str) {
    if target_id == app.active_session_id {
        return;
    }
    save_active_session(app);
    match sessions::load_session(target_id) {
        Ok(sess) => {
            app.active_session_id = target_id.to_string();
            app.messages = sess.messages;
            app.scroll = 0;
            app.follow_tail = true;
            app.search_query = None;
            app.search_matches.clear();
            app.search_idx = 0;
            if let Some(idx) = app.sessions.iter().position(|m| m.id == target_id) {
                app.session_panel_idx = idx;
            }
            set_toast(app, format!("Switched to: {}", sess.meta.name));
        }
        Err(e) => {
            set_toast(app, format!("Load failed: {}", e));
        }
    }
}

fn new_session(app: &mut App) {
    save_active_session(app);
    let id = sessions::new_session_id();
    let now = app::now();
    let session = sessions::Session {
        meta: sessions::SessionMeta {
            id: id.clone(),
            name: "New chat".to_string(),
            created_at: now.clone(),
            updated_at: now,
        },
        messages: vec![ChatMessage::system(
            "Welcome. Type to start. /system to set a prompt, /model to pick a model."
                .to_string(),
        )],
        system_prompt: app.cfg.system_prompt.clone(),
    };
    if let Err(e) = sessions::save_session(&session) {
        set_toast(app, format!("New session failed: {}", e));
        return;
    }
    refresh_sessions_list(app);
    app.active_session_id = id.clone();
    app.messages = session.messages;
    app.scroll = 0;
    app.follow_tail = true;
    app.search_query = None;
    app.search_matches.clear();
    app.search_idx = 0;
    app.cfg.system_prompt = session.system_prompt;
    if let Some(idx) = app.sessions.iter().position(|m| m.id == id) {
        app.session_panel_idx = idx;
    }
    set_toast(app, "New session");
}

fn delete_session_at(app: &mut App, idx: usize) {
    if idx >= app.sessions.len() {
        return;
    }
    let target = app.sessions[idx].clone();
    if target.id == app.active_session_id {
        set_toast(app, "Can't delete the active session -- switch first");
        return;
    }
    if let Err(e) = sessions::delete_session(&target.id) {
        set_toast(app, format!("Delete failed: {}", e));
        return;
    }
    refresh_sessions_list(app);
    if app.session_panel_idx >= app.sessions.len() && !app.sessions.is_empty() {
        app.session_panel_idx = app.sessions.len() - 1;
    }
    set_toast(app, format!("Deleted: {}", target.name));
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
        set_toast(app, "No model selected. Use /model");
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
                save_active_session(app);
            }
            app.streaming = false;
            app.stream_cancel = None;
        }
        StreamEvent::Error(e) => {
            set_toast(app, format!("Stream error: {}", e));
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
