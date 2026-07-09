use std::collections::HashMap;
use std::io;
use std::time::{Duration, Instant};

use ratatui::Frame;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{List, ListItem, ListState};

use crate::classify::{AgentState, classify};
use crate::detect::{AgentKind, find_agent, ps_snapshot};
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

/// Idle after Working means the agent finished a task → Done. Done sticks
/// until the agent starts working again (or blocks).
fn next_state(new: AgentState, prev: Option<AgentState>) -> AgentState {
    match (new, prev) {
        (AgentState::Idle, Some(AgentState::Working | AgentState::Done)) => AgentState::Done,
        _ => new,
    }
}

// ponytail: synchronous fork-per-pane each tick; fine for a handful of agents, batch/async if pane count grows
fn refresh(prev: &HashMap<String, AgentState>) -> io::Result<Vec<Agent>> {
    let panes = tmux::list_panes()?;
    let procs = ps_snapshot()?;
    let own = std::env::var("TMUX_PANE").unwrap_or_default();
    let mut agents: Vec<Agent> = panes
        .into_iter()
        .filter(|p| p.id != own)
        .filter_map(|pane| {
            let text = tmux::capture(&pane.id).ok()?;
            let kind = find_agent(&procs, pane.pid).or_else(|| AgentKind::from_pane_text(&text))?;
            let state = next_state(classify(&text), prev.get(&pane.id).copied());
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

fn row_for_pane(rows: &[Row], agents: &[Agent], pane_id: &str) -> Option<usize> {
    rows.iter()
        .position(|r| matches!(r, Row::Agent(i) if agents[*i].pane.id == pane_id))
}

const SPINNER: [char; 10] = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];

fn state_style(state: AgentState, spin: usize) -> (char, Style) {
    match state {
        AgentState::Working => (
            SPINNER[spin % SPINNER.len()],
            Style::default().fg(Color::Yellow),
        ),
        AgentState::Blocked => (
            '!',
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        ),
        AgentState::Idle => ('●', Style::default().fg(Color::DarkGray)),
        AgentState::Done => ('✓', Style::default().fg(Color::Green)),
    }
}

fn render(frame: &mut Frame, agents: &[Agent], rows: &[Row], list_state: &mut ListState, spin: usize) {
    let area = frame.area();
    if area.width < COLLAPSE_THRESHOLD {
        render_collapsed(frame, agents, area, spin);
        return;
    }
    let [title_area, list_area] =
        Layout::vertical([Constraint::Length(2), Constraint::Min(0)]).areas(area);
    frame.render_widget(
        Line::from(Span::styled(
            " Agents",
            Style::default().add_modifier(Modifier::BOLD),
        )),
        title_area,
    );
    let mut first_header = true;
    let selected = list_state.selected();
    let items: Vec<ListItem> = rows
        .iter()
        .enumerate()
        .map(|(idx, row)| match row {
            Row::Header(session) => {
                let header = Line::from(Span::styled(
                    format!(" {session}"),
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ));
                // Blank line above every session group except the first.
                let lines = if first_header {
                    first_header = false;
                    vec![header]
                } else {
                    vec![Line::default(), header]
                };
                ListItem::new(lines)
            }
            Row::Agent(i) => {
                let a = &agents[*i];
                let (icon, style) = state_style(a.state, spin);
                let (bar, kind_style, name_style) = if selected == Some(idx) {
                    (
                        "▌",
                        Style::default()
                            .fg(Color::White)
                            .add_modifier(Modifier::BOLD),
                        Style::default().fg(Color::White),
                    )
                } else {
                    (
                        " ",
                        Style::default(),
                        Style::default().fg(Color::DarkGray),
                    )
                };
                ListItem::new(Line::from(vec![
                    Span::styled(format!(" {bar}"), Style::default().fg(Color::Cyan)),
                    Span::styled(format!("{icon} "), style),
                    Span::styled(format!("{:<6} ", a.kind.label()), kind_style),
                    Span::styled(
                        format!("{}:{}", a.pane.window_index, a.pane.window_name),
                        name_style,
                    ),
                ]))
            }
        })
        .collect();
    let list = List::new(items);
    frame.render_stateful_widget(list, list_area, list_state);
}

fn render_collapsed(frame: &mut Frame, agents: &[Agent], area: Rect, spin: usize) {
    let items: Vec<ListItem> = agents
        .iter()
        .map(|a| {
            let (icon, style) = state_style(a.state, spin);
            ListItem::new(Line::from(Span::styled(format!(" {icon}"), style)))
        })
        .collect();
    frame.render_widget(List::new(items), area);
}

pub fn run() -> io::Result<()> {
    let interval: u64 = tmux::option("@agents-interval", "1").parse().unwrap_or(1);
    let interval = Duration::from_secs(interval.max(1));
    let width: u16 = tmux::option("@agents-width", "32").parse().unwrap_or(32);
    let collapsed_width: u16 = tmux::option("@agents-collapsed-width", "6")
        .parse()
        .unwrap_or(6);

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
    let mut prev_states = HashMap::new();
    let mut agents = refresh(&prev_states)?;
    let mut rows = build_rows(&agents);
    let mut list_state = ListState::default();
    let mut last_tick = Instant::now();
    let mut expanded = true;
    let start = Instant::now();

    loop {
        let spin = (start.elapsed().as_millis() / 100) as usize;
        terminal.draw(|f| render(f, &agents, &rows, &mut list_state, spin))?;

        let mut timeout = interval.saturating_sub(last_tick.elapsed());
        if agents.iter().any(|a| matches!(a.state, AgentState::Working)) {
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
            let selected_pane = match list_state.selected().and_then(|s| rows.get(s)) {
                Some(Row::Agent(i)) => Some(agents[*i].pane.id.clone()),
                _ => None,
            };
            prev_states = agents
                .iter()
                .map(|a| (a.pane.id.clone(), a.state))
                .collect();
            // Server gone → refresh errors → exit cleanly.
            agents = refresh(&prev_states)?;
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
    fn done_transitions() {
        use AgentState::*;
        assert_eq!(next_state(Idle, Some(Working)), Done); // finished
        assert_eq!(next_state(Idle, Some(Done)), Done); // sticks
        assert_eq!(next_state(Idle, Some(Idle)), Idle); // never worked
        assert_eq!(next_state(Idle, None), Idle); // fresh sidebar
        assert_eq!(next_state(Working, Some(Done)), Working); // new task clears
        assert_eq!(next_state(Blocked, Some(Working)), Blocked);
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
