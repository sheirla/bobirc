# Changelog

## 0.2.0 -- 2026-06-14

Features and bugfix pass.

**Bug fixes**

- Streaming cancel: `Esc` during an active stream used to set only a local
  flag, but the spawned forwarder task kept consuming tokens and would push
  the full buffer as a bot message on `Done`. Fix: `Arc<AtomicBool>` cancel
  flag + `u64` epoch on the `App` side; `AppEvent::Stream` now carries the
  epoch so the main loop drops stale events. Inner SSE task also exits
  early when the receiver is dropped, which closes the HTTP stream.

**Features**

- **Persistent message history** -- `~/.config/bobric/history.jsonl`.
  Loaded on startup, appended on every user/bot message, cleared by
  `/clear`.
- **System prompt** -- new field in `Config` and a 4th row in the setup
  screen (`SetupField::SystemPrompt`). Tab cycles through it; multi-line
  editing with `Shift+Enter`. Sent as the first message in the chat
  completion request when non-empty.
- **Multi-line input** -- `Shift+Enter` inserts a newline in the chat
  input. The input area is now 5 rows tall; text wraps and the caret
  tracks the end of the last line.
- **Token usage** -- request now sets `stream_options.include_usage`;
  the SSE `usage` chunk is parsed and stored in `App::last_usage`.
  Surfaced in the status bar as `in:N out:N tot:N`.
- **Auto-refresh on `/model`** -- entering the model picker via `/model`
  from chat now automatically fires `F` if the model list is empty.
- **`/system` command** -- jumps to the setup screen and focuses the
  system prompt field.
- **Color palette refresh** -- base color `#CDDC2A` (Material Lime 500)
  for borders, screen label, bot nick, and selection accents. YOU
  recolored to a contrasting green; dim/amber/red/magenta/background
  tuned for legibility on the dark canvas.

**Other**

- `AppEvent::Stream` now carries the stream `epoch` alongside the
  `StreamEvent` payload.
- Inner SSE task in `api.rs` exits as soon as a `send` fails, so
  cancellation propagates all the way back to the HTTP response.
- New module `src/history.rs` for persistence helpers.
- `Role` and `ChatMessage` now `Serialize`/`Deserialize` for the
  history file.

## 0.1.0 -- 2026-06-14

Initial release.

**Features**

- mIRC-style TUI with top status bar, channel area, right-side user list, bottom input
- OpenAI-compatible API client (list models, test connection, streaming chat completions)
- Three-screen state machine: Setup -> ModelSelect -> Chat
- Config persisted to `~/.config/bobric/config.json`
- Slash commands: `/help`, `/clear`, `/model`, `/setup`, `/quit`
- Input history recall (Up/Down)
- Async event loop with tokio + crossterm EventStream
- Manual SSE parser for streaming responses
- Base URL normalization (accepts with or without `/v1` suffix)

**Stack**

- Rust 2021, edition 1.86
- ratatui 0.28, crossterm 0.28
- reqwest 0.12 (rustls-tls), tokio 1
- serde, serde_json, anyhow, dirs, chrono, futures-util
