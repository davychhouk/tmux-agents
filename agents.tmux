#!/usr/bin/env bash
dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
bin="$dir/target/release/tmux-agents"

if [ ! -x "$bin" ]; then
  (cd "$dir" && cargo build --release) || exit 1
fi

key="$(tmux show-option -gqv @agents-key)"
tmux bind-key "${key:-a}" run-shell "$bin toggle"
