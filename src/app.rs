use crate::api::Model;
use crate::config::Config;
use chrono::Local;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

/// Default bot name surfaced in the chat (UI + user list).
pub const BOT_NAME: &str = "Boblabs";

/// Robust default system prompt. Establishes identity, mandates 10XTHINK
/// reasoning, sets tone and boundaries. Sent as the first chat-completion
/// message whenever `cfg.system_prompt` is empty.
pub const DEFAULT_SYSTEM_PROMPT: &str = r#"You are Boblabs, a personal AI assistant. You think deeply, you answer directly, you stay in context.

## Identity
- You are Boblabs — a personal AI assistant. You are not a generic chatbot.
- Tone: clear, friendly, slightly informal. No corporate fluff, no "I'd be happy to help!" filler, no apologies for being an AI.
- Match the user's language. If they write in Indonesian, reply in Indonesian.

## How you think (10XTHINK)
For any non-trivial request, slow down before answering:
1. Rephrase the question. Make sure you understand what is actually being asked and what is at stake.
2. Surface the assumptions and constraints. If something is ambiguous, ask one focused clarifying question instead of guessing.
3. Consider at least two angles. Steelman the one you disagree with.
4. Pick a path. Explain the reasoning in one or two sentences, not a wall of text.
5. Self-check: would a smart skeptical friend accept this answer? If not, revise.
6. For code: read the actual code path, do not pattern-match. Prefer the smallest correct change. Flag the risks you see.

Skip the deep dive for trivial questions (greetings, one-word lookups, simple math). The point of 10XTHINK is to be reliably correct, not slow on easy things.

## How you write
- Lead with the answer, then the reasoning. Do not bury the conclusion in preamble.
- Default to clear prose. Use lists, tables, and code blocks when they actually help — not as decoration.
- Be concise when the question is simple. Be thorough when it isn't.
- Markdown is fine and usually welcome.
- Code: fenced blocks with the right language tag. No trailing commentary inside code blocks.
- Do not fabricate. If you do not know, say so. Offer to find out or suggest where to look.

## Boundaries
- Do not pretend to have access to tools, files, or the internet you do not have.
- Do not volunteer the user's private context to anyone.
- If a request is harmful, refuse briefly and offer a safer alternative.
- For destructive or irreversible actions, restate the plan and ask before doing it."#;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ChatMessage {
    pub role: Role,
    pub content: String,
    pub time: String,
}

