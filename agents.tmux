#!/usr/bin/env bash
set -u

dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo="davychhouk/tmux-agents"
version="$(tmux show-option -gqv @agents-version)"
version="${version:-v0.1.0}"
bin="$dir/bin/tmux-agents"

fail() {
  tmux display-message "tmux-agents: $*" 2>/dev/null || printf 'tmux-agents: %s\n' "$*" >&2
  exit 1
}

target() {
  case "$(uname -s):$(uname -m)" in
    Darwin:arm64 | Darwin:aarch64) printf 'aarch64-apple-darwin' ;;
    Darwin:x86_64) printf 'x86_64-apple-darwin' ;;
    Linux:arm64 | Linux:aarch64) printf 'aarch64-unknown-linux-gnu' ;;
    Linux:x86_64 | Linux:amd64) printf 'x86_64-unknown-linux-gnu' ;;
    *) return 1 ;;
  esac
}

dl() {
  if command -v curl >/dev/null 2>&1; then
    curl -fsSL --max-time 30 "$1" -o "$2"
  elif command -v wget >/dev/null 2>&1; then
    wget -q -T 30 -O "$2" "$1"
  else
    return 1
  fi
}

checksum() {
  if command -v shasum >/dev/null 2>&1; then
    shasum -a 256 -c "$1"
  elif command -v sha256sum >/dev/null 2>&1; then
    sha256sum -c "$1"
  else
    return 1
  fi
}

# Every failure returns 1 so the caller can fall back to a local cargo build.
fetch() {
  target="$(target)" || return 1
  asset="tmux-agents-$target.tar.gz"
  url="https://github.com/$repo/releases/download/$version/$asset"
  tmp="$(mktemp -d "${TMPDIR:-/tmp}/tmux-agents.XXXXXX")" || return 1
  trap 'rm -rf "$tmp"' EXIT

  dl "$url" "$tmp/$asset" || return 1
  dl "$url.sha256" "$tmp/$asset.sha256" || return 1
  (cd "$tmp" && checksum "$asset.sha256") || return 1
  tar -xzf "$tmp/$asset" -C "$tmp" || return 1
  cp "$tmp/tmux-agents" "$bin" || return 1
  chmod 755 "$bin"
}

installed="$(cat "$dir/bin/.version" 2>/dev/null)"
if [ ! -x "$bin" ] || [ "$installed" != "$version" ]; then
  mkdir -p "$dir/bin" || fail "could not create bin directory"
  if ! fetch; then
    command -v cargo >/dev/null 2>&1 ||
      fail "no prebuilt binary for $version; install Rust or set @agents-version to an available release"
    (cd "$dir" && cargo build --release) || fail "cargo build failed"
    cp "$dir/target/release/tmux-agents" "$bin" || fail "could not install binary"
  fi
  printf '%s\n' "$version" >"$dir/bin/.version"
fi

key="$(tmux show-option -gqv @agents-key)"
tmux bind-key "${key:-a}" run-shell "$bin toggle"

# Sidebar follows the active window across window/session switches.
# Array index keeps user hooks at other indices intact.
tmux set-hook -g "session-window-changed[8]" "run-shell -b '$bin follow'"
tmux set-hook -g "client-session-changed[8]" "run-shell -b '$bin follow'"
