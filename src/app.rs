use std::collections::HashMap;
use std::io;
use std::time::{Duration, Instant};

use ratatui::Frame;
use ratatui::crossterm::event::{
    self, DisableFocusChange, EnableFocusChange, Event, KeyCode, KeyEventKind, KeyModifiers,
};
use ratatui::crossterm::execute;
use ratatui::layout::{Constraint, Flex, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{List, ListItem, ListState};

use crate::classify::{
    AgentState, CLAUDE_GLYPHS, classify, classify_pane, hook_state, is_transcript_view,
};
use crate::detect::{AgentKind, find_agent, ps_snapshot};
use crate::tmux;

const COLLAPSE_THRESHOLD: u16 = 12;
/// Cap on how long the Working→Done debounce (see `next_state`) can lag a
/// slow user-configured `@agents-interval` — an agent awaiting its second
/// confirming Idle read is rechecked on this cadence instead of waiting out
/// the full interval.
const CONFIRM_INTERVAL: Duration = Duration::from_secs(2);
const SELECTED_BG: Color = Color::Rgb(51, 65, 90);

pub struct Agent {
    pub pane: tmux::Pane,
    pub kind: AgentKind,
    pub state: AgentState,
    /// Unsmoothed classify() result, carried to next tick to debounce state.
    raw: AgentState,
}

enum Row {
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
    use AgentState::*;
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
            let raw = match hook_state(&pane.hook_state) {
                // Hook-reported state is ground truth, except a live dialog
                // on screen outranks a non-blocked reading: some dialogs
                // (question pickers) emit no hook event, so the last hook
                // write still says working. Screen-only classify() here —
                // the title spinner can keep running through a mid-turn
                // dialog, and classify_pane would let it mask the dialog.
                Some(state) => {
                    if state != AgentState::Blocked
                        && !is_transcript_view(&text)
                        && classify(&text) == AgentState::Blocked
                    {
                        AgentState::Blocked
                    } else {
                        state
                    }
                }
                // Transcript viewer shows history, not live state: hold the
                // last reading instead of classifying stale text.
                None if is_transcript_view(&text) => {
                    prev_raw.get(&pane.id).copied().unwrap_or(AgentState::Idle)
                }
                None => classify_pane(&text, &pane.title),
            };
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

/// Pane title minus agent status prefixes: spinner frames and claude's
/// glyph cycle lead the OSC title while a turn runs, and the state column
/// already shows that.
fn clean_title(title: &str) -> &str {
    title
        .trim_start_matches(|c: char| {
            matches!(c, '\u{2800}'..='\u{28FF}') || CLAUDE_GLYPHS.contains(&c) || c.is_whitespace()
        })
        .trim_end()
}

fn row_for_pane(rows: &[Row], agents: &[Agent], pane_id: &str) -> Option<usize> {
    rows.iter()
        .position(|r| matches!(r, Row::Agent(i) if agents[*i].pane.id == pane_id))
}

const SPINNER: [char; 10] = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];

fn state_label(state: AgentState) -> &'static str {
    match state {
        AgentState::Working => "working",
        AgentState::Blocked => "blocked",
        AgentState::Idle => "idle",
        AgentState::Done => "done",
    }
}

fn state_style(state: AgentState, spin: usize) -> (char, Style) {
    match state {
        AgentState::Working => (
            SPINNER[spin % SPINNER.len()],
            Style::default().fg(Color::Yellow),
        ),
        AgentState::Blocked => (
            '?',
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        ),
        AgentState::Idle => ('○', Style::default().fg(Color::DarkGray)),
        AgentState::Done => ('✓', Style::default().fg(Color::Green)),
    }
}

