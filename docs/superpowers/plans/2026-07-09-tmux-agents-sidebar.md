# tmux-agents Sidebar Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** tmux plugin showing claude/codex agent status across all sessions in a toggleable Ratatui sidebar pane, with jump-to-pane and expand/collapse.

**Architecture:** Single Rust binary (`tmux-agents`) + thin TPM shim (`agents.tmux`). The binary has two modes: `toggle` (spawn/kill the sidebar pane) and default (the TUI, which polls `tmux list-panes`/`capture-pane` + one `ps` snapshot per tick). No daemon, no state files. Spec: `docs/superpowers/specs/2026-07-09-tmux-agents-sidebar-design.md`.

**Tech Stack:** Rust 1.95, ratatui 0.30 (crossterm re-exported as `ratatui::crossterm`), no other deps. Plain substring matching for classification — no regex crate.

**File map:**
- `Cargo.toml` — crate manifest
- `src/main.rs` — arg dispatch: `toggle` vs TUI
- `src/classify.rs` — pane text → `AgentState` (unit tested)
- `src/detect.rs` — `ps` snapshot + descendant walk → `AgentKind` (unit tested)
- `src/tmux.rs` — tmux command wrappers + pane-list parsing (parse unit tested)
- `src/toggle.rs` — spawn/kill sidebar pane
- `src/app.rs` — TUI: refresh, rows, event loop, render (row logic unit tested)
- `agents.tmux` — TPM entry script
- `README.md`

Run all commands from `/Users/dvc/projects/tmux-agents`. `direnv` provides cargo; if a bare `cargo` fails, prefix with `direnv exec . `.

---

### Task 1: Cargo scaffold + CLI dispatch

**Files:**
- Create: `Cargo.toml`, `src/main.rs`, `src/classify.rs`, `src/detect.rs`, `src/tmux.rs`, `src/toggle.rs`, `src/app.rs`

- [ ] **Step 1: Init crate**

```bash
cargo init --name tmux-agents
cargo add ratatui@0.30
```

- [ ] **Step 2: Write `src/main.rs` dispatch + empty module stubs**

`src/main.rs`:
```rust
mod app;
mod classify;
mod detect;
mod tmux;
mod toggle;

fn main() -> std::io::Result<()> {
    match std::env::args().nth(1).as_deref() {
        Some("toggle") => toggle::toggle(),
        None => app::run(),
        Some(other) => {
            eprintln!("unknown command: {other}\nusage: tmux-agents [toggle]");
            std::process::exit(2);
        }
    }
}
```

Stub contents so it compiles:

`src/toggle.rs`:
```rust
pub fn toggle() -> std::io::Result<()> {
    todo!("task 5")
}
```

`src/app.rs`:
```rust
pub fn run() -> std::io::Result<()> {
    todo!("task 6")
}
```

`src/classify.rs`, `src/detect.rs`, `src/tmux.rs`: empty files for now.

- [ ] **Step 3: Verify build**

Run: `cargo build`
Expected: compiles, no warnings.

- [ ] **Step 4: Commit**

```bash
git add Cargo.toml Cargo.lock src
git commit -m "feat: scaffold tmux-agents crate with CLI dispatch"
```

---

### Task 2: State classifier (`src/classify.rs`)

Pane text → Working/Blocked/Idle. Blocked checked before Working (an approval prompt may still show working-ish text above it). Patterns are lowercase substrings; spinner chars cover codex (braille) and claude (asterisk glyphs). These are best-effort seeds — Task 8 captures real fixtures and refines.

**Files:**
- Modify: `src/classify.rs`

- [ ] **Step 1: Write failing tests**

