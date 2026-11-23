# tmux-agents

Sidebar showing the status of AI coding agents (claude, codex) across all
tmux sessions. Working / blocked / done / idle, color-coded. Jump to any agent's
pane. Ratatui, single binary, no daemon.

<img width="1512" height="950" alt="image" src="https://github.com/user-attachments/assets/e4407b7b-1ed7-4f8e-9d93-5be8076dc9bb" />

## Install (TPM)

    set -g @plugin 'davychhouk/tmux-agents'

Downloads a prebuilt binary on first run, and re-downloads when the requested
version changes. If the download fails for any reason (unsupported platform,
no `curl`/`wget`, failed checksum), it falls back to `cargo build --release`
when Rust is installed. Requires tmux ≥ 3.0 and either `curl`/`wget` or a
Rust toolchain.

Prebuilt binaries are published for macOS and Linux on x86_64 and arm64.

## Features

- **Agent detection**: finds claude and codex in any tmux session or window,
  including agents started through `node`/`bun`/shell wrappers or inside nvim
  terminals, and both when they share one pane.
- **Live status**: each agent shows as working (animated spinner), blocked
  (`?`, waiting on you), done (`✓`, finished a task), or idle (`○`).
- **Accurate state detection**: uses the agent's own hooks when set up, then
  its terminal title, then what's on screen. Short idle blips while working
  are smoothed out so nothing flashes "done" mid-task.
- **Grouped by session**: agents listed under their tmux session, with window
  name and pane title.
- **Jump to agent**: `Enter` switches to the agent's session, window and pane,
  then collapses (or closes) the sidebar.
- **Collapsible**: toggle between the full list and a narrow icon-only strip,
  or close it entirely with `hidden` mode.
- **Follows you**: the sidebar moves with you when you switch windows or
  sessions, keeping its width.
- **Focus-aware selection**: when you focus the sidebar, the selection lands
  on the current window's agent (if any); the highlight only shows while
  focused.
- **Left or right**: dock on either side, with configurable widths.
- **Lightweight**: one binary, no daemon or state files. Polls on an interval
  and only animates while an agent is working.
- **Easy install**: TPM downloads a checksummed prebuilt binary, or builds
  from source if no binary is available.

## Usage

`prefix a` opens the sidebar, then switches it between expanded and
collapsed. In the default `collapsed` mode it shrinks to an icon-only strip
instead of closing; set `@agents-toggle 'hidden'` to close it entirely.

Focus the sidebar pane (e.g. `prefix` + arrow) to use the keys below. The
selection starts on the current window's agent, if it has one.

| key              | action                                                 |
| ---------------- | ------------------------------------------------------ |
| `j`/`k`, `↓`/`↑` | move selection                                         |
| `Enter`          | jump to agent's pane, then collapse (or close) sidebar |
| `q`, `Ctrl-C`    | quit the sidebar                                       |

## Options

    set -g @agents-key 'a'                # prefix-table key
    set -g @agents-toggle 'collapsed'     # collapsed | hidden
    set -g @agents-follow 'on'            # sidebar follows window/session switches
    set -g @agents-position 'right'       # left | right
    set -g @agents-width '32'             # integer columns (no %)
    set -g @agents-collapsed-width '2'    # default: 3 on left, 2 on right; keep < 12
    set -g @agents-interval '1'           # poll seconds (min 1)
    set -g @agents-version 'v0.2.0'       # release binary to download

Widths are integer column counts; percentages are not supported. The sidebar
renders icon-only below 12 columns, so keep the collapsed width under 12.

## Tech stack

- Rust 2024
- Ratatui 0.30 with its crossterm backend
- tmux commands for pane management, capture, options, hooks, and navigation
- `ps` process snapshots for claude/codex process-tree detection
- Bash TPM entry script (`agents.tmux`)
- GitHub Actions for PR checks and release builds

The project is a single binary. It has no daemon, socket, IPC layer, database,
or state files; the sidebar keeps runtime state in memory.

## Project structure

```text
.
├── .github/workflows/ # CI (ci.yml) and release builds (release.yml)
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

CI runs `cargo fmt --check` and `cargo clippy --all-targets` on every pull
request, plus `cargo test` and `cargo build --release` on Linux and macOS,
and a coverage report (see [Coverage](#coverage)). Warnings fail the build
(`RUSTFLAGS=-D warnings`).

Inside tmux, run the binary directly for local testing:

```sh
cargo run -- toggle      # create/collapse/expand/close the sidebar
cargo run                # run the sidebar TUI in the current pane
cargo run -- follow      # move the sidebar to the active window
```

## Tests

Unit tests live next to the code they test (`#[cfg(test)]` modules) and run
with `cargo test`. They need no tmux server, so they run anywhere, including
CI on Linux and macOS.

