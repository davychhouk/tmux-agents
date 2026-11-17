use std::collections::HashMap;
use std::io;
use std::time::{Duration, Instant};

use ratatui::crossterm::event::{
    self, DisableFocusChange, EnableFocusChange, Event, KeyCode, KeyEventKind, KeyModifiers,
};
use ratatui::crossterm::execute;
use ratatui::widgets::ListState;

use crate::classify::{AgentState, resolve_state};
use crate::detect::{AgentKind, find_agent, ps_snapshot};
use crate::{tmux, ui};

/// Cap on how long the Working→Done debounce (see `next_state`) can lag a
/// slow user-configured `@agents-interval` — an agent awaiting its second
/// confirming Idle read is rechecked on this cadence instead of waiting out
/// the full interval.
const CONFIRM_INTERVAL: Duration = Duration::from_secs(2);

pub struct Agent {
    pub pane: tmux::Pane,
    pub kind: AgentKind,
    pub state: AgentState,
    /// Unsmoothed `resolve_state` result, carried to next tick to debounce
    /// the displayed state.
    raw: AgentState,
}

pub enum Row {
    Header(String),
    Agent(usize),
}

/// Idle after Working means the agent finished a task → Done. Done sticks
/// until the agent starts working again (or blocks). A single-tick Idle
/// reading right after Working is held over as Working instead of flashing
/// Done — a real stop reads Idle again next tick and confirms the flip.
fn next_state(
    raw: AgentState,
    prev_state: Option<AgentState>,
    prev_raw: Option<AgentState>,
) -> AgentState {
    use AgentState::{Done, Idle, Working};
    match raw {
        Idle => match prev_state {
            Some(Working | Done) if prev_raw == Some(Idle) => Done,
            Some(s @ (Working | Done)) => s,
            _ => Idle,
        },
        other => other,
    }
}

/// Record this tick's states/raws, keeping prior entries for panes absent
/// this tick (a transient capture/detect miss shouldn't wipe debounce
/// history) instead of rebuilding the maps from scratch.
// ponytail: entries for panes that close permanently linger for the sidebar's lifetime; fine, tmux pane ids aren't reused within a server run
fn record_history(
    states: &mut HashMap<String, AgentState>,
    raw: &mut HashMap<String, AgentState>,
    agents: &[Agent],
) {
    for a in agents {
        states.insert(a.pane.id.clone(), a.state);
        raw.insert(a.pane.id.clone(), a.raw);
    }
}