Append to `src/classify.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocked_on_claude_permission_prompt() {
        let text = "Bash command\n\nDo you want to proceed?\n❯ 1. Yes\n  2. No";
        assert_eq!(classify(text), AgentState::Blocked);
    }

    #[test]
    fn blocked_on_codex_approval() {
        let text = "codex wants to run: rm -rf build\nAllow command?\n> Yes (y)";
        assert_eq!(classify(text), AgentState::Blocked);
    }

    #[test]
    fn working_on_esc_to_interrupt() {
        let text = "✳ Cerebrating… (12s · esc to interrupt)";
        assert_eq!(classify(text), AgentState::Working);
    }

    #[test]
    fn working_on_braille_spinner() {
        let text = "⠹ Running tests";
        assert_eq!(classify(text), AgentState::Working);
    }

    #[test]
    fn blocked_wins_over_working() {
        let text = "esc to interrupt\nDo you want to proceed?\n❯ 1. Yes";
        assert_eq!(classify(text), AgentState::Blocked);
    }

    #[test]
    fn idle_on_plain_prompt() {
        let text = "╭──────╮\n│ >    │\n╰──────╯\n? for shortcuts";
        assert_eq!(classify(text), AgentState::Idle);
    }

    #[test]
    fn idle_on_empty() {
        assert_eq!(classify(""), AgentState::Idle);
    }
}
```

- [ ] **Step 2: Run tests, verify they fail**

Run: `cargo test classify`
Expected: compile error (`classify`, `AgentState` undefined).

- [ ] **Step 3: Implement**

Top of `src/classify.rs`:
```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentState {
    Working,
    Blocked,
    Idle,
}

// ponytail: seed patterns; refined against real captures in the fixtures task
const BLOCKED: &[&str] = &[
    "do you want",
    "would you like",
    "❯ 1. yes",
    "allow command",
    "yes (y)",
];

const WORKING: &[&str] = &["esc to interrupt", "ctrl+c to interrupt"];

const SPINNERS: &[char] = &[
    '⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏', // braille (codex)
    '✳', '✶', '✻', '✽', '✢', // claude thinking glyphs
];

pub fn classify(pane_text: &str) -> AgentState {
    let text = pane_text.to_lowercase();
    if BLOCKED.iter().any(|p| text.contains(p)) {
        return AgentState::Blocked;
    }
    if WORKING.iter().any(|p| text.contains(p))
        || pane_text.chars().any(|c| SPINNERS.contains(&c))
    {
        return AgentState::Working;
    }
    AgentState::Idle
}
```

- [ ] **Step 4: Run tests, verify pass**

Run: `cargo test classify`
Expected: 7 passed.

- [ ] **Step 5: Commit**

```bash
git add src/classify.rs
git commit -m "feat: pane-text state classifier"
```

---

### Task 3: Agent detection via process tree (`src/detect.rs`)

Pane pid → walk descendants in one `ps` snapshot → first claude/codex match. Matching checks the basename of every whitespace token in the command line, so `nvim` → `claude` children and `node /path/to/claude` both match.

**Files:**
- Modify: `src/detect.rs`

- [ ] **Step 1: Write failing tests**

Append to `src/detect.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn p(pid: u32, ppid: u32, command: &str) -> Proc {
        Proc { pid, ppid, command: command.to_string() }
    }

    #[test]
    fn finds_direct_child() {
        let procs = vec![p(10, 1, "-fish"), p(20, 10, "claude")];
        assert_eq!(find_agent(&procs, 10), Some(AgentKind::Claude));
    }

    #[test]
    fn finds_agent_nested_in_nvim() {
        let procs = vec![
            p(10, 1, "-fish"),
            p(20, 10, "nvim ."),
            p(30, 20, "/bin/fish -c codex"),
            p(40, 30, "codex --model gpt-5"),
        ];
        assert_eq!(find_agent(&procs, 10), Some(AgentKind::Codex));
    }

    #[test]
    fn matches_path_basename() {
        let procs = vec![p(10, 1, "fish"), p(20, 10, "node /Users/x/.bun/bin/claude")];
        assert_eq!(find_agent(&procs, 10), Some(AgentKind::Claude));
    }

    #[test]
    fn no_agent() {
        let procs = vec![p(10, 1, "-fish"), p(20, 10, "vim notes.md")];
        assert_eq!(find_agent(&procs, 10), None);
    }

    #[test]
    fn ignores_other_subtrees() {
        let procs = vec![p(10, 1, "fish"), p(11, 1, "fish"), p(20, 11, "claude")];
        assert_eq!(find_agent(&procs, 10), None);
    }

    #[test]
    fn parses_ps_output() {
        let out = "  10     1 -fish\n  20    10 nvim .\n";
        let procs = parse_ps(out);
        assert_eq!(procs.len(), 2);
        assert_eq!(procs[1], Proc { pid: 20, ppid: 10, command: "nvim .".to_string() });
    }
}
```

