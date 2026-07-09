# tmux-agents: agent-status sidebar plugin

2026-07-09. Approved design.

## Goal

tmux plugin showing status of AI coding agents (claude, codex) across all tmux
sessions, herdr-style, in a Ratatui sidebar pane. Toggleable, left/right,
expand/collapse, jump-to-pane.

## Architecture

Single Rust binary + thin TPM shim. No daemon, no IPC, no state files.

- `agents.tmux` — TPM entry script. Reads `@agents-*` options, binds toggle key
  in prefix table.
- `tmux-agents` — Rust binary (ratatui + crossterm).
  - `tmux-agents toggle` — if a sidebar pane exists in current window, kill it;
    else `split-window -fh -l <width>` (`-b` when position=left) running
    `tmux-agents`. Sidebar pane identified by pane user option
    `@agents_sidebar=1`, set at spawn.
  - `tmux-agents` (no args) — the TUI.

## Config (tmux options)

| option                    | default | meaning                        |
|---------------------------|---------|--------------------------------|
| `@agents-position`        | `right` | `left` or `right`              |
| `@agents-width`           | `32`    | expanded width (cols)          |
| `@agents-collapsed-width` | `6`     | collapsed width (cols)         |
| `@agents-key`             | `a`     | prefix-table toggle key        |
| `@agents-interval`        | `1`     | poll interval (seconds)        |

`@agents-key` is bound in the prefix table (default toggle: `prefix a`).

## Detection loop (every tick)

1. `tmux list-panes -a -F ...` — all panes: pane id, pid, session name, window
   index/name, active flags.
2. One `ps -eo pid,ppid,command` snapshot; walk descendants of each pane pid;
   pane is an agent pane if `claude` or `codex` appears anywhere in its process
   tree (covers agents nested inside nvim terminals).
3. For each agent pane: `tmux capture-pane -p -t <pane> -S -40` and classify
   last lines:
   - **blocked** — approval/permission prompt patterns (e.g. "Do you want",
     numbered yes/no menus, codex approval prompts)
   - **working** — braille spinner chars, "esc to interrupt", thinking
     indicators
   - **idle** — none of the above
4. Classifier regexes live in one module with unit tests against captured real
   claude/codex output fixtures.

## UI

Expanded:
- Agents grouped by session: state icon + agent name + window name.
- Colors: working yellow, blocked red bold, idle dim.
- Keys: `j`/`k` select, `Enter` jump (`switch-client -t` +
  `select-window`/`select-pane`), `Tab` collapse/expand (resizes own pane via
  `tmux resize-pane`), `q` quit.

Collapsed: one state icon per agent, same colors, no text.

## Errors

- tmux server gone / command fails → clean exit.
- Pane disappears between list and capture → drop it this tick.
- Unrecognized output → idle.

## Testing

- Unit: classifier regexes vs fixtures; process-tree matcher.
- Manual: run inside tmux with live claude/codex panes.

## Out of scope (v1)

- "done" state (needs working→idle transition history) — add if missed.
- Mouse support, custom detection regexes via config, status-line integration,
  agents beyond claude/codex.