// ponytail: synchronous fork-per-pane each tick; fine for a handful of agents, batch/async if pane count grows
fn refresh(
    prev_states: &HashMap<String, AgentState>,
    prev_raw: &HashMap<String, AgentState>,
) -> io::Result<Vec<Agent>> {
    let panes = tmux::list_panes()?;
    let procs = ps_snapshot()?;
    let own = std::env::var("TMUX_PANE").unwrap_or_default();
    let mut agents: Vec<Agent> = panes
        .into_iter()
        .filter(|p| p.id != own)
        .filter_map(|pane| {
            let text = tmux::capture(&pane.id).ok()?;
            let kind = find_agent(&procs, pane.pid).or_else(|| AgentKind::from_pane_text(&text))?;
            let raw = resolve_state(
                &text,
                &pane.title,
                &pane.hook_state,
                prev_raw.get(&pane.id).copied(),
            );
            let state = next_state(
                raw,
                prev_states.get(&pane.id).copied(),
                prev_raw.get(&pane.id).copied(),
            );
            Some(Agent {
                pane,
                kind,
                state,
                raw,
            })
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

fn row_for_pane(rows: &[Row], agents: &[Agent], pane_id: &str) -> Option<usize> {
    rows.iter()
        .position(|r| matches!(r, Row::Agent(i) if agents[*i].pane.id == pane_id))
}

fn select_own_window(rows: &[Row], agents: &[Agent], list_state: &mut ListState) {
    if let Ok((session, window)) = tmux::own_window()
        && let Some(a) = agents
            .iter()
            .find(|a| a.pane.session == session && a.pane.window_index == window)
    {
        list_state.select(row_for_pane(rows, agents, &a.pane.id));
    }
}

pub fn run() -> io::Result<()> {
    let interval: u64 = tmux::option("@agents-interval", "1").parse().unwrap_or(1);
    let interval = Duration::from_secs(interval.max(1));

    let mut terminal = ratatui::init();
    let _ = execute!(io::stdout(), EnableFocusChange);
    let result = event_loop(&mut terminal, interval);
    let _ = execute!(io::stdout(), DisableFocusChange);
    ratatui::restore();
    result
}

fn event_loop(terminal: &mut ratatui::DefaultTerminal, interval: Duration) -> io::Result<()> {
    let mut prev_states = HashMap::new();
    let mut prev_raw = HashMap::new();
    let mut agents = refresh(&prev_states, &prev_raw)?;
    let mut rows = build_rows(&agents);
    let mut list_state = ListState::default();
    let mut last_tick = Instant::now();
    let start = Instant::now();
    // tmux doesn't send focus-in for a pane that spawns active (toggle's
    // split-window), so seed from tmux's own state.
    let mut focused = tmux::own_pane_active();
    if focused {
        select_own_window(&rows, &agents, &mut list_state);
    }

    loop {
        let spin = (start.elapsed().as_millis() / 100) as usize;
        terminal.draw(|f| ui::render(f, &agents, &rows, &mut list_state, spin, focused))?;

        let pending_confirmation = agents
            .iter()
            .any(|a| a.raw == AgentState::Idle && matches!(a.state, AgentState::Working));
        let refresh_interval = if pending_confirmation {
            interval.min(CONFIRM_INTERVAL)
        } else {
            interval
        };

        let mut timeout = refresh_interval.saturating_sub(last_tick.elapsed());
        if agents
            .iter()
            .any(|a| matches!(a.state, AgentState::Working))
        {
            timeout = timeout.min(Duration::from_millis(100));
        }
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
                        if let Some(Row::Agent(i)) = list_state.selected().and_then(|s| rows.get(s))
                        {
                            let _ = tmux::jump(&agents[*i].pane);
                        }
                    }
                    _ => {}
                },
                // Land the selection on this window's agent when the user
                // focuses the sidebar.
                Event::FocusGained => {
                    focused = true;
                    select_own_window(&rows, &agents, &mut list_state);
                }
                Event::FocusLost => focused = false,
                _ => {}
            }
        }

        if last_tick.elapsed() >= refresh_interval {
            let selected_pane = match list_state.selected().and_then(|s| rows.get(s)) {
                Some(Row::Agent(i)) => Some(agents[*i].pane.id.clone()),
                _ => None,
            };
            record_history(&mut prev_states, &mut prev_raw, &agents);
            // Server gone → refresh errors → exit cleanly.
            agents = refresh(&prev_states, &prev_raw)?;
            rows = build_rows(&agents);
            if let Some(id) = selected_pane {
                list_state.select(
                    row_for_pane(&rows, &agents, &id).or_else(|| next_agent_row(&rows, None, 1)),
                );
            }
            last_tick = Instant::now();
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::classify::AgentState;
    use crate::detect::AgentKind;
    use crate::tmux::Pane;

    pub(crate) fn agent(session: &str, window: u32) -> Agent {
        Agent {
            pane: Pane {
                session: session.into(),
                window_index: window,
                window_name: format!("w{window}"),
                id: format!("%{window}"),
                pid: 100 + window,
                title: String::new(),
                hook_state: String::new(),
            },
            kind: AgentKind::Claude,
            state: AgentState::Idle,
            raw: AgentState::Idle,
        }
    }

    #[test]
    fn done_transitions() {
        use AgentState::*;
        // Idle confirmed two ticks in a row → Done.
        assert_eq!(next_state(Idle, Some(Working), Some(Idle)), Done);
        assert_eq!(next_state(Idle, Some(Done), Some(Idle)), Done); // sticks
        assert_eq!(next_state(Idle, Some(Idle), Some(Idle)), Idle); // never worked
        assert_eq!(next_state(Idle, None, None), Idle); // fresh sidebar
        assert_eq!(next_state(Working, Some(Done), Some(Idle)), Working); // new task clears
        assert_eq!(next_state(Blocked, Some(Working), Some(Working)), Blocked);
    }

    #[test]
    fn idle_blip_held_as_working_for_one_tick() {
        use AgentState::*;
        // First Idle reading right after Working: displayed state holds,
        // doesn't flash Done.
        assert_eq!(next_state(Idle, Some(Working), Some(Working)), Working);
        // Confirmed on the next tick.
        assert_eq!(next_state(Idle, Some(Working), Some(Idle)), Done);
    }

    #[test]
    fn record_history_survives_transient_pane_dropout() {
        use AgentState::*;
        let mut states = HashMap::new();
        let mut raw = HashMap::new();

        let mut a = agent("a", 1);
        a.state = Working;
        a.raw = Working;
        record_history(&mut states, &mut raw, &[a]);

        // Pane transiently drops out of refresh() (e.g. capture error) —
        // history for it must not be wiped.
        record_history(&mut states, &mut raw, &[]);

        // Reappears reading Idle: still held as Working (blip), not reset
        // to a fresh Idle with no history.
        assert_eq!(
            next_state(Idle, states.get("%1").copied(), raw.get("%1").copied()),
            Working
        );
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

    #[test]
    fn selection_pinned_to_pane_across_refresh() {
        // Select b:2 (pane %2), then agent session "a" disappears.
        let before = vec![agent("a", 1), agent("b", 2)];
        let rows = build_rows(&before); // a-hdr, a:1, b-hdr, b:2
        let selected_id = match rows[3] {
            Row::Agent(i) => before[i].pane.id.clone(),
            Row::Header(_) => unreachable!(),
        };
        let after = vec![agent("b", 2)];
        let rows = build_rows(&after); // b-hdr, b:2
        assert_eq!(row_for_pane(&rows, &after, &selected_id), Some(1));
        // Selected pane gone entirely → no row.
        assert_eq!(row_for_pane(&rows, &after, "%99"), None);
    }
}
