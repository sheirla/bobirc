# Changelog

## 0.3.0 -- 2026-06-15

Multi-session, popup system, markdown, search, polish.

**Multi-session**

- Per-session JSON storage at `~/.config/bobric/sessions/<id>.json`
  (one file per session, plus a `sessions` list cached in `App`).
  Loaded on startup, saved on every message push.
- Sessions panel on the **left** (replaces the right-side "Users"
  panel, which is gone). Each row shows the session name + a
  `●`/`○` active marker + the `updated_at` timestamp.
- Session nav mode toggled by `F2`: while on, `Up`/`Down` navigate
  the panel, `Enter` switches, `n` creates a new session, `d` is
  an arming-delete (press twice to confirm), `Esc`/`F2` exit.
- `Alt+1..9` quick-switches to the nth session from anywhere
  (not just in nav mode).
- Slash-command equivalents for everything: `/new`, `/sessions`,
  `/switch <n|name>`, `/delete <n>`, `/rename <newname>`.
- Auto-name: the first user message of a fresh session is used to
  derive the session name (truncated to 30 chars at a word
  boundary, via `sessions::auto_name`).

**Popups**

- Generic `Popup { id, title, body, scroll }` widget. `open_popup`
  preserves the scroll offset if the same `id` is re-opened.
- `/help` opens a cheatsheet popup with the full command list
  plus a "Keys" section.
- `?` opens a `:messages`-style log of recent `Role::System` +
  `Role::Error` lines from the chat history.
- Standard popup keys: `Esc`/`Enter`/`q` close, `j`/`k` or arrow
  keys scroll one line, `PageDown`/`PageUp` scroll a page,
  `g`/`G` jump to top/bottom.

**Toasts**

- Floating 3.5s notification in the top-right corner for
  transient feedback (replace ad-hoc chat messages):
  - Search match count (`Match 1/3 for "foo"`)
  - Session switches / new / delete confirmations
  - `/copy`, `/export`, `/rename`, `/delete` outcomes
  - Errors: model missing, stream error, export failed,
    session save failed, OSC52 not supported, etc.

**Markdown rendering**

- `pulldown-cmark` parser in `ui::render_markdown`; every chat
  user/bot message runs through it and produces styled `Line`s.
  Bold/italic/inline code/code blocks/headings/lists/blockquotes
  supported. The whole render is wrapped in
  `catch_unwind` as a panic guard so a malformed partial stream
  can't kill the TUI.

**Search**

- `/search <keyword>` filters the current session's messages
  case-insensitively. `n` / `N` cycle prev/next. The current
  match is highlighted inline (amber background, black bold) in
  the chat area. `Esc` clears the search. Match info (count,
  hints) lives in a toast, not in the chat.

**Chat hygiene**

- Audit: the only `app.messages.push` calls left are for actual
  user and bot messages. All status / error / hint messages were
  migrated to toast or popup, so the chat is strictly
  user <-> agent content.

**Input / UX**

- Streaming-cancel order swapped: `PageUp` now scrolls up
  (toward older) and `PageDown` scrolls down (toward newer).
  Previously inverted.
- `follow_tail` flag pins the chat to the bottom during streaming;
  `PageUp` disengages it.
- Caret position in the input box accounts for wrap (uses
  `wrap_pos` to mirror Paragraph's wrap).
- Terminal-native text selection is enabled (no `EnableMouseCapture`),
  so you can drag-select chat output in the alternate screen.
  Mouse wheel is **not** captured (scroll the chat with
  `PageUp`/`PageDown` or the terminal's native scrollback).
- Default `system_prompt` is the in-tree 10XTHINK preset unless
  the user has set their own.

**Polish / refactor**

- OSC52 clipboard module is pure-Rust (no new dep).
- "Boblabs" name in the empty-state ASCII art is the only place
  the brand word appears. Package, binary, config dir are all
  `bobric`.
- /clear now wipes the in-memory chat (history file is left in
  place for the active session; each session has its own).
- `Config::default` no longer pre-fills a nick; the setup screen
  still defaults to "bob".

**Installers**

- `install.sh` (macOS / Linux bash) and `install.ps1` (Windows
  PowerShell) one-liner installers. Both detect the Rust
  toolchain, install it via `rustup` if missing, then
  `cargo install --git ... --locked`. Set `BOBRIC_VERSION=vX.Y.Z`
  to pin a specific tag.

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
