# Architecture

## Module map

```
src/
|-- main.rs     # event loop, state machine wiring, async orchestration
|-- app.rs      # App state, ChatMessage, Role, Screen, ThinkState, Popup
|-- api.rs      # HTTP client (reqwest) -- list_models, test_connection, stream_chat
|-- config.rs   # load/save Config to ~/.config/bobirc/config.json
|-- history.rs  # per-session JSONL persistence (legacy, superseded by sessions.rs)
|-- sessions.rs # multi-session storage: SessionMeta, Session, save/load/delete
|-- osc52.rs    # OSC52 clipboard copy (pure-Rust base64, no external dep)
|-- ui.rs       # render: status bar, sessions panel, chat, input, popups, toasts
```

## State machine

```
                +---------+
                |  Setup  |  <-- /setup
                +----+----+
                     | Enter (save)
                     v
           +------------------+
           |  ModelSelect     | <-- Esc / /model
           +----+-------------+
                | Enter (pick)
                v
           +---------+
           |  Chat   | <-- /model
           +----+----+
                | Esc (back to ModelSelect)
                v
           (ModelSelect)
```

Startup behavior:

- If `~/.config/bobirc/config.json` exists and has `base_url` + `api_key` -> go straight to **Chat** (with `/model` to revisit picker).
- Otherwise -> **Setup**.
- Sessions are loaded from `~/.config/bobirc/sessions/` at startup; a "Default" session is created if the dir is empty.

## Event loop

```
+----------------------------------------------------------+
|  tokio::select! {                                        |
|      events.next()       -> AppEvent::Key               |
|      rx.recv()           -> AppEvent::Api               |
|                            AppEvent::Stream {epoch, ev} |
|      interval(80ms)       -> AppEvent::Tick             |
|  }                                                      |
|  -> draw -> repeat                                       |
+----------------------------------------------------------+
```

- `EventStream` (crossterm with `event-stream` feature) yields key events.
- API/stream tasks push results via an `mpsc::UnboundedSender<AppEvent>`.
- `AppEvent::Stream` carries the stream `epoch` so stale events from cancelled streams are dropped by `handle_stream`.
- 80ms tick drives redraws and the spinner frame counter (`app.spinner_frame`).

## Streaming pipeline

```
[Chat] Enter pressed
   v
do_stream() spawns tokio task
   |  - cancels any prior stream via Arc<AtomicBool>
   |  - bumps app.stream_epoch
   |  - builds message history from app.messages
   v
stream_chat() -> reqwest POST {base}/v1/chat/completions
   |  - stream_options.include_usage = true
   v
bytes_stream() -> manual SSE parser
   |  - splits on blank-line events
   |  - parses "data: {...}" lines
   |  - extracts delta.content, usage, [DONE]
   v
mpsc::Receiver<StreamEvent>  <- Delta | Done | Error | Usage
   v
Outer task forwards each event via tx.send(AppEvent::Stream { epoch, ev })
   |  - checks cancel flag before each send
   v
Main loop -> handle_stream(app, epoch, ev)
   |  - drops events where epoch != app.stream_epoch
   |  - Delta: strip_think_chunk() -> append to stream_buf
   |  - Done: push ChatMessage, save session, reset state
   |  - Usage: store TokenUsage in app.last_usage
   |  - Error: toast + reset
   v
draw() renders stream_buf as live bot message
```

## Think stripping (`<think>...</think>`)

```
Delta arrives
   v
strip_think_chunk(input, &mut think_state, &mut think_pending)
   |  ThinkState::Normal  -> look for `<think>`, flush safe prefix
   |  ThinkState::InThink -> look for `</think>`, drop content in between
   |  pending buffer holds last ~7 chars across chunk boundaries
   v
clean text -> stream_buf -> draw() shows spinner or live content
```

- If `<think>` arrives, thinking spinner activates (`SPINNER_FRAMES`).
- If `</think>` arrives, thinking ends, normal streaming resumes.
- If stream ends mid-think (unclosed tag), pending is cleared, no crash.
- Panic guard: `render_markdown` is wrapped in `catch_unwind`.

## Keybindings per screen

### Global (Chat screen, no menu/nav/search active)

| Key | Action |
|---|---|
| `F1` | Open :messages log popup |
| `F2` | Enter session nav mode |
| `Alt+1..9` | Quick switch session |
| `Tab` | Autocomplete slash command (when `/` typed) |
| `Enter` | Send / run command |
| `Shift+Enter` | Newline in input |
| `Esc` | Cancel stream / close popup / exit nav mode |
| `PageUp` / `PageDown` | Scroll chat |
| `Ctrl+C` | Quit |

