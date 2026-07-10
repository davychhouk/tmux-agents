# tmux-agents

Sidebar showing the status of AI coding agents (claude, codex) across all
tmux sessions. Working / blocked / done / idle, color-coded. Jump to any agent's
pane. Ratatui, single binary, no daemon.

## Install (TPM)

    set -g @plugin 'davychhouk/tmux-agents'

Downloads a prebuilt binary on first run, and re-downloads when the requested
version changes. If the download fails for any reason (unsupported platform,
no `curl`/`wget`, failed checksum), it falls back to `cargo build --release`
when Rust is installed. Requires tmux ≥ 3.0 and either `curl`/`wget` or a
Rust toolchain.

Prebuilt binaries are published for macOS and Linux on x86_64 and arm64.

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

## Tech stack

- Rust 2024
- Ratatui 0.30 with its crossterm backend
- tmux commands for pane management, capture, options, hooks, and navigation
- `ps` process snapshots for claude/codex process-tree detection
- Bash TPM entry script (`agents.tmux`)
- GitHub Actions release builds for prebuilt binaries

The project is a single binary. It has no daemon, socket, IPC layer, database,
or state files; the sidebar keeps runtime state in memory.

## Project structure

```text
.
├── .github/workflows/ # Release build automation
├── agents.tmux        # TPM entry script; downloads/binds/hooks the binary
├── Cargo.toml         # Rust crate manifest and lint settings
├── Cargo.lock         # Locked Rust dependencies
├── README.md          # User and developer documentation
├── docs/              # Design notes and implementation plans
└── src/
    ├── main.rs        # CLI dispatch: default TUI, toggle, follow
    ├── app.rs         # Polling loop, selection, debounce, jump behavior
    ├── classify.rs    # Pane text/title/hook state -> agent state
    ├── detect.rs      # Process snapshot and descendant agent detection
    ├── tmux.rs        # tmux command wrapper and pane parsing
    ├── toggle.rs      # Sidebar spawn, collapse/expand, close, follow
    └── ui.rs          # Expanded list and collapsed icon-strip rendering
```

## Commands

```sh
cargo build              # debug build
cargo build --release    # production binary for local fallback builds
cargo test               # unit tests
cargo fmt --check        # formatting check
cargo clippy --all-targets
```

Inside tmux, run the binary directly for local testing:

```sh
cargo run -- toggle      # create/collapse/expand/close the sidebar
cargo run                # run the sidebar TUI in the current pane
cargo run -- follow      # move the sidebar to the active window
```

## Release

Releases are tag-driven. A `v*` tag builds tarballs and SHA-256 files for:

- `aarch64-apple-darwin`
- `x86_64-apple-darwin`
- `aarch64-unknown-linux-gnu`
- `x86_64-unknown-linux-gnu`

To cut a release, bump the default `version` in `agents.tmux` to the new tag
in the same commit, then:

```sh
cargo fmt --check
cargo test
cargo clippy --all-targets
git tag v0.1.0
git push origin main v0.1.0
```

`agents.tmux` downloads `v0.1.0` by default. For testing another release:

```tmux
set -g @agents-version 'v0.1.1'
```

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

## License

MIT — see [LICENSE](LICENSE).