- [ ] **Step 2: Run tests, verify fail**

Run: `cargo test detect`
Expected: compile error (types undefined).

- [ ] **Step 3: Implement**

Top of `src/detect.rs`:
```rust
use std::process::Command;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentKind {
    Claude,
    Codex,
}

impl AgentKind {
    pub fn label(self) -> &'static str {
        match self {
            AgentKind::Claude => "claude",
            AgentKind::Codex => "codex",
        }
    }

    fn from_command(command: &str) -> Option<Self> {
        for token in command.split_whitespace() {
            match token.rsplit('/').next() {
                Some("claude") => return Some(AgentKind::Claude),
                Some("codex") => return Some(AgentKind::Codex),
                _ => {}
            }
        }
        None
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Proc {
    pub pid: u32,
    pub ppid: u32,
    pub command: String,
}

pub fn ps_snapshot() -> std::io::Result<Vec<Proc>> {
    let out = Command::new("ps").args(["-eo", "pid=,ppid=,command="]).output()?;
    Ok(parse_ps(&String::from_utf8_lossy(&out.stdout)))
}

fn parse_ps(out: &str) -> Vec<Proc> {
    out.lines()
        .filter_map(|line| {
            let mut parts = line.split_whitespace();
            let pid = parts.next()?.parse().ok()?;
            let ppid = parts.next()?.parse().ok()?;
            let command = parts.collect::<Vec<_>>().join(" ");
            Some(Proc { pid, ppid, command })
        })
        .collect()
}

/// First claude/codex found in the descendant tree of `root` (BFS).
pub fn find_agent(procs: &[Proc], root: u32) -> Option<AgentKind> {
    let mut queue = vec![root];
    while let Some(pid) = queue.pop() {
        for proc in procs.iter().filter(|p| p.ppid == pid) {
            if let Some(kind) = AgentKind::from_command(&proc.command) {
                return Some(kind);
            }
            queue.push(proc.pid);
        }
    }
    None
}
```

Note: `parse_ps` stays private — the `#[cfg(test)] mod tests` in the same file sees private items. Same pattern in other modules.

- [ ] **Step 4: Run tests, verify pass**

Run: `cargo test detect`
Expected: 6 passed.

- [ ] **Step 5: Commit**

```bash
git add src/detect.rs
git commit -m "feat: agent detection via process-tree walk"
```

---

### Task 4: tmux wrappers (`src/tmux.rs`)

Thin `Command` wrappers + a unit-tested pane-list parser. Uses `\t` field separator; window/session names can contain spaces but not tabs.

**Files:**
- Modify: `src/tmux.rs`

- [ ] **Step 1: Write failing test for parser**

Append to `src/tmux.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_pane_lines() {
        let out = "work\t1\tapi server\t%3\t4242\nwork\t2\tshell\t%7\t4300\n";
        let panes = parse_panes(out);
        assert_eq!(panes.len(), 2);
        assert_eq!(
            panes[0],
            Pane {
                session: "work".into(),
                window_index: 1,
                window_name: "api server".into(),
                id: "%3".into(),
                pid: 4242,
            }
        );
    }

    #[test]
    fn skips_malformed_lines() {
        assert!(parse_panes("garbage\n").is_empty());
    }
}
```

- [ ] **Step 2: Run test, verify fail**

Run: `cargo test tmux`
Expected: compile error.

- [ ] **Step 3: Implement**

