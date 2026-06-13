# BobRIC

mIRC-style TUI chat client for any OpenAI-compatible LLM API.

## Run

```
cargo run
```

First launch goes to **Setup** screen — Tab between Base URL / API Key / Nick, Enter to save. Then **Model Select** — `F` to fetch, `T` to test, `↑↓` to navigate, Enter to pick. Then **Chat**.

Config persists at `~/.config/bobric/config.json`.

## Keys

- `Tab` / `BackTab` — switch field (setup)
- `Enter` — submit / send
- `Esc` — back / cancel stream
- `F` — fetch models
- `T` — test connection
- `↑` / `↓` — navigate / recall history
- `PageUp` / `PageDown` — scroll
- `Ctrl-C` — quit

## Slash commands (in chat)

- `/help` — show commands
- `/clear` — clear messages
- `/model` — reopen model picker
- `/setup` — reopen config
- `/quit` — exit

## Compatible APIs

Anything that speaks OpenAI chat completions + `/v1/models`. Tested format:
- OpenAI (`https://api.openai.com/v1`)
- OpenRouter (`https://openrouter.ai/api/v1`)
- Ollama (`http://localhost:11434/v1`)
- LM Studio (`http://localhost:1234/v1`)
