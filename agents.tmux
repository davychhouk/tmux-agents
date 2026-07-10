#!/usr/bin/env bash
dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
bin="$dir/target/release/tmux-agents"

if [ ! -x "$bin" ]; then
  (cd "$dir" && cargo build --release) || exit 1
fi

key="$(tmux show-option -gqv @agents-key)"
tmux bind-key "${key:-a}" run-shell "$bin toggle"

# Sidebar follows the active window across window/session switches.
# Array index keeps user hooks at other indices intact.
tmux set-hook -g "session-window-changed[8]" "run-shell -b '$bin follow'"
tmux set-hook -g "client-session-changed[8]" "run-shell -b '$bin follow'"