Top of `src/tmux.rs`:
```rust
use std::io;
use std::process::Command;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pane {
    pub session: String,
    pub window_index: u32,
    pub window_name: String,
    pub id: String,
    pub pid: u32,
}

/// Run tmux, return stdout. Err if tmux exits nonzero (e.g. server gone).
pub fn tmux(args: &[&str]) -> io::Result<String> {
    let out = Command::new("tmux").args(args).output()?;
    if !out.status.success() {
        return Err(io::Error::other(String::from_utf8_lossy(&out.stderr).into_owned()));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

pub fn list_panes() -> io::Result<Vec<Pane>> {
    let out = tmux(&[
        "list-panes",
        "-a",
        "-F",
        "#{session_name}\t#{window_index}\t#{window_name}\t#{pane_id}\t#{pane_pid}",
    ])?;
    Ok(parse_panes(&out))
}

fn parse_panes(out: &str) -> Vec<Pane> {
    out.lines()
        .filter_map(|line| {
            let f: Vec<&str> = line.split('\t').collect();
            let [session, window_index, window_name, id, pid] = f.as_slice() else {
                return None;
            };
            Some(Pane {
                session: session.to_string(),
                window_index: window_index.parse().ok()?,
                window_name: window_name.to_string(),
                id: id.to_string(),
                pid: pid.parse().ok()?,
            })
        })
        .collect()
}

/// Last 40 lines of a pane's visible content.
pub fn capture(pane_id: &str) -> io::Result<String> {
    tmux(&["capture-pane", "-p", "-t", pane_id, "-S", "-40"])
}

/// Global option with default (`show-options -gqv` prints empty when unset).
pub fn option(name: &str, default: &str) -> String {
    match tmux(&["show-options", "-gqv", name]) {
        Ok(v) if !v.trim().is_empty() => v.trim().to_string(),
        _ => default.to_string(),
    }
}

pub fn jump(pane: &Pane) -> io::Result<()> {
    tmux(&["switch-client", "-t", &pane.session])?;
    tmux(&["select-window", "-t", &format!("{}:{}", pane.session, pane.window_index)])?;
    tmux(&["select-pane", "-t", &pane.id])?;
    Ok(())
}

pub fn resize_self(width: u16) -> io::Result<()> {
    let pane = std::env::var("TMUX_PANE").map_err(io::Error::other)?;
    tmux(&["resize-pane", "-t", &pane, "-x", &width.to_string()])?;
    Ok(())
}
```

- [ ] **Step 4: Run tests, verify pass**

Run: `cargo test tmux`
Expected: 2 passed.

- [ ] **Step 5: Full build check**

Run: `cargo build 2>&1 | grep -i warn`
Expected: no output. Dead-code warnings for not-yet-used functions are acceptable ONLY if they disappear by Task 6 — if any appear, note and move on.

- [ ] **Step 6: Commit**

```bash
git add src/tmux.rs
git commit -m "feat: tmux command wrappers and pane parsing"
```

---

### Task 5: `toggle` subcommand (`src/toggle.rs`)

Kill existing sidebar in current window, else spawn one. Sidebar marked with pane user option `@agents_sidebar=1` (needs tmux ≥ 3.0).

**Files:**
- Modify: `src/toggle.rs`

- [ ] **Step 1: Implement (no unit test — pure tmux side effects; verified manually in Task 7)**

Replace `src/toggle.rs`:
```rust
use std::io;

use crate::tmux::{option, tmux};

pub fn toggle() -> io::Result<()> {
    // Panes in the *current* window, with sidebar marker.
    let out = tmux(&["list-panes", "-F", "#{pane_id}\t#{@agents_sidebar}"])?;
    if let Some(id) = out.lines().find_map(|l| {
        let (id, mark) = l.split_once('\t')?;
        (mark == "1").then(|| id.to_string())
    }) {
        tmux(&["kill-pane", "-t", &id])?;
        return Ok(());
    }

    let width = option("@agents-width", "32");
    let exe = std::env::current_exe()?;
    let exe = exe.to_str().ok_or_else(|| io::Error::other("non-utf8 exe path"))?;

    let mut args = vec!["split-window", "-h", "-f", "-l", width.as_str()];
    if option("@agents-position", "right") == "left" {
        args.push("-b");
    }
    args.extend(["-P", "-F", "#{pane_id}", exe]);
    let pane_id = tmux(&args)?;
    tmux(&["set-option", "-p", "-t", pane_id.trim(), "@agents_sidebar", "1"])?;
    Ok(())
}
```

