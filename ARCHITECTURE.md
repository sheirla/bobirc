# Architecture

## Module map

\`\`\`
src/
|-- main.rs   # event loop, state machine wiring, async orchestration
|-- app.rs    # App state, ChatMessage, Role, Screen enum
|-- api.rs    # HTTP client (reqwest) -- list_models, test_connection, stream_chat
|-- config.rs # load/save Config to ~/.config/bobric/config.json
\`\`\`

## State machine

\`\`\`
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
\`\`\`

Startup behavior:

- If \`~/.config/bobric/config.json\` exists and has \`base_url\` + \`api_key\` -> go straight to **Chat** (with \`/model\` to revisit picker).
- Otherwise -> **Setup**.

## Event loop

\`\`\`
+----------------------------------------------+
|  tokio::select! {                            |
|      events.next()       -> AppEvent::Key    |
|      rx.recv()           -> AppEvent::Api    |
|                            AppEvent::Stream  |
|      interval(80ms)       -> AppEvent::Tick  |
|  }                                          |
|  -> draw -> repeat                           |
+----------------------------------------------+
\`\`\`

- \`EventStream\` (crossterm with \`event-stream\` feature) yields key events.
- API/stream tasks push results via an \`mpsc::UnboundedSender<AppEvent>\`.
- 80ms tick drives redraws (for streaming deltas, "typing" indicators).

## Streaming pipeline

\`\`\`
[Chat] Enter pressed
   v
do_stream() spawns tokio task
   v
stream_chat() -> reqwest POST {base}/v1/chat/completions
   v
bytes_stream() -> manual SSE parser
   |  - splits on blank-line events
   |  - parses "data: {...}" lines
   |  - extracts delta.content
   v
mpsc::Receiver<StreamEvent>          <- Delta | Done | Error
   v
Outer task forwards each event via tx.send(AppEvent::Stream(_))
   v
Main loop -> handle_stream() appends to stream_buf, redraws on tick
   v
On StreamEvent::Done -> push ChatMessage, clear stream_buf, set streaming=false
\`\`\`

## Keybindings per screen

### Setup

- \`Tab\` / \`BackTab\` -- cycle field
- \`Backspace\` -- delete
- \`Enter\` -- save and advance
- \`Esc\` -- quit (or back to chat if already configured)

### ModelSelect

- \`Up\` / \`Down\` -- navigate list
- \`PgUp\` / \`PgDn\` -- page
- \`F\` -- fetch models (calls \`GET {base}/v1/models\`)
- \`T\` -- test connection (calls same, reports HTTP status)
- \`Enter\` -- select and advance to chat
- \`Esc\` -- back to setup

### Chat

- \`Enter\` -- send message
- \`Backspace\` -- delete char
- \`Up\` / \`Down\` -- recall input history
- \`PageUp\` / \`PageDown\` -- scroll message area
- \`Esc\` -- interrupt active stream / back to model picker

## Layout (mIRC vibe)

\`\`\`
+-------------------------------------------------------------+
|  top status bar (1 row)                                     |
+----------------------------------------------+--------------+
|                                              |              |
|  main channel area (timestamps + nick + msg) |  user list   |
|                                              |  (right col) |
|                                              |              |
+----------------------------------------------+--------------+
|  bottom input area (3 rows: border, prompt, cursor)         |
+-------------------------------------------------------------+
\`\`\`

Color palette:

- Cyan -- borders, bot nick, screen label
- Green -- your nick
- Yellow -- system messages
- Red -- errors
- Dark gray -- timestamps
- White -- message text

## Adding a new feature

1. Add state to \`App\` in \`app.rs\`.
2. Add handler in \`main.rs\` \`handle_*_key\` + (if needed) a new \`AppEvent\` variant.
3. Render in \`ui.rs\` -- pick the right screen branch.
4. If async, spawn a task in \`do_*\` and forward results through \`tx\`.

## Known limitations / future work

- No multi-line input (single-line prompt only)
- No message edit / delete
- No model reload on \`/model\` (must press F again)
- No token usage display
- No system prompt UI (history-only)
- No file/image attachments
- No persistent message history (in-memory only)
