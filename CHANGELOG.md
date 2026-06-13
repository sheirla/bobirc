# Changelog

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