- [ ] **Step 2: Build check**

Run: `cargo build 2>&1 | grep -iE 'warn|error'`
Expected: no output.

- [ ] **Step 3: Commit**

```bash
git add src/toggle.rs
git commit -m "feat: toggle subcommand spawns/kills sidebar pane"
```

---

### Task 6: TUI (`src/app.rs`)

Refresh loop + grouped list + keys. Row building and selection movement are pure functions with unit tests; rendering/event loop verified manually in Task 7.

Behavior:
- Tick: every `@agents-interval` seconds rebuild agents (list_panes → ps snapshot → find_agent → capture → classify). Panes that vanish mid-tick just drop out (capture error → skip).
- Rows: per session (sorted): header row, then one row per agent. Selection only lands on agent rows.
- Collapsed = render icons only; decided by actual frame width (`< 12` cols), so manual pane resizes behave too. `Tab` resizes own pane between `@agents-width` and `@agents-collapsed-width`.
- Keys: `j`/`Down`, `k`/`Up`, `Enter` jump, `Tab` toggle width, `q`/`Ctrl-c` quit.
- tmux errors during refresh (server gone) → exit cleanly.

**Files:**
- Modify: `src/app.rs`

- [ ] **Step 1: Write failing tests for row logic**

Replace `src/app.rs` with tests first (implementation added in Step 3 above them):
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::classify::AgentState;
    use crate::detect::AgentKind;
    use crate::tmux::Pane;

    fn agent(session: &str, window: u32) -> Agent {
        Agent {
            pane: Pane {
                session: session.into(),
                window_index: window,
                window_name: format!("w{window}"),
                id: format!("%{window}"),
                pid: 100 + window,
            },
            kind: AgentKind::Claude,
            state: AgentState::Idle,
        }
    }

    #[test]
    fn rows_group_by_session_with_headers() {
        // build_rows assumes input already sorted (refresh() sorts).
        let agents = vec![agent("a", 1), agent("a", 2), agent("b", 1)];
        let rows = build_rows(&agents);
        // a-header, a:1, a:2, b-header, b:1
        assert_eq!(rows.len(), 5);
        assert!(matches!(rows[0], Row::Header(ref s) if s == "a"));
        assert!(matches!(rows[1], Row::Agent(i) if agents_at(&agents, i) == ("a", 1)));
        assert!(matches!(rows[3], Row::Header(ref s) if s == "b"));
    }

    fn agents_at(agents: &[Agent], i: usize) -> (&str, u32) {
        (&agents[i].pane.session, agents[i].pane.window_index)
    }

    #[test]
    fn selection_skips_headers() {
        let agents = vec![agent("a", 1), agent("b", 1)];
        let rows = build_rows(&agents); // header, agent, header, agent
        assert_eq!(next_agent_row(&rows, None, 1), Some(1));
        assert_eq!(next_agent_row(&rows, Some(1), 1), Some(3));
        assert_eq!(next_agent_row(&rows, Some(3), 1), Some(3)); // clamp at end
        assert_eq!(next_agent_row(&rows, Some(3), -1), Some(1));
        assert_eq!(next_agent_row(&rows, Some(1), -1), Some(1)); // clamp at start
    }

    #[test]
    fn no_agents_no_selection() {
        assert_eq!(next_agent_row(&[], None, 1), None);
    }
}
```

- [ ] **Step 2: Run tests, verify fail**

Run: `cargo test app`
Expected: compile error.

- [ ] **Step 3: Implement**

Insert above the tests in `src/app.rs`:
```rust
use std::io;
use std::time::{Duration, Instant};

use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{List, ListItem, ListState};
use ratatui::Frame;

use crate::classify::{classify, AgentState};
use crate::detect::{find_agent, ps_snapshot, AgentKind};
use crate::tmux;

const COLLAPSE_THRESHOLD: u16 = 12;

pub struct Agent {
    pub pane: tmux::Pane,
    pub kind: AgentKind,
    pub state: AgentState,
}