fn render(
    frame: &mut Frame,
    agents: &[Agent],
    rows: &[Row],
    list_state: &mut ListState,
    spin: usize,
    focused: bool,
) {
    let area = frame.area();
    if area.width < COLLAPSE_THRESHOLD {
        render_collapsed(frame, agents, rows, list_state, area, spin, focused);
        return;
    }
    let [title_area, list_area, hints_area] = Layout::vertical([
        Constraint::Length(2),
        Constraint::Min(0),
        Constraint::Length(1),
    ])
    .areas(area);
    frame.render_widget(
        Line::from(Span::styled(
            " Agents",
            Style::default().add_modifier(Modifier::BOLD),
        )),
        title_area,
    );
    frame.render_widget(
        Line::from(Span::styled(
            " j/k move · ⏎ jump · q quit",
            Style::default().fg(Color::DarkGray),
        )),
        hints_area,
    );
    let mut first_header = true;
    let selected = if focused { list_state.selected() } else { None };
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
                let is_selected = selected == Some(idx);
                let (kind_style, detail_style) = if is_selected {
                    (
                        Style::default()
                            .fg(Color::White)
                            .add_modifier(Modifier::BOLD),
                        Style::default().fg(Color::Gray),
                    )
                } else {
                    (Style::default(), Style::default().fg(Color::DarkGray))
                };
                let mut top = format!(" {}:{}", a.pane.window_index, a.pane.window_name);
                let title = clean_title(&a.pane.title);
                if !title.is_empty() {
                    top.push_str(&format!(" · {title}"));
                }
                let item = ListItem::new(vec![
                    Line::from(vec![
                        Span::styled(format!(" {icon}"), style),
                        Span::styled(top, kind_style),
                    ]),
                    Line::from(vec![
                        Span::styled(format!("   {}", a.kind.label()), detail_style),
                        Span::styled(format!(" · {}", state_label(a.state)), style),
                    ]),
                ]);
                if is_selected {
                    item.style(Style::default().bg(SELECTED_BG))
                } else {
                    item
                }
            }
        })
        .collect();
    let heights: Vec<usize> = items.iter().map(ListItem::height).collect();
    let list_area = center_vertically(list_area, heights.iter().sum());
    let list = List::new(items);
    frame.render_stateful_widget(list, list_area, list_state);
    if let Some(sel) = selected {
        let offset = list_state.offset().min(sel);
        let y: usize = heights[offset..sel].iter().sum();
        frame.set_cursor_position((list_area.x, list_area.y + y as u16));
    }
}

/// Slice of `area` with the content block centered; full area when the
/// content doesn't fit (the list then scrolls as before).
fn center_vertically(area: Rect, content_height: usize) -> Rect {
    let [centered] = Layout::vertical([Constraint::Length(content_height as u16)])
        .flex(Flex::Center)
        .areas(area);
    centered
}

fn render_collapsed(
    frame: &mut Frame,
    agents: &[Agent],
    rows: &[Row],
    list_state: &ListState,
    area: Rect,
    spin: usize,
    focused: bool,
) {
    // Selection lives in row-space (headers + agents); icons are agent-space.
    let selected = if focused { list_state.selected() } else { None };
    let selected_agent = selected.and_then(|s| match rows.get(s) {
        Some(Row::Agent(i)) => Some(*i),
        _ => None,
    });
    let items: Vec<ListItem> = agents
        .iter()
        .enumerate()
        .map(|(i, a)| {
            let (icon, style) = state_style(a.state, spin);
            let item = ListItem::new(Line::from(Span::styled(format!(" {icon}"), style)));
            if selected_agent == Some(i) {
                item.style(Style::default().bg(SELECTED_BG))
            } else {
                item
            }
        })
        .collect();
    let area = center_vertically(area, items.len());
    frame.render_widget(List::new(items), area);
    if let Some(i) = selected_agent {
        frame.set_cursor_position((area.x, area.y + i as u16));
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
    // tmux replays the pane's focus state when focus reporting is enabled,
    // so this corrects itself immediately if we spawn focused.
    let mut focused = false;

    loop {
        let spin = (start.elapsed().as_millis() / 100) as usize;
        terminal.draw(|f| render(f, &agents, &rows, &mut list_state, spin, focused))?;

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
                    if let Ok((session, window)) = tmux::own_window() {
                        if let Some(a) = agents
                            .iter()
                            .find(|a| a.pane.session == session && a.pane.window_index == window)
                        {
                            list_state.select(row_for_pane(&rows, &agents, &a.pane.id));
                        }
                    }
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
mod tests {
    use super::*;
    use crate::classify::AgentState;
    use crate::detect::AgentKind;
    use crate::tmux::Pane;

    #[test]
    fn clean_title_strips_status_glyph_prefixes() {
        assert_eq!(clean_title("⠐ tmux-agents"), "tmux-agents");
        assert_eq!(clean_title("✳ my-project"), "my-project");
        assert_eq!(clean_title("v ~/p/macdots"), "v ~/p/macdots");
        assert_eq!(clean_title("  "), "");
    }

    fn agent(session: &str, window: u32) -> Agent {
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
