# tmux-agents

Sidebar showing the status of AI coding agents (claude, codex) across all
tmux sessions. Working / blocked / done / idle, color-coded. Jump to any agent's
pane. Ratatui, single binary, no daemon.

## Install (TPM)

    set -g @plugin 'dvcrn/tmux-agents'

Needs a Rust toolchain on first run (builds the binary). tmux ≥ 3.0.

## Usage

`prefix a` toggles the sidebar. In the default `collapsed` mode it shrinks
to an icon-only strip instead of closing; set `@agents-toggle 'hidden'` to
close it entirely.

| key     | action               |
| ------- | -------------------- |
| `j`/`k` | move selection       |
| `Enter` | jump to agent's pane |
| `q`     | close                |

## Options

    set -g @agents-key 'a'                # prefix-table key
    set -g @agents-toggle 'collapsed'     # collapsed | hidden
    set -g @agents-follow 'on'            # sidebar follows window/session switches
    set -g @agents-position 'right'       # left | right
    set -g @agents-width '32'             # integer columns (no %)
    set -g @agents-collapsed-width '2'    # default: 3 on left, 2 on right; keep < 12
    set -g @agents-interval '1'           # poll seconds

Widths are integer column counts; percentages are not supported. The sidebar
renders icon-only below 12 columns, so keep the collapsed width under 12.

## How it works

Polls `tmux list-panes -a`, walks each pane's process tree for claude/codex
(including inside nvim terminals), and classifies `capture-pane` output into
working / blocked / idle. An agent that goes from working to idle shows as
done (green) until it starts working again.
