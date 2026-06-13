use crate::api::Model;
use crate::config::Config;
use chrono::Local;

#[derive(Debug, Clone)]
pub struct ChatMessage {
    pub role: Role,
    pub content: String,
    pub time: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Role {
    System,
    You,
    Bot,
    Error,
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
    pub scroll: u16,
    pub streaming: bool,
    pub stream_buf: String,
}

impl App {
    pub fn new(cfg: Config) -> Self {
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
            streaming: false,
            stream_buf: String::new(),
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

