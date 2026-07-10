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
(including inside nvim terminals), and classifies each pane into
working / blocked / idle. An agent that goes from working to idle shows as
done (green) until it starts working again.

Per pane, signals are consulted in order:

1. **`@agent_state` pane option** — ground truth, pushed by the agent's own
   lifecycle hooks (see below). A live dialog on screen still overrides a
   non-blocked value, since some dialogs emit no hook event.
2. **Pane title (OSC)** — a leading braille spinner frame means working;
   codex titles a pending approval "Action Required".
3. **Screen scraping** — `capture-pane` tail matched against live UI shapes
   (spinners, interrupt hints, dialog choices/footers).

### Hook integration (optional, most accurate)

Have the agents report state themselves by setting the `@agent_state` pane
option (`working` | `blocked` | `idle` | `done`; unset it to fall back to
scraping). Hook command for both claude (`~/.claude/settings.json`) and
codex (`~/.codex/hooks.json`):

```
tmux set -p -t "$TMUX_PANE" @agent_state <state> 2>/dev/null; exit 0
```

Map `UserPromptSubmit`/`PreToolUse` → `working`, `Stop` → `done`,
`SessionStart` → `idle`, and (claude) `Notification` with a permission
message → `blocked`:

```
grep -qi permission && tmux set -p -t "$TMUX_PANE" @agent_state blocked 2>/dev/null; exit 0
```
