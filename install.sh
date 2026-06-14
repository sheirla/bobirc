#!/usr/bin/env bash
# Bobric installer (macOS / Linux)
#
# Usage:
#   curl -sSf https://raw.githubusercontent.com/sheirla/bobric/main/install.sh | sh
#
# Env vars:
#   BOBRIC_VERSION   pin a specific git tag/branch (default: main)
#   BOBRIC_REPO      override the git URL (default: github.com/sheirla/bobric)
#
set -euo pipefail

REPO_URL="${BOBRIC_REPO:-https://github.com/sheirla/bobric}"
VERSION="${BOBRIC_VERSION:-main}"
BINARY="bobric"

say()  { printf '\033[1;32m==>\033[0m %s\n' "$*"; }
warn() { printf '\033[1;33m[warn]\033[0m %s\n' "$*" >&2; }
die()  { printf '\033[1;31m[err]\033[0m %s\n' "$*" >&2; exit 1; }

say "bobric installer (target: $REPO_URL @ $VERSION)"

# 1. Ensure Rust toolchain
if ! command -v cargo >/dev/null 2>&1; then
    say "Rust not found -- installing via rustup"
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain stable --profile minimal
    # shellcheck disable=SC1091
    source "$HOME/.cargo/env"
    command -v cargo >/dev/null 2>&1 || die "cargo not on PATH after rustup install"
fi

say "Rust: $(rustc --version)  cargo: $(cargo --version)"

# 2. Build + install bobric
say "Building and installing '$BINARY' (this can take a few minutes on first run)..."
if [ "$VERSION" = "main" ] || [ -z "$VERSION" ]; then
    cargo install --git "$REPO_URL" --locked
else
    cargo install --git "$REPO_URL" --tag "$VERSION" --locked
fi

# 3. PATH hint
CARGO_BIN="${CARGO_HOME:-$HOME/.cargo}/bin"
case ":$PATH:" in
    *":$CARGO_BIN:"*) ;;
    *)
        warn "$CARGO_BIN is not on your PATH"
        case "${SHELL:-/bin/sh}" in
            *zsh)  echo "  echo 'export PATH=\"$CARGO_BIN:\$PATH\"' >> ~/.zshrc" ;;
            *bash) echo "  echo 'export PATH=\"$CARGO_BIN:\$PATH\"' >> ~/.bashrc" ;;
            *fish) echo "  fish_add_path $CARGO_BIN" ;;
            *)     echo "  export PATH=\"$CARGO_BIN:\$PATH\"   # add to your shell rc" ;;
        esac
        ;;
esac

say "Done! Run: $BINARY"