| module        | tests | covers                                                                                                                                        |
| ------------- | ----: | --------------------------------------------------------------------------------------------------------------------------------------------- |
| `classify.rs` |    54 | working/blocked/idle from screen text, titles and hook state; screen correcting stale hooks; attributing split regions to agents; dialogs, side-by-side splits, transcript view, false positives from quoted text and prose |
| `detect.rs`   |    14 | finding claude/codex in process trees (direct, nested in nvim, via interpreters, both in one pane, agent run as another's tool), `ps` parsing |
| `app.rs`      |    11 | done debounce, session grouping, j/k selection skipping headers, selection kept across refreshes, picking each agent's hook in a shared pane |
| `toggle.rs`   |     4 | finding the sidebar pane, collapse/expand width, default collapsed width per side                                                             |
| `ui.rs`       |     4 | highlight only while focused, collapsed-mode selection, title cleanup                                                                         |
| `tmux.rs`     |     2 | parsing `list-panes` output                                                                                                                   |

Not covered by automated tests: the tmux commands themselves (spawning,
resizing, moving and jumping panes), focus events, and key handling in the
event loop. Check those by hand in tmux, ideally on a separate server
(`tmux -L test`) so your own session is untouched.

### Coverage

Measured with [cargo-llvm-cov](https://github.com/taiki-e/cargo-llvm-cov). CI
runs it on every pull request and posts the per-file table in the job
summary.

```sh
rustup component add llvm-tools-preview
cargo install cargo-llvm-cov
cargo llvm-cov --summary-only            # per-file table
cargo llvm-cov --open                    # HTML report with uncovered lines
```

Without rustup (e.g. a Nix toolchain), point it at a matching LLVM:
`nix shell nixpkgs#cargo-llvm-cov nixpkgs#rustc.llvm`, then set `LLVM_COV`
and `LLVM_PROFDATA` to the `llvm-cov` and `llvm-profdata` on `PATH`.

Current numbers:

| file          | lines  | functions |
| ------------- | -----: | --------: |
| `classify.rs` |  99.4% |    100.0% |
| `ui.rs`       |  94.7% |    100.0% |
| `detect.rs`   |  92.6% |     96.4% |
| `app.rs`      |  60.7% |     66.7% |
| `toggle.rs`   |  37.3% |     56.3% |
| `tmux.rs`     |  39.3% |     35.7% |
| `main.rs`     |   0.0% |      0.0% |
| **total**     |  77.9% |     86.1% |

The pure logic (state detection, process detection, rendering) is nearly
fully covered. The low numbers are the tmux side: the event loop, tmux
commands, and CLI dispatch.

## Release

Releases are tag-driven. A `v*` tag builds tarballs and SHA-256 files for:

- `aarch64-apple-darwin`
- `x86_64-apple-darwin`
- `aarch64-unknown-linux-gnu`
- `x86_64-unknown-linux-gnu`

To cut a release, bump `version` in `Cargo.toml` and the default `version` in
`agents.tmux` to the new tag in the same commit, then:

```sh
cargo fmt --check
cargo test
cargo clippy --all-targets
git tag -a vX.Y.Z -m vX.Y.Z
git push origin main vX.Y.Z
```

`agents.tmux` downloads `v0.2.0` by default. To test another release:

```tmux
set -g @agents-version 'v0.1.0'
```

## How it works

Polls `tmux list-panes -a`, walks each pane's process tree for claude/codex
(including inside nvim terminals; an agent's own child processes are its
tools, not more agents), falls back to spotting codex's "OpenAI Codex"
banner on screen, and classifies each agent into
working / blocked / idle. An agent that goes from working to idle shows as
done (green) until it starts working again. The switch to done waits for a
second idle reading, so a brief pause mid-task doesn't flash done.

Per pane, signals are consulted in order:

1. **`@agent_state` pane option** — ground truth, pushed by the agent's own
   lifecycle hooks (see below). A live dialog on screen still overrides a
   non-blocked value, since some dialogs emit no hook event.
2. **Pane title (OSC)** — a leading braille spinner frame means working;
   codex titles a pending approval "Action Required".
3. **Screen scraping** — `capture-pane` tail matched against live UI shapes
   (spinners, interrupt hints, dialog choices/footers). Side-by-side splits
   (e.g. an agent in an nvim split) are read as separate regions, and claude's
   `ctrl+o` transcript view keeps the last known state.

### Hook integration (optional, most accurate)

Have the agents report state themselves by setting the `@agent_state` pane
option (`working` | `blocked` | `idle` | `done`; unset it to fall back to
scraping). Strongly recommended when agents run inside nvim: the terminal
title never reaches tmux there, so hooks and the screen are the only signals.

Hook command for both claude (`~/.claude/settings.json`) and codex
(`~/.codex/hooks.json`):

```
tmux set -p -t "$TMUX_PANE" @agent_state <state> 2>/dev/null; exit 0
```

**Running both agents in one pane** (e.g. two terminal buffers in one
nvim): they share `$TMUX_PANE`, so they'd overwrite each other's
`@agent_state`. Have each write its own option instead,
`@agent_state_claude` or `@agent_state_codex`. It's enough for one of them
to switch: the sidebar then knows the shared `@agent_state` belongs to the
other.

Without per-agent hooks, the screen still works when each agent is visible
in its own split: the pane is cut at nvim's vertical separators and each
region is claimed by its agent's prompt line (claude's input box or a
claude dialog choice, codex's `›` composer). An agent whose split is hidden or covered (another tab, a
floating window) has no screen evidence, so it shows idle rather than the
other agent's state. The pane title never counts in a shared pane.

| event                                         | state                                         |
| --------------------------------------------- | --------------------------------------------- |
| `UserPromptSubmit`, `PreToolUse`, `PostToolUse` | `working`                                     |
| `Stop`                                        | `done`                                        |
| `SessionStart`                                | `idle`                                        |
| `SessionEnd`                                  | unset: `tmux set -p -u -t "$TMUX_PANE" @agent_state` |
| `PermissionRequest` (claude)                  | `blocked`                                     |
| `Interrupt` (codex)                           | `idle`                                        |

`PostToolUse` clears `blocked` once an approved tool runs. Use claude's
`PermissionRequest` rather than its `Notification` hook: the permission
notification only fires after about 6 seconds without typing.

Hooks can go stale (claude skips `Stop` when you interrupt with Esc), so the
screen still corrects them when it's clear: a dialog on screen means blocked,
visible work clears a stale blocked, and claude's empty input box with no
work showing clears a stale working or blocked. When the agent isn't visible
(e.g. a hidden nvim buffer), the hook is trusted as is.

## License

MIT — see [LICENSE](LICENSE).
