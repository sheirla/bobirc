#!/usr/bin/env bash
# Bobric uninstaller (macOS / Linux)
#
# Removes the binary AND all per-user data so the machine is left
# clean. Confirm-before-delete unless --yes is passed.
#
# Usage:
#   curl -sSf https://raw.githubusercontent.com/sheirla/bobirc/main/uninstall.sh | sh
#   curl -sSf ... | sh -s -- --yes         # skip confirmation
#
set -euo pipefail

DATA_DIR="${XDG_CONFIG_HOME:-$HOME/.config}/bobirc"
CARGO_BIN="${CARGO_HOME:-$HOME/.cargo}/bin/bobirc"

say()  { printf '\033[1;32m==>\033[0m %s\n' "$*"; }
warn() { printf '\033[1;33m[warn]\033[0m %s\n' "$*" >&2; }
die()  { printf '\033[1;31m[err]\033[0m %s\n' "$*" >&2; exit 1; }

if [ "${1:-}" != "--yes" ]; then
    echo "This will REMOVE:"
    [ -e "$DATA_DIR" ] && echo "  - $DATA_DIR  (config + sessions + history)"
    [ -e "$CARGO_BIN" ]  && echo "  - $CARGO_BIN"
    if [ ! -e "$DATA_DIR" ] && [ ! -e "$CARGO_BIN" ]; then
        echo "  (nothing found to remove -- bobirc may already be uninstalled)"
        exit 0
    fi
    echo ""
    read -r -p "Continue? [y/N] " ans
    case "$ans" in
        y|Y|yes) ;;
        *) echo "Aborted."; exit 0 ;;
    esac
fi

removed=0
if [ -e "$DATA_DIR" ]; then
    rm -rf "$DATA_DIR"
    say "removed $DATA_DIR"
    removed=1
fi
if [ -e "$CARGO_BIN" ]; then
    rm -f "$CARGO_BIN"
    say "removed $CARGO_BIN"
    removed=1
fi

# Also remove cargo's installed-package registry entry, so a future
# `cargo install bobirc` starts clean. Best-effort.
if command -v cargo >/dev/null 2>&1; then
    cargo uninstall bobirc 2>/dev/null && say "removed cargo package registry entry" || true
fi

if [ "$removed" -eq 0 ]; then
    warn "nothing to remove"
fi
echo ""
say "Done. bobirc is fully uninstalled."
say "Reinstall: curl -sSf https://raw.githubusercontent.com/sheirla/bobirc/main/install.sh | sh"