enum Row {
    Header(String),
    Agent(usize),
}

fn refresh() -> io::Result<Vec<Agent>> {
    let panes = tmux::list_panes()?;
    let procs = ps_snapshot()?;
    let own = std::env::var("TMUX_PANE").unwrap_or_default();
    let mut agents: Vec<Agent> = panes
        .into_iter()
        .filter(|p| p.id != own)
        .filter_map(|pane| {
            let kind = find_agent(&procs, pane.pid)?;
            let state = classify(&tmux::capture(&pane.id).ok()?);
            Some(Agent { pane, kind, state })
        })
        .collect();
    agents.sort_by(|a, b| {
        (&a.pane.session, a.pane.window_index).cmp(&(&b.pane.session, b.pane.window_index))
    });
    Ok(agents)
}

fn build_rows(agents: &[Agent]) -> Vec<Row> {
    let mut rows = Vec::new();
    let mut current: Option<&str> = None;
    for (i, agent) in agents.iter().enumerate() {
        if current != Some(agent.pane.session.as_str()) {
            current = Some(agent.pane.session.as_str());
            rows.push(Row::Header(agent.pane.session.clone()));
        }
        rows.push(Row::Agent(i));
    }
    rows
}

/// Next selectable (agent) row from `from` in direction `dir`; clamps at
/// ends. `from` of None or a non-agent row selects the first agent row.
fn next_agent_row(rows: &[Row], from: Option<usize>, dir: i32) -> Option<usize> {
    let agent_rows: Vec<usize> = rows
        .iter()
        .enumerate()
        .filter_map(|(i, r)| matches!(r, Row::Agent(_)).then_some(i))
        .collect();
    let pos = from.and_then(|cur| agent_rows.iter().position(|&i| i == cur));
    let next = match pos {
        None => 0,
        Some(p) if dir > 0 => (p + 1).min(agent_rows.len().saturating_sub(1)),
        Some(p) => p.saturating_sub(1),
    };
    agent_rows.get(next).copied()
}

