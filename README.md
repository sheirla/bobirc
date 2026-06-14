# Bobric

A mIRC-style TUI chat client for any OpenAI-compatible LLM API. Built
in Rust with [ratatui](https://ratatui.rs).

> **Note**: the "Boblabs" name in the empty-state ASCII art is a
> branding reference only. The package, binary, and config dir are
> all named `bobric`.

## Install

### One-liner

macOS / Linux (bash):

```bash
curl -sSf https://raw.githubusercontent.com/sheirla/bobric/main/install.sh | sh
```

Windows (PowerShell):

```powershell
iwr -useb https://raw.githubusercontent.com/sheirla/bobric/main/install.ps1 | iex
```

The installer:

1. Installs the Rust toolchain via [rustup](https://rustup.rs) if
   it's not already present
2. Builds and installs `bobric` via `cargo install --git ...`
3. Drops the binary at `~/.cargo/bin/bobric` (or
   `%USERPROFILE%\.cargo\bin\bobric.exe` on Windows)
4. Prints a hint to add `~/.cargo/bin` to your `PATH` if it isn't
   already

To pin a specific version (git tag):

```bash
BOBRIC_VERSION=v0.3.0 curl -sSf ... | sh
```

### From source

```bash
git clone https://github.com/sheirla/bobric
cd bobric
cargo install --path .
```

### Prebuilt binaries

Check the [Releases page](https://github.com/sheirla/bobric/releases)
for prebuilt binaries (Linux x86_64, macOS universal, Windows
x86_64). Download, extract, and put `bobric` on your `PATH`.

## Run

```bash
bobric
```

First launch goes to the **Setup** screen. Tab between fields, Enter
to save. Then pick a model on the next screen (F to fetch, T to
test). After that you're in the chat.

To re-run setup later: `/setup`. To re-pick a model: `/model`.

## Features

- **mIRC-style TUI** — sessions sidebar (left), chat area, input
  box, status bar, toast overlays
- **Multi-session** — JSON-per-session storage at
  `~/.config/bobric/sessions/<id>.json`. Switch between them
  via `F2` (nav mode), `Alt+1..9` (quick switch), or
  `/switch <n|name>`. Sessions auto-name from the first user
  message.
- **OpenAI-compatible** — works with OpenAI, OpenRouter, Ollama,
  LM Studio, vLLM, or any proxy that speaks the chat completions +
  `/v1/models` API.
- **Markdown rendering** — bold, italic, inline code, code blocks,
  lists, headings, blockquotes
- **Thinking animation** for models that emit `<think>...</think>`
  blocks (DeepSeek-R1, etc.) — content stripped from the visible
  stream, replaced with an animated indicator
- **Search** with nvim-style `n`/`N` navigation and inline
  highlight of the current match
- **Slash commands** for session management, config, copy, export,
  search, etc. (see table below)
- **OSC52 clipboard** copy via `/copy` — works in kitty, wezterm,
  alacritty >= 0.13, foot, iTerm2, Windows Terminal, modern
  gnome-terminal
- **Terminal-native text selection** — no mouse capture, so you can
  drag-select chat output in the alternate screen and copy with
  your terminal's native keybind
- **Persistent history** across restarts

## Slash commands

| Command | Description |
|---|---|
| `/help` | Open command + key cheatsheet popup |
| `/new` | Start a new chat session |
| `/sessions` | List all sessions (popup) |
| `/switch <n\|name>` | Switch to session n (1-based) or by partial name |
| `/delete <n>` | Delete session n |
| `/rename <name>` | Rename current session |
| `/clear` | Wipe current chat |
| `/copy` | Copy last bot reply to clipboard (OSC52) |
| `/export <path>` | Save current chat to file (markdown stripped) |
| `/search <kw>` | Search current session; `n`/`N` to jump |
| `/model` | Open model picker |
| `/setup` | Open connection config |
| `/system` | Edit system prompt |
| `/quit` | Exit |

## Keys

| Key | Action |
|---|---|
| `F2` | Enter session nav mode (Up/Down/Enter/n/d/Esc) |
| `Alt+1..9` | Quick switch session |
| `?` | Open :messages log popup |
| `Tab` | Autocomplete slash command (when input starts with `/`) |
| `Enter` | Send / run command |
| `Shift+Enter` | Newline in input |
| `Esc` | Cancel stream · close popup · exit nav mode |
| `n` / `N` | Next / prev search match (when `/search` active) |
| `j` / `k` | Popup scroll down / up |
| `PageDown` / `PageUp` | Popup scroll page |
| `g` / `G` | Popup top / bottom |
| `Up` / `Down` | Input history recall |
| `PageUp` / `PageDown` | Chat scroll |
| `q` / `Enter` / `Esc` | Close popup |
| `Ctrl+C` | Quit |

## Configuration

Config at `~/.config/bobric/config.json`:

```json
{
  "base_url": "https://api.openai.com",
  "api_key": "sk-...",
  "model": "gpt-4o-mini",
  "nick": "bob",
  "system_prompt": "..."
}
```

Setup on first launch, or via `/setup` and `/model`. The
`system_prompt` field is per-session (each new session inherits the
default but can be edited inline via `/system`).

## Compatible APIs

Anything that speaks OpenAI chat completions + `/v1/models`:

- **OpenAI** — `https://api.openai.com/v1`
- **OpenRouter** — `https://openrouter.ai/api/v1`
- **Ollama** — `http://localhost:11434/v1`
- **LM Studio** — `http://localhost:1234/v1`
- **vLLM** — `http://localhost:8000/v1`
- **any proxied endpoint**

The `base_url` can end with or without `/v1` — both are normalised.

## Building

```bash
cargo build --release
```

Output binary at `target/release/bobric`. Run `cargo install --path .`
to install to `~/.cargo/bin/bobric`.

## License

MIT.