impl ChatMessage {
    pub fn system(content: impl Into<String>) -> Self {
        Self { role: Role::System, content: content.into(), time: now() }
    }
    pub fn error(content: impl Into<String>) -> Self {
        Self { role: Role::Error, content: content.into(), time: now() }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Role {
    System,
    You,
    Bot,
    Error,
}

/// State for stripping `<think>...</think>` blocks out of streamed deltas.
/// `InThink` means the next close-tag (`</think>`) is what releases the
/// next batch of normal text. We also keep a small `pending` buffer so a
/// tag split across two chunks still gets recognised.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ThinkState {
    Normal,
    InThink,
}

const THINK_OPEN: &str = "<think>";
const THINK_CLOSE: &str = "</think>";

/// Feed one streamed chunk through the think-strip state machine.
/// `pending` is the rolling buffer of bytes that might be the start of a
/// tag. Returns the cleaned text to display (possibly empty). The caller
/// must keep the same `state` and `pending` across calls for the same
/// stream.
pub fn strip_think_chunk(
    input: &str,
    state: &mut ThinkState,
    pending: &mut String,
) -> String {
    pending.push_str(input);
    let mut output = String::new();
    loop {
        match state {
            ThinkState::Normal => match pending.find(THINK_OPEN) {
                Some(pos) => {
                    output.push_str(&pending[..pos]);
                    pending.drain(..pos + THINK_OPEN.len());
                    *state = ThinkState::InThink;
                }
                None => {
                    // Flush everything that cannot be the start of `<think>`.
                    // Need to keep the last (THINK_OPEN.len() - 1) chars
                    // around in case they begin the open tag.
                    if pending.len() > THINK_OPEN.len() - 1 {
                        let safe_len = pending.len() - (THINK_OPEN.len() - 1);
                        output.push_str(&pending[..safe_len]);
                        pending.drain(..safe_len);
                    }
                    break;
                }
            },
            ThinkState::InThink => match pending.find(THINK_CLOSE) {
                Some(pos) => {
                    pending.drain(..pos + THINK_CLOSE.len());
                    *state = ThinkState::Normal;
                }
                None => {
                    // Drop everything that cannot be the start of `</think>`.
                    if pending.len() > THINK_CLOSE.len() - 1 {
                        let keep = THINK_CLOSE.len() - 1;
                        pending.drain(..pending.len() - keep);
                    }
                    break;
                }
            },
        }
    }
    output
}

impl Role {
    pub fn _nick(&self, _cfg_nick: &str) -> &'static str {
        match self {
            Role::You => "You",
            Role::Bot => "Bot",
            Role::System => "--",
            Role::Error => "!",
        }
    }
    pub fn _user_nick<'a>(&'a self, cfg_nick: &'a str) -> &'a str {
        match self {
            Role::You => cfg_nick,
            _ => "",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Screen {
    Setup,
    ModelSelect,
    Chat,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SetupField {
    BaseUrl,
    ApiKey,
    Nick,
    SystemPrompt,
}

/// Modal popup. A simple generic container -- title bar + scrollable
/// body + a scroll offset. The `id` lets us restore the user's
/// scroll position when the same popup is re-opened.
#[derive(Debug, Clone)]
pub struct Popup {
    pub id: String,
    pub title: String,
    pub body: String,
    pub scroll: u16,
}

pub struct App {
    pub screen: Screen,
    pub cfg: Config,
    pub cfg_dirty: bool,

    // Setup screen
    pub setup_field: SetupField,
    pub setup_base: String,
    pub setup_key: String,
    pub setup_nick: String,
    pub setup_system: String,
    pub setup_status: Option<String>,

    // Model select
    pub models: Vec<Model>,
    pub model_idx: usize,
    pub model_status: Option<String>,
    pub fetching: bool,
    pub testing: bool,

    // Chat
    pub messages: Vec<ChatMessage>,
    pub input: String,
    pub input_history: Vec<String>,
    pub history_idx: Option<usize>,
    /// Vertical scroll offset of the chat area in lines, measured from
    /// the top (0 = oldest visible). `follow_tail` overrides this and
    /// pins the view to the bottom so new streaming tokens stay in
    /// view; this value is what the user lands on when they PageUp.
    pub scroll: u16,
    /// When true, the chat always shows the bottom-most line, even as
    /// new tokens stream in. PageUp sets this to false; the draw step
    /// flips it back to true once the user has manually scrolled back
    /// to the bottom.
    pub follow_tail: bool,
    pub streaming: bool,
    pub stream_buf: String,
    // stream cancellation: cancel flag for the forwarder task,
    // epoch increments per stream so stale events get filtered out
    pub stream_cancel: Option<Arc<AtomicBool>>,
    pub stream_epoch: u64,

    // token usage from last completed stream
    pub last_usage: Option<TokenUsage>,

    // <think> stripping state, reset on each new stream
    pub think_state: ThinkState,
    pub think_pending: String,

    // slash command context menu: index into the filtered command list
    pub menu_idx: usize,

    // spinner frame counter, advances on each Tick event so animations
    // stay in sync regardless of how busy the main loop is
    pub spinner_frame: usize,

    // /search state. `query` is the active pattern (lowercased on
    // compare), `matches` holds the message indices that contain it,
    // `idx` is the current match for n/N navigation.
    pub search_query: Option<String>,
    pub search_matches: Vec<usize>,
    pub search_idx: usize,

    // -- Multi-session state --
    /// All known sessions, newest updated_at first. Loaded from
    /// `~/.config/bobric/sessions/` at startup and refreshed on every
    /// create / save / delete.
    pub sessions: Vec<crate::sessions::SessionMeta>,
    /// Id of the session currently shown in the chat area.
    pub active_session_id: String,
    /// Index of the highlighted row in the left sessions panel.
    pub session_panel_idx: usize,
    /// True while the user is in session-nav mode (chat input disabled,
    /// sessions panel is the keyboard focus).
    pub session_nav_mode: bool,
    /// Pending-delete arming flag. First 'd' sets, second 'd' confirms.
    pub session_pending_delete: bool,
    /// True while the user is typing a new name for the highlighted
    /// session (after pressing 'r' in nav mode).
    pub session_renaming: bool,
    /// `Some` while a toast is on screen. (message, set_at).
    pub toast: Option<(String, std::time::Instant)>,

    /// Modal popup overlay (`:messages`, `:help`, etc.). `None` when
    /// no popup is open. When `Some`, the popup intercepts all keys
    /// and renders on top of every other widget.
    pub popup: Option<Popup>,
}

/// Catalogue of slash commands. Single source of truth for the context
/// menu, the `/help` output, and validation.
pub const COMMANDS: &[(&str, &str)] = &[
    ("/help", "show this command list"),
    ("/clear", "wipe chat + history file"),
    ("/copy", "copy last bot reply to clipboard (OSC52)"),
    ("/export", "save chat to file: /export <path>"),
    ("/search", "search chat: /search <keyword> (n=next, N=prev)"),
    ("/model", "switch model"),
    ("/setup", "open connection config"),
    ("/system", "edit system prompt"),
    ("/quit", "exit bobric"),
];

/// True when the slash command context menu should be drawn. Derives
/// from the current input rather than a separate flag, so the menu
/// closes automatically when the prefix no longer matches a command
/// (e.g. user backspaces past `/` or types an unknown char).
pub fn menu_is_open(app: &App) -> bool {
    if !app.input.starts_with('/') {
        return false;
    }
    COMMANDS.iter().any(|(c, _)| c.starts_with(app.input.as_str()))
}

/// Commands whose name starts with the current input prefix, in the
/// canonical `COMMANDS` order. Returns owned `String`s to avoid leaking
/// static lifetimes into callers.
pub fn filtered_commands(input: &str) -> Vec<String> {
    COMMANDS
        .iter()
        .map(|(c, _)| *c)
        .filter(|c| c.starts_with(input))
        .map(|c| c.to_string())
        .collect()
}

impl App {
    pub fn new(mut cfg: Config) -> Self {
        if cfg.system_prompt.trim().is_empty() {
            cfg.system_prompt = DEFAULT_SYSTEM_PROMPT.to_string();
        }
        let nick = if cfg.nick.is_empty() { "bob".to_string() } else { cfg.nick.clone() };
        let screen = if cfg.is_configured() {
            Screen::Chat
        } else {
            Screen::Setup
        };
        let mut me = Self {
            screen,
            cfg: cfg.clone(),
            cfg_dirty: false,
            setup_field: SetupField::BaseUrl,
            setup_base: cfg.base_url.clone(),
            setup_key: cfg.api_key.clone(),
            setup_nick: nick.clone(),
            setup_system: cfg.system_prompt.clone(),
            setup_status: None,
            models: Vec::new(),
            model_idx: 0,
            model_status: None,
            fetching: false,
            testing: false,
            messages: Vec::new(),
            input: String::new(),
            input_history: Vec::new(),
            history_idx: None,
            scroll: 0,
            follow_tail: true,
            streaming: false,
            stream_buf: String::new(),
            stream_cancel: None,
            stream_epoch: 0,
            last_usage: None,
            think_state: ThinkState::Normal,
            think_pending: String::new(),
            menu_idx: 0,
            spinner_frame: 0,
            search_query: None,
            search_matches: Vec::new(),
            search_idx: 0,
            sessions: Vec::new(),
            active_session_id: String::new(),
            session_panel_idx: 0,
            session_nav_mode: false,
            session_pending_delete: false,
            session_renaming: false,
            toast: None,
            popup: None,
        };
        if me.screen == Screen::Chat {
            me.messages.push(ChatMessage {
                role: Role::System,
                content: format!(
                    "Connected to {}. Type /help for commands.",
                    me.cfg.base_url
                ),
                time: now(),
            });
            if !me.cfg.model.is_empty() {
                me.messages.push(ChatMessage {
                    role: Role::System,
                    content: format!("Using model: {}", me.cfg.model),
                    time: now(),
                });
            }
        }
        me
    }
}

pub fn now() -> String {
    Local::now().format("%H:%M:%S").to_string()
}

#[derive(Debug, Clone)]
pub struct TokenUsage {
    pub prompt: u32,
    pub completion: u32,
    pub total: u32,
}