fn state_style(state: AgentState) -> (char, Style) {
    match state {
        AgentState::Working => ('●', Style::default().fg(Color::Yellow)),
        AgentState::Blocked => ('●', Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
        AgentState::Idle => ('●', Style::default().fg(Color::DarkGray)),
    }
}

fn render(frame: &mut Frame, agents: &[Agent], rows: &[Row], list_state: &mut ListState) {
    let area = frame.area();
    if area.width < COLLAPSE_THRESHOLD {
        render_collapsed(frame, agents, area);
        return;
    }
    let items: Vec<ListItem> = rows
        .iter()
        .map(|row| match row {
            Row::Header(session) => ListItem::new(Line::from(Span::styled(
                session.clone(),
                Style::default().add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
            ))),
            Row::Agent(i) => {
                let a = &agents[*i];
                let (icon, style) = state_style(a.state);
                ListItem::new(Line::from(vec![
                    Span::styled(format!(" {icon} "), style),
                    Span::raw(format!("{:<6} ", a.kind.label())),
                    Span::styled(a.pane.window_name.clone(), Style::default().fg(Color::Gray)),
                ]))
            }
        })
        .collect();
    let list = List::new(items).highlight_style(Style::default().add_modifier(Modifier::REVERSED));
    frame.render_stateful_widget(list, area, list_state);
}

fn render_collapsed(frame: &mut Frame, agents: &[Agent], area: Rect) {
    let items: Vec<ListItem> = agents
        .iter()
        .map(|a| {
            let (icon, style) = state_style(a.state);
            ListItem::new(Line::from(Span::styled(format!(" {icon}"), style)))
        })
        .collect();
    frame.render_widget(List::new(items), area);
}

pub fn run() -> io::Result<()> {
    let interval: u64 = tmux::option("@agents-interval", "1").parse().unwrap_or(1);
    let interval = Duration::from_secs(interval.max(1));
    let width: u16 = tmux::option("@agents-width", "32").parse().unwrap_or(32);
    let collapsed_width: u16 = tmux::option("@agents-collapsed-width", "6").parse().unwrap_or(6);

    let mut terminal = ratatui::init();
    let result = event_loop(&mut terminal, interval, width, collapsed_width);
    ratatui::restore();
    result
}

fn event_loop(
    terminal: &mut ratatui::DefaultTerminal,
    interval: Duration,
    width: u16,
    collapsed_width: u16,
) -> io::Result<()> {
    let mut agents = refresh()?;
    let mut rows = build_rows(&agents);
    let mut list_state = ListState::default();
    let mut last_tick = Instant::now();
    let mut expanded = true;

    loop {
        terminal.draw(|f| render(f, &agents, &rows, &mut list_state))?;

        let timeout = interval.saturating_sub(last_tick.elapsed());
        if event::poll(timeout)? {
            match event::read()? {
                Event::Key(key) if key.kind == KeyEventKind::Press => match key.code {
                    KeyCode::Char('q') => return Ok(()),
                    KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        return Ok(());
                    }
                    KeyCode::Char('j') | KeyCode::Down => {
                        list_state.select(next_agent_row(&rows, list_state.selected(), 1));
                    }
                    KeyCode::Char('k') | KeyCode::Up => {
                        list_state.select(next_agent_row(&rows, list_state.selected(), -1));
                    }
                    KeyCode::Enter => {
                        if let Some(Row::Agent(i)) =
                            list_state.selected().and_then(|s| rows.get(s))
                        {
                            let _ = tmux::jump(&agents[*i].pane);
                        }
                    }
                    KeyCode::Tab => {
                        expanded = !expanded;
                        let _ = tmux::resize_self(if expanded { width } else { collapsed_width });
                    }
                    _ => {}
                },
                _ => {}
            }
        }

        if last_tick.elapsed() >= interval {
            // Server gone → refresh errors → exit cleanly.
            agents = refresh()?;
            rows = build_rows(&agents);
            if let Some(s) = list_state.selected() {
                if s >= rows.len() {
                    list_state.select(next_agent_row(&rows, None, 1));
                }
            }
            last_tick = Instant::now();
        }
    }
}
```

- [ ] **Step 4: Run all tests + build**

Run: `cargo test && cargo build --release 2>&1 | grep -iE 'warn|error'`
Expected: all tests pass; grep prints nothing.

- [ ] **Step 5: Commit**

```bash
git add src/app.rs
git commit -m "feat: ratatui sidebar TUI with jump and collapse"
```

---

### Task 7: TPM shim + README + manual verification

**Files:**
- Create: `agents.tmux`, `README.md`

- [ ] **Step 1: Write `agents.tmux`**

```bash
#!/usr/bin/env bash
dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
bin="$dir/target/release/tmux-agents"

if [ ! -x "$bin" ]; then
  (cd "$dir" && cargo build --release) || exit 1
fi

key="$(tmux show-option -gqv @agents-key)"
tmux bind-key "${key:-a}" run-shell "$bin toggle"
```

Run: `chmod +x agents.tmux`

- [ ] **Step 2: Write `README.md`**

```markdown
# tmux-agents

Sidebar showing the status of AI coding agents (claude, codex) across all
tmux sessions. Working / blocked / idle, color-coded. Jump to any agent's
pane. Ratatui, single binary, no daemon.

## Install (TPM)

    set -g @plugin 'dvcrn/tmux-agents'

Needs a Rust toolchain on first run (builds the binary). tmux ≥ 3.0.

## Usage

`prefix a` toggles the sidebar.

| key | action |
|-----|--------|
| `j`/`k` | move selection |
| `Enter` | jump to agent's pane |
| `Tab` | collapse/expand |
| `q` | close |

## Options

    set -g @agents-position 'right'       # left | right
    set -g @agents-width '32'
    set -g @agents-collapsed-width '6'
    set -g @agents-key 'a'                # prefix-table key
    set -g @agents-interval '1'           # poll seconds

## How it works