### Slash menu (when input starts with `/`)

| Key | Action |
|---|---|
| `Up` / `Down` | Navigate command list |
| `Tab` | Complete highlighted command |
| `Esc` | Clear input, close menu |

### Session nav mode (`F2`)

| Key | Action |
|---|---|
| `Up` / `Down` | Navigate sessions panel |
| `Enter` | Switch to highlighted session |
| `n` | New session |
| `d` | Delete highlighted session (press twice to confirm) |
| `Esc` / `F2` | Exit nav mode |

### Search (when `/search` active)

| Key | Action |
|---|---|
| `n` | Next match |
| `N` | Previous match |
| `Esc` | Clear search |

### Popup (`/help`, `?` messages log)

| Key | Action |
|---|---|
| `Esc` / `Enter` / `q` | Close |
| `j` / `Down` | Scroll down 1 line |
| `k` / `Up` | Scroll up 1 line |
| `PageDown` / `PageUp` | Scroll page |
| `g` / `G` | Top / bottom |

## Slash commands

| Command | Alias | Action |
|---|---|---|
| `/help` | — | Open cheatsheet popup (commands + keys) |
| `/new` | `/newchat` | Create new chat session |
| `/sessions` | `/list` | List all sessions (popup) |
| `/switch <n\|name>` | — | Switch to session n (1-based) or by partial name |
| `/delete <n>` | `/del` | Delete session n |
| `/rename <name>` | — | Rename current session |
| `/clear` | — | Wipe current chat messages |
| `/copy` | — | Copy last bot reply to clipboard (OSC52) |
| `/export <path>` | — | Save current chat to file (markdown stripped) |
| `/search <kw>` | — | Search current session; `n`/`N` to jump |
| `/model` | — | Open model picker |
| `/setup` | — | Open connection config |
| `/system` | — | Edit system prompt |
| `/quit` | `/exit` | Exit |

## Layout

```
+-----------+-------------------------------------------+
|           |  top status bar (1 row)                   |
+-----------+-------------------------------------------+
|           |                                            |
| sessions  |  chat area (timestamps + nick + msg)      |
|  panel    |  continuation lines indented to align     |
| (24 cols) |  with content start                       |
|           |                                            |
+-----------+-------------------------------------------+
|  bottom input area (4 rows: border, prompt, cursor)   |
+--------------------------------------------------------+
```

- Sessions panel: left sidebar, 24 cols. Shows session list with active marker.
- Chat area: flex (remaining width). Pre-wrapped to `chat_width - 20` for alignment.
- Input: 4 rows (1 top border, 2 text lines, 1 bottom border).
- Popup: centered modal, ~60% x ~60%, cyan border, on top of everything.
- Toast: floating 3.5s notification in top-right corner.

## Color palette

Base color: `#CDDC2A` (Material Lime 500)

| Constant | RGB | Used for |
|---|---|---|
| `TITLE` / `BASE` | `#CDDC2A` | Borders, screen labels, bot nick |
| `BOT` | `#CDDC2A` | Bot nick in chat + user list |
| `YOU` | `#66BB6A` | User nick in chat |
| `SYSTEM` | `#FFB300` | System messages |
| `ERROR` | `#FF5252` | Error messages |
| `SEL` | `#E040FB` | Selection highlight (menus) |
| `DIM` | `#707080` | Timestamps, hints, dim text |
| `INPUT` | `#ECECEC` | Message text, input |
| `BG` | `#14141C` | Background |
| `BG_HI` | `#282818` | Code blocks, active fields |
| `BG_SEL` | `#2A2A32` | List selection background |
| `STREAMING` | `#FFB300` | Status bar streaming indicator |

## Adding a new feature

1. Add state to `App` in `app.rs`.
2. Add handler in `main.rs` `handle_*_key` + (if needed) a new `AppEvent` variant.
3. Render in `ui.rs` -- pick the right screen branch.
4. If async, spawn a task in `do_*` and forward results through `tx`.
5. If the feature affects chat rendering, update `render_message` or `render_markdown`.
6. If it's a slash command, add to `COMMANDS` in `app.rs` and `handle_command` in `main.rs`.
7. If it's transient feedback (confirmation, error), use `set_toast()` instead of pushing to `app.messages`.

## Known limitations / future work

- No message edit / delete
- No file/image attachments
- Cross-session search (`/searchall`) not yet implemented
- Rename via inline `r` in session nav mode (only via `/rename` slash command)
- Word-wrap indent only applies to explicit `\n`; Paragraph wrap continuations may be slightly misaligned in very narrow terminals
