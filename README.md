# BobRIC

A mIRC-style TUI chat client for any OpenAI-compatible LLM API. Built in Rust with [ratatui](https://ratatui.rs).

## Run

```bash
cargo run --release
```

First launch goes to the **Setup** screen. Tab between fields, Enter to save.

Config persists at `~/.config/bobric/config.json`:

```json
{
  "base_url": "https://api.openai.com",
  "api_key": "sk-...",
  "model": "gpt-4o-mini",
  "nick": "bob"
}
```

## Keys

| Key | Action |
|---|---|
| `Tab` / `BackTab` | Switch field (setup) |
| `Enter` | Submit / send |
| `Esc` | Back / cancel stream |
| `F` | Fetch models |
| `T` | Test connection |
| `Up` / `Down` | Navigate / recall history |
| `PageUp` / `PageDown` | Scroll chat |
| `Ctrl-C` | Quit |

## Slash commands (in chat)

- `/help` — show command list
- `/clear` — clear messages
- `/model` — reopen model picker
- `/setup` — reopen config
- `/quit` — exit

## Compatible APIs

Anything that speaks OpenAI chat completions + `/v1/models`:

- **OpenAI** — `https://api.openai.com/v1`
- **OpenRouter** — `https://openrouter.ai/api/v1`
- **Ollama** — `http://localhost:11434/v1`
- **LM Studio** — `http://localhost:1234/v1`
- **vLLM** — `http://localhost:8000/v1`
- **any proxied endpoint**

The base URL can end with or without `/v1` — both are normalized.

## Architecture

See [ARCHITECTURE.md](ARCHITECTURE.md) for module map, state machine, event flow, and streaming pipeline.

See [CHANGELOG.md](CHANGELOG.md) for release notes.

## Build

```bash
cargo build --release
```

Output binary: `target/release/bobric`

## License

MIT.