Polls `tmux list-panes -a`, walks each pane's process tree for claude/codex
(including inside nvim terminals), and classifies `capture-pane` output into
working / blocked / idle.
```

- [ ] **Step 3: Manual verification inside tmux**

Preconditions: run these inside a tmux session.

```bash
tmux bind-key a run-shell "$PWD/target/release/tmux-agents toggle"
```

Then verify each, noting results:
1. `prefix a` → sidebar opens right, 32 cols, full height.
2. Start `claude` in another window; within ~1s it appears with a session header; ask it something → yellow while working; let it hit a permission prompt → red; idle → dim.
3. Start `codex` inside `nvim` terminal → appears.
4. `j`/`k` move between agents, skip headers; `Enter` jumps to the agent's pane.
5. In sidebar: `Tab` → shrinks to 6 cols, icons only; `Tab` → back.
6. `prefix a` again → sidebar closes. Re-open, `q` also closes.
7. `tmux set -g @agents-position left`, toggle twice → opens on the left.
8. Kill an agent pane while sidebar open → row disappears next tick.

Fix whatever fails before proceeding (use superpowers:systematic-debugging if non-obvious).

- [ ] **Step 4: Commit**

```bash
git add agents.tmux README.md
git commit -m "feat: TPM entry script and README"
```

---

### Task 8: Real fixtures for the classifier

Seed patterns in Task 2 are guesses; validate against reality.

**Files:**
- Create: `tests/fixtures/*.txt`
- Modify: `src/classify.rs`

- [ ] **Step 1: Capture real pane content**

With real agents running in tmux panes (get pane ids from `tmux list-panes -a`):

```bash
mkdir -p tests/fixtures
tmux capture-pane -p -t <claude-working-pane> -S -40 > tests/fixtures/claude_working.txt
tmux capture-pane -p -t <claude-blocked-pane> -S -40 > tests/fixtures/claude_blocked.txt
tmux capture-pane -p -t <claude-idle-pane>    -S -40 > tests/fixtures/claude_idle.txt
tmux capture-pane -p -t <codex-working-pane>  -S -40 > tests/fixtures/codex_working.txt
tmux capture-pane -p -t <codex-blocked-pane>  -S -40 > tests/fixtures/codex_blocked.txt
tmux capture-pane -p -t <codex-idle-pane>     -S -40 > tests/fixtures/codex_idle.txt
```

Capture each state as it happens (working while the agent runs, blocked at an approval prompt, idle at rest).

- [ ] **Step 2: Add fixture tests**

Append inside `mod tests` in `src/classify.rs`:
```rust
macro_rules! fixture_test {
    ($name:ident, $file:literal, $expected:expr) => {
        #[test]
        fn $name() {
            let text = include_str!(concat!("../tests/fixtures/", $file));
            assert_eq!(classify(text), $expected, "fixture {}", $file);
        }
    };
}

fixture_test!(fx_claude_working, "claude_working.txt", AgentState::Working);
fixture_test!(fx_claude_blocked, "claude_blocked.txt", AgentState::Blocked);
fixture_test!(fx_claude_idle, "claude_idle.txt", AgentState::Idle);
fixture_test!(fx_codex_working, "codex_working.txt", AgentState::Working);
fixture_test!(fx_codex_blocked, "codex_blocked.txt", AgentState::Blocked);
fixture_test!(fx_codex_idle, "codex_idle.txt", AgentState::Idle);
```

- [ ] **Step 3: Run tests; adjust patterns until green**

Run: `cargo test classify`
Expected: fixture tests may fail — adjust `BLOCKED`/`WORKING`/`SPINNERS` constants in `src/classify.rs` to match the real captures (keep existing unit tests green). Re-run until all pass.

- [ ] **Step 4: Commit**

```bash
git add tests/fixtures src/classify.rs
git commit -m "test: real claude/codex pane fixtures for classifier"
```

---

### Task 9: Final verification

- [ ] **Step 1: Full check**

Run: `cargo test && cargo build --release 2>&1 | grep -iE 'warn|error'`
Expected: all tests pass, no warnings.

- [ ] **Step 2: Re-run manual checklist items 1, 2, 5, 6 from Task 7**

- [ ] **Step 3: Commit any remaining changes**

```bash
git status --short   # should be clean; commit anything pending
```
