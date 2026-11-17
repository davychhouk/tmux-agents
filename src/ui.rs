//! Rendering: the expanded list, the icon-only collapsed strip, and the
//! styling shared between them. Pure view code — all state lives in `app`.

use ratatui::Frame;
use ratatui::layout::{Constraint, Flex, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{List, ListItem, ListState};

use crate::app::{Agent, Row};
use crate::classify::{AgentState, CLAUDE_GLYPHS};

const COLLAPSE_THRESHOLD: u16 = 12;
const SELECTED_BG: Color = Color::Rgb(51, 65, 90);
const SPINNER: [char; 10] = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];

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

pub fn render(
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
                    top.push_str(" · ");
                    top.push_str(title);
                }
                let item = ListItem::new(vec![
                    Line::from(vec![
                        Span::styled(format!(" {icon}"), style),
                        Span::styled(top, kind_style),
                    ]),
                    Line::from(vec![
                        Span::styled(format!("   {}", a.kind.label()), detail_style),
                        Span::styled(format!(" · {}", a.state.label()), style),
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
        let y = u16::try_from(y).unwrap_or(u16::MAX);
        frame.set_cursor_position((list_area.x, list_area.y.saturating_add(y)));
    }
}

/// Slice of `area` with the content block centered; full area when the
/// content doesn't fit (the list then scrolls as before).
fn center_vertically(area: Rect, content_height: usize) -> Rect {
    let height = u16::try_from(content_height).unwrap_or(u16::MAX);
    let [centered] = Layout::vertical([Constraint::Length(height)])
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
        let y = u16::try_from(i).unwrap_or(u16::MAX);
        frame.set_cursor_position((area.x, area.y.saturating_add(y)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::tests::agent;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    /// Render into a `width`-wide buffer; return (y, line text) of each
    /// highlighted row, plus y of every row showing the idle icon.
    fn render_rows(
        width: u16,
        rows: &[Row],
        selected: usize,
        focused: bool,
    ) -> (Vec<(u16, String)>, Vec<u16>) {
        let agents = vec![agent("a", 1), agent("b", 2)];
        let mut terminal = Terminal::new(TestBackend::new(width, 20)).unwrap();
        let mut state = ListState::default().with_selected(Some(selected));
        terminal
            .draw(|f| render(f, &agents, rows, &mut state, 0, focused))
            .unwrap();
        let buf = terminal.backend().buffer();
        let line = |y| (0..width).map(|x| buf[(x, y)].symbol()).collect::<String>();
        let highlighted = (0..20)
            .filter(|&y| (0..width).any(|x| buf[(x, y)].bg == SELECTED_BG))
            .map(|y| (y, line(y)))
            .collect();
        let icons = (0..20).filter(|&y| line(y).contains('○')).collect();
        (highlighted, icons)
    }

    fn rows() -> Vec<Row> {
        vec![
            Row::Header("a".into()),
            Row::Agent(0),
            Row::Header("b".into()),
            Row::Agent(1),
        ]
    }

    #[test]
    fn selection_highlighted_only_while_focused() {
        let (hl, _) = render_rows(40, &rows(), 3, false);
        assert_eq!(hl, []);
        let (hl, _) = render_rows(40, &rows(), 3, true);
        // Agent rows are two lines tall; both carry the highlight.
        assert_eq!(hl.len(), 2);
        assert!(hl[0].1.contains("2:w2"));
    }

    #[test]
    fn collapsed_maps_selected_row_to_agent_icon() {
        let (hl, icons) = render_rows(2, &rows(), 3, true);
        assert_eq!(icons.len(), 2);
        assert_eq!(hl.iter().map(|h| h.0).collect::<Vec<_>>(), [icons[1]]);
        let (hl, _) = render_rows(2, &rows(), 3, false);
        assert_eq!(hl, []);
    }

    #[test]
    fn collapsed_header_selection_highlights_nothing() {
        let (hl, _) = render_rows(2, &rows(), 2, true);
        assert_eq!(hl, []);
    }

    #[test]
    fn clean_title_strips_status_glyph_prefixes() {
        assert_eq!(clean_title("⠐ tmux-agents"), "tmux-agents");
        assert_eq!(clean_title("✳ my-project"), "my-project");
        assert_eq!(clean_title("v ~/p/macdots"), "v ~/p/macdots");
        assert_eq!(clean_title("  "), "");
    }
}
