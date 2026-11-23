use std::collections::HashMap;

use crate::detect::AgentKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentState {
    Working,
    Blocked,
    Idle,
    /// Was working, now idle. Set by the app from state transitions or a
    /// Stop hook; `classify()` never returns it (not detectable from pane
    /// text alone).
    Done,
}

impl AgentState {
    pub fn label(self) -> &'static str {
        match self {
            AgentState::Working => "working",
            AgentState::Blocked => "blocked",
            AgentState::Idle => "idle",
            AgentState::Done => "done",
        }
    }
}

// Match the shape of the agent's live UI, never bare phrases: an agent's
// transcript can *print* any phrase we look for (an editor showing this
// file, or a chat explaining this classifier), and then the classifier reads
// its own patterns back and believes them.

/// Spinner used by older codex builds. 0.143 ships no glyph frames at all
/// (it animates the status text), so codex is caught by [`INTERRUPT`].
const BRAILLE: &[char] = &['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];

/// Both agents footer an in-progress turn with an interrupt hint, and drop it
/// the moment the turn ends. It is codex's only working signal.
const INTERRUPT: &str = "to interrupt";

/// Claude's spinner cycle. Its glyph also leads the finished-task summary
/// ("✻ Cooked for 2m 0s"), so a glyph alone means nothing — the trailing
/// ellipsis of an in-progress gerund ("✻ Bloviating…") is what says working.
pub(crate) const CLAUDE_GLYPHS: &[char] = &['·', '✢', '✳', '✶', '✻', '✽'];

/// Leading marks on the selected entry of a confirmation dialog.
const CARETS: &[char] = &['❯', '›', '>', '▶', '▸'];

/// Live agent UI (status line, input box, confirmation dialog) sits at the
/// bottom of the screen; everything above is transcript. A dialog's choice
/// list is the last thing drawn before input, so a tall dialog still lands
/// inside this window — only its trailing choice has to.
const TAIL_LINES: usize = 25;

/// Chars that can draw a vertical rule between side-by-side regions. An agent
/// in an nvim or tmux split shares its lines with whatever else is on screen,
/// so a captured line is really N independent regions.
const BORDERS: &[char] = &['│', '┃', '┆', '┇', '┊', '┋', '╎', '╏', '║', '▏', '▕'];

/// Lines a border char must span, at one column, to count as a rule.
const MIN_RULE_LINES: usize = 3;

/// Columns holding a real vertical rule. A border char drawn once — inside a
/// diff or a string literal quoting a dialog back at us — is not a rule, and
/// must not carve out a region that escapes the quote check below.
fn rule_columns(lines: &[&str]) -> Vec<usize> {
    let mut counts: HashMap<usize, usize> = HashMap::new();
    for line in lines {
        for (col, c) in line.chars().enumerate() {
            if BORDERS.contains(&c) {
                *counts.entry(col).or_default() += 1;
            }
        }
    }
    counts
        .into_iter()
        .filter(|&(_, n)| n >= MIN_RULE_LINES)
        .map(|(col, _)| col)
        .collect()
}

/// The pieces of `line` belonging to distinct on-screen regions. A region
/// holding a `"` is source or a diff quoting our patterns back at us — live
/// UI text is never drawn wrapped in quote marks — so it is dropped.
fn segments(line: &str, rules: &[usize]) -> Vec<String> {
    // Blank rather than drop, so region indices stay aligned across lines.
    regions(line, rules)
        .into_iter()
        .map(|s| if s.contains('"') { String::new() } else { s })
        .collect()
}

/// `line` cut at rule columns, trimmed, quotes kept: for finding prompt
/// lines, where a quote is just something the user typed.
fn regions(line: &str, rules: &[usize]) -> Vec<String> {
    let mut segs = Vec::new();
    let mut cur = String::new();
    for (col, c) in line.chars().enumerate() {
        if BORDERS.contains(&c) && rules.contains(&col) {
            segs.push(std::mem::take(&mut cur));
        } else {
            cur.push(c);
        }
    }
    segs.push(cur);
    segs.into_iter().map(|s| s.trim().to_string()).collect()
}

fn starts_with(seg: &str, marks: &[char]) -> bool {
    seg.chars().next().is_some_and(|c| marks.contains(&c))
}

/// A spinner leads its region; a glyph inside running text is prose or code.
/// The interrupt hint either ends its footer line (`… · esc to interrupt`)
/// or closes the status parenthetical (`(12s • esc to interrupt)`), which
/// codex can follow with ` · 1 background terminal running …`. Anywhere
/// else it's prose talking about it.
fn is_working(seg: &str) -> bool {
    let low = seg.to_lowercase();
    starts_with(seg, BRAILLE)
        || (starts_with(seg, CLAUDE_GLYPHS) && seg.contains('…'))
        || low.ends_with(INTERRUPT)
        || timed_interrupt_hint(&low)
}

/// `(<elapsed> • esc to interrupt)`: the status parenthetical opens with the
/// turn's timer (`5s`, `1m 05s`). Prose like "(press Esc to interrupt)"
/// doesn't.
fn timed_interrupt_hint(low: &str) -> bool {
    low.match_indices(&format!("{INTERRUPT})")).any(|(end, _)| {
        low[..end]
            .rfind('(')
            .is_some_and(|open| low[open + 1..].starts_with(|c: char| c.is_ascii_digit()))
    })
}

/// Claude's live input box: a `❯` line directly under a horizontal rule, in
/// the same screen region. Proves the agent's UI is visible, so a missing
/// working signal really means idle — not "agent buffer out of view".
fn has_input_box(pane_text: &str) -> bool {
    let tail = tail(pane_text);
    let rules = rule_columns(&tail);
    let rows: Vec<Vec<String>> = tail.iter().map(|l| regions(l, &rules)).collect();
    rows.windows(2).any(|w| {
        w[0].iter()
            .zip(&w[1])
            .any(|(above, seg)| is_hrule(above) && seg.starts_with('❯'))
    })
}

/// Text of the screen region showing `kind`'s own UI, for agents sharing a
/// pane (e.g. two nvim terminal splits). Regions are split at vertical
/// rules and claimed by each agent's prompt line: claude's input box or
/// dialog, codex's `›` composer. `None` when the agent isn't on screen or the claim is
/// ambiguous — the caller then has no screen evidence for it.
pub fn agent_region(pane_text: &str, kind: AgentKind) -> Option<String> {
    let lines: Vec<&str> = pane_text.lines().collect();
    let rules = rule_columns(&lines);
    // Raw text: `classify` drops quoted lines itself when it scans the region.
    let rows: Vec<Vec<String>> = lines.iter().map(|l| regions(l, &rules)).collect();
    let width = rows.iter().map(Vec::len).max().unwrap_or(0);
    let mut mine = (0..width)
        .map(|k| {
            rows.iter()
                .map(|r| r.get(k).map_or("", String::as_str))
                .collect::<Vec<_>>()
                .join("\n")
        })
        .filter(|region| region_owner(region) == Some(kind));
    let region = mine.next()?;
    mine.next().is_none().then_some(region)
}

/// Claude claims a region by its input box, or by a live dialog — its `❯`
/// caret plus a dialog shape (Yes/No choice or select footer), since the
/// dialog replaces the box. Codex by its `›` composer or its banner line;
/// its name elsewhere (transcript, source) proves nothing.
fn region_owner(region: &str) -> Option<AgentKind> {
    let lines: Vec<&str> = region.lines().collect();
    let claude = has_input_box(region)
        || (lines.iter().any(|l| l.starts_with('❯')) && lines.iter().any(|l| is_blocked(l)));
    let codex = lines
        .iter()
        .any(|l| l.starts_with('›') || is_codex_banner(l));
    match (claude, codex) {
        (true, false) => Some(AgentKind::Claude),
        (false, true) => Some(AgentKind::Codex),
        _ => None,
    }
}

/// `>_ OpenAI Codex (v0.171.0)`, bare or inside its box border.
fn is_codex_banner(line: &str) -> bool {
    line.trim_start_matches(|c: char| BORDERS.contains(&c) || c.is_whitespace())
        .to_lowercase()
        .starts_with(">_ openai codex")
}

fn is_hrule(seg: &str) -> bool {
    seg.chars().count() >= 10 && seg.chars().all(|c| c == '─')
}

fn tail(pane_text: &str) -> Vec<&str> {
    let lines: Vec<&str> = pane_text.lines().collect();
    lines[lines.len().saturating_sub(TAIL_LINES)..].to_vec()
}

/// A live dialog. Either the footer of a select dialog (question picker,
/// plan approval — these can lack a "Yes" option entirely, so the footer
/// hints are the only tell), or the selected choice: "❯ 1. Yes",
/// "> Yes (y)". All three footer hints on one line is the dialog's shape;
/// prose restating a hint or two ("use Enter to select…") lacks the
/// navigate hint beside them. The caret is required for choices — without
/// it, ordinary numbered output ("1. yesterday…") at the bottom of an idle
/// agent would read as an approval prompt. "yes" must also end the word,
/// so "yesterday" never matches.
fn is_blocked(seg: &str) -> bool {
    let low = seg.to_lowercase();
    if low.contains("to navigate")
        && low.contains("enter to select")
        && low.contains("esc to cancel")
    {
        return true;
    }
    if !starts_with(seg, CARETS) {
        return false;
    }
    let rest = seg.trim_start_matches(|c| CARETS.contains(&c)).trim_start();
    let rest = rest.to_lowercase();
    // Any numbered yes/no choice: the user can move the caret off "1. Yes".
    let numbered = rest
        .split_once(". ")
        .filter(|(n, _)| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()))
        .is_some_and(|(_, choice)| {
            ["yes", "no"].iter().any(|w| {
                choice
                    .strip_prefix(w)
                    .is_some_and(|tail| !tail.starts_with(|c: char| c.is_alphanumeric()))
            })
        });
    numbered || rest.starts_with("yes (y") || rest.starts_with("no (n")
}

/// Agents announce state through the terminal title (OSC 0/2), which tmux
/// exposes as `pane_title`: a leading braille spinner frame means an active
/// turn (claude and codex), and codex titles a pending approval "Action
/// Required". The agent writes the title itself — transcript text can't
/// forge it — so it outranks anything scraped off the screen. No braille
/// title is not evidence of idle: a default title is just the hostname, and
/// an agent nested in nvim may not reach the terminal title at all.
fn classify_pane(pane_text: &str, title: &str) -> AgentState {
    if title.to_lowercase().contains("action required") {
        return AgentState::Blocked;
    }
    let mut chars = title.chars();
    if matches!(chars.next(), Some('\u{2800}'..='\u{28FF}')) && chars.next() == Some(' ') {
        return AgentState::Working;
    }
    classify(pane_text)
}

/// Full per-pane state decision, combining every signal in priority order.
///
/// `hook` is the `@agent_state` pane option written by the agent's own
/// lifecycle hooks — trusted unless the screen clearly contradicts it:
/// - a live dialog outranks any non-blocked reading: some dialogs (question
///   pickers) emit no hook event;
/// - a stale blocked yields to visible work: nothing resets the hook once the
///   prompt is approved;
/// - a stale working/blocked yields to the idle input box: claude skips its
///   Stop hook when the user interrupts with Esc.
///
/// Overrides use the screen-only classifier — the title spinner can keep
/// running through a mid-turn dialog and would mask it.
///
/// Without a hook, the screen+title classifier decides, holding `held`
/// (the previous reading) while claude's transcript viewer replaces the
/// live screen with history.
pub fn resolve_state(text: &str, title: &str, hook: &str, held: Option<AgentState>) -> AgentState {
    use AgentState::{Blocked, Idle, Working};
    match hook_state(hook) {
        Some(state) if is_transcript_view(text) => state,
        Some(state) => match (state, classify(text)) {
            (_, Blocked) => Blocked,
            (Blocked, Working) => Working,
            (Working | Blocked, Idle) if has_input_box(text) => Idle,
            (state, _) => state,
        },
        None if is_transcript_view(text) => held.unwrap_or(Idle),
        None => classify_pane(text, title),
    }
}

/// State pushed by the agent's own lifecycle hooks via the `@agent_state`
/// tmux pane option. Ground truth when present — the agent reports its own
/// transitions — so it outranks scraping; panes without hooks yield `None`
/// (empty or unrecognized value) and fall back to the classifier.
fn hook_state(value: &str) -> Option<AgentState> {
    match value {
        "working" => Some(AgentState::Working),
        "blocked" => Some(AgentState::Blocked),
        "idle" => Some(AgentState::Idle),
        "done" => Some(AgentState::Done),
        _ => None,
    }
}

/// Claude's ctrl+o transcript viewer replaces the live screen with history;
/// nothing on it reflects the turn's real state, so the caller should hold
/// the last known state while it is up.
fn is_transcript_view(pane_text: &str) -> bool {
    tail(pane_text)
        .iter()
        .any(|l| !l.contains('"') && l.to_lowercase().contains("showing detailed transcript"))
}

fn classify(pane_text: &str) -> AgentState {
    let tail = tail(pane_text);
    let rules = rule_columns(&tail);
    let segs: Vec<String> = tail.iter().flat_map(|l| segments(l, &rules)).collect();

    if segs.iter().any(|s| is_blocked(s)) {
        return AgentState::Blocked;
    }
    if segs.iter().any(|s| is_working(s)) {
        return AgentState::Working;
    }
    AgentState::Idle
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocked_on_claude_permission_prompt() {
        let text = "Bash command\n\nDo you want to proceed?\n❯ 1. Yes\n  2. No";
        assert_eq!(classify(text), AgentState::Blocked);
    }

    #[test]
    fn blocked_with_selection_moved_off_yes() {
        let dialog = |sel: usize| {
            [
                "1. Yes",
                "2. Yes, and don't ask again",
                "3. No, and tell Claude (esc)",
            ]
            .iter()
            .enumerate()
            .map(|(i, c)| format!("{} {c}", if i == sel { "❯" } else { " " }))
            .collect::<Vec<_>>()
            .join("\n")
        };
        for sel in 0..3 {
            let text = format!("✢ Doing… (5s)\nDo you want to proceed?\n{}", dialog(sel));
            assert_eq!(classify(&text), AgentState::Blocked, "selected {sel}");
            // Must not read as working and clear a correct blocked hook.
            assert_eq!(
                resolve_state(&text, "", "blocked", None),
                AgentState::Blocked
            );
        }
        assert_eq!(classify("› No (n)"), AgentState::Blocked);
    }

    #[test]
    fn idle_on_caret_line_that_is_not_a_choice() {
        // Echoed user prompt or numbered output with a caret.
        assert_eq!(classify("❯ 2. nothing else to add"), AgentState::Idle);
        assert_eq!(classify("❯ 1. yesterday's build"), AgentState::Idle);
    }

    #[test]
    fn blocked_on_codex_approval() {
        let text = "codex wants to run: rm -rf build\nAllow command?\n> Yes (y)";
        assert_eq!(classify(text), AgentState::Blocked);
    }

    #[test]
    fn blocked_on_tall_dialog_past_tail_window() {
        // A long command preview pushes the question off the window; only the
        // trailing choice needs to survive.
        let mut text = (0..30).fold(String::new(), |acc, i| acc + &format!("preview line {i}\n"));
        text.push_str("Allow command?\n> Yes (y)\n  No (n)");
        assert_eq!(classify(&text), AgentState::Blocked);
    }

    #[test]
    fn blocked_on_select_dialog_footer_without_yes_option() {
        // Question picker / plan approval: no "Yes" choice, only the footer.
        let text = "Which approach?\n  ❯ Option A\n    Option B\n\
                    ↑/↓ to navigate · Enter to select · Esc to cancel";
        assert_eq!(classify(text), AgentState::Blocked);
    }

    #[test]
    fn idle_on_quoted_select_dialog_footer() {
        let text = "contains = [\"enter to select\", \"esc to cancel\"]";
        assert_eq!(classify(text), AgentState::Idle);
    }

    #[test]
    fn idle_on_prose_restating_footer_hints() {
        // Unquoted transcript prose carrying two hints but not the full
        // three-hint footer shape.
        let text = "Use Enter to select and Esc to cancel when the dialog appears.";
        assert_eq!(classify(text), AgentState::Idle);
    }

    #[test]
    fn blocked_on_codex_action_required_title() {
        assert_eq!(
            classify_pane("│ >    │", "Action Required · codex"),
            AgentState::Blocked
        );
    }

    #[test]
    fn working_on_braille_spinner_title_over_idle_screen() {
        assert_eq!(
            classify_pane("│ >    │\n? for shortcuts", "⠧ my-project"),
            AgentState::Working
        );
    }

    #[test]
    fn working_title_outranks_screen_dialog() {
        // Mirrors herdr's priorities: the self-reported title beats a
        // dialog-shaped scrape (e.g. transcript echoing an old prompt).
        let text = "Do you want to proceed?\n❯ 1. Yes";
        assert_eq!(classify_pane(text, "⠹ repo"), AgentState::Working);
    }

    #[test]
    fn plain_titles_leave_screen_verdict_alone() {
        assert_eq!(classify_pane("⠹ Running tests", ""), AgentState::Working);
        assert_eq!(classify_pane("│ >    │", "myhost.local"), AgentState::Idle);
        // Claude's idle title glyph is not a braille frame.
        assert_eq!(classify_pane("│ >    │", "✳ my-project"), AgentState::Idle);
    }

    #[test]
    fn resolve_prefers_hook_state() {
        assert_eq!(
            resolve_state("│ >    │", "", "working", None),
            AgentState::Working
        );
        assert_eq!(
            resolve_state("⠹ Running tests", "", "done", None),
            AgentState::Done
        );
    }

    #[test]
    fn resolve_screen_dialog_overrides_non_blocked_hook() {
        let dialog = "Do you want to proceed?\n❯ 1. Yes";
        assert_eq!(
            resolve_state(dialog, "", "working", None),
            AgentState::Blocked
        );
        // Spinner title must not mask the dialog (screen-only check).
        assert_eq!(
            resolve_state(dialog, "⠹ repo", "working", None),
            AgentState::Blocked
        );
        // ...but a dialog in transcript history doesn't override.
        let viewing = format!("{dialog}\nShowing detailed transcript · ctrl+o to toggle");
        assert_eq!(
            resolve_state(&viewing, "", "working", None),
            AgentState::Working
        );
    }

    const RULE: &str = "────────────────────";
    /// Claude's live input box after a finished or interrupted turn.
    fn idle_box() -> String {
        format!("✻ Cooked for 12s\n\n{RULE}\n❯ \n{RULE}\n  ⏵⏵ auto mode on (shift+tab to cycle)")
    }
    /// The same box mid-turn: spinner line plus the interrupt footer.
    fn working_box() -> String {
        format!(
            "✢ Doing… (1m 5s)\n\n{RULE}\n❯ \n{RULE}\n  ⏵⏵ auto mode on (shift+tab to cycle) · esc to interrupt"
        )
    }

    #[test]
    fn resolve_interrupted_turn_reads_idle_despite_working_hook() {
        // Esc skips claude's Stop hook, so the hook still says working.
        assert_eq!(
            resolve_state(&idle_box(), "", "working", None),
            AgentState::Idle
        );
    }

    #[test]
    fn resolve_keeps_working_hook_while_screen_shows_work() {
        assert_eq!(
            resolve_state(&working_box(), "", "working", None),
            AgentState::Working
        );
    }

    #[test]
    fn resolve_trusts_hook_when_agent_ui_not_visible() {
        // Agent buffer hidden (e.g. another nvim tab): no input box, so a
        // missing working signal proves nothing.
        assert_eq!(
            resolve_state("fn main() {}\n~\n~", "", "working", None),
            AgentState::Working
        );
        // A past prompt in the transcript isn't the live box.
        assert_eq!(
            resolve_state("❯ fix the bug\n\nDone.", "", "working", None),
            AgentState::Working
        );
    }

    #[test]
    fn resolve_clears_stale_blocked_hook() {
        // Approved: the tool runs, but nothing reset the hook yet.
        assert_eq!(
            resolve_state(&working_box(), "", "blocked", None),
            AgentState::Working
        );
        // Dialog dismissed with Esc: no Stop hook either.
        assert_eq!(
            resolve_state(&idle_box(), "", "blocked", None),
            AgentState::Idle
        );
        // Dialog still up: stays blocked.
        assert_eq!(
            resolve_state("Do you want to proceed?\n❯ 1. Yes", "", "blocked", None),
            AgentState::Blocked
        );
    }

    #[test]
    fn resolve_keeps_done_and_idle_hooks_over_idle_box() {
        assert_eq!(
            resolve_state(&idle_box(), "", "done", None),
            AgentState::Done
        );
        assert_eq!(
            resolve_state(&idle_box(), "", "idle", None),
            AgentState::Idle
        );
    }

    /// Two nvim splits side by side, separated by a `│` rule.
    fn split(left: &str, right: &str) -> String {
        let (l, r): (Vec<&str>, Vec<&str>) = (left.lines().collect(), right.lines().collect());
        (0..l.len().max(r.len()))
            .map(|i| {
                format!(
                    "{:<45}│{}",
                    l.get(i).copied().unwrap_or(""),
                    r.get(i).copied().unwrap_or("")
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    const CODEX_WORKING: &str = "• Ran cargo test\n\nWorking (5s • esc to interrupt)\n\n› Ask Codex to do anything\n\n  ? for shortcuts";

    #[test]
    fn region_attributes_each_split_to_its_agent() {
        let text = split(CODEX_WORKING, &idle_box());
        let claude = agent_region(&text, AgentKind::Claude).unwrap();
        let codex = agent_region(&text, AgentKind::Codex).unwrap();
        assert_eq!(classify(&claude), AgentState::Idle);
        assert_eq!(classify(&codex), AgentState::Working);
        // The other agent's spinner/footer isn't in this agent's region.
        assert!(!claude.contains("Working"));
        assert!(!codex.contains("Cooked"));
    }

    #[test]
    fn region_none_when_agent_not_on_screen() {
        // Codex's buffer hidden; only nvim's dashboard beside claude.
        let text = split("  Find File   f\n  Quit        q\n\n", &working_box());
        assert!(agent_region(&text, AgentKind::Codex).is_none());
        assert!(agent_region(&text, AgentKind::Claude).is_some());
    }

    #[test]
    fn region_ignores_shell_prompt_that_looks_like_claude() {
        // A starship-style `❯` shell prompt isn't claude's input box.
        let text = split("~/p/repo\n❯ cargo test\n\n", CODEX_WORKING);
        assert!(agent_region(&text, AgentKind::Claude).is_none());
        assert!(agent_region(&text, AgentKind::Codex).is_some());
    }

    #[test]
    fn region_claims_claude_permission_dialog() {
        // The dialog replaces claude's input box.
        let dialog = "Bash command\n\nDo you want to proceed?\n❯ 1. Yes\n  2. No";
        let text = split(CODEX_WORKING, dialog);
        let claude = agent_region(&text, AgentKind::Claude).unwrap();
        assert_eq!(resolve_state(&claude, "", "", None), AgentState::Blocked);
        // Codex's approval caret is `›`: still codex's, not claude's.
        let codex_dialog = "Allow command?\n› 1. Yes, proceed (y)\n  2. No (esc)";
        let text = split(codex_dialog, &idle_box());
        let codex = agent_region(&text, AgentKind::Codex).unwrap();
        assert_eq!(classify(&codex), AgentState::Blocked);
        assert!(agent_region(&text, AgentKind::Claude).is_some());
    }

    #[test]
    fn region_claims_claude_question_picker() {
        // No Yes/No label: the caret plus the dialog footer claim it.
        let picker = "Which approach?\n  ❯ Option A\n    Option B\n\
                      ↑/↓ to navigate · Enter to select · Esc to cancel";
        let text = split(CODEX_WORKING, picker);
        let claude = agent_region(&text, AgentKind::Claude).unwrap();
        assert_eq!(resolve_state(&claude, "", "", None), AgentState::Blocked);
    }

    #[test]
    fn region_codex_claimed_by_banner_not_by_its_name_in_text() {
        // Claude's transcript mentioning codex doesn't make the region codex's.
        let claude = format!("The label is \"OpenAI Codex\".\n{}", working_box());
        let text = split("~\n~\n~", &claude);
        let claude = agent_region(&text, AgentKind::Claude).unwrap();
        assert_eq!(classify(&claude), AgentState::Working);
        // The real banner (inside its box) still claims codex's region.
        let banner = "╭──────────────────────────────╮\n\
                      │ >_ OpenAI Codex (v0.171.0)   │\n\
                      │ model: gpt-5                 │\n\
                      ╰──────────────────────────────╯";
        let text = split(banner, &idle_box());
        assert!(agent_region(&text, AgentKind::Codex).is_some());
    }

    #[test]
    fn region_survives_quotes_typed_in_prompt() {
        let codex = CODEX_WORKING.replace("Ask Codex to do anything", "explain \"foo\"");
        let claude = working_box().replace("❯ ", "❯ explain \"foo\"");
        let text = split(&codex, &claude);
        let codex = agent_region(&text, AgentKind::Codex).unwrap();
        let claude = agent_region(&text, AgentKind::Claude).unwrap();
        assert_eq!(classify(&codex), AgentState::Working);
        assert_eq!(classify(&claude), AgentState::Working);
    }

    #[test]
    fn region_none_when_both_agents_claim_one_region() {
        let both = format!("{}\n{CODEX_WORKING}", idle_box());
        assert!(agent_region(&both, AgentKind::Claude).is_none());
        assert!(agent_region(&both, AgentKind::Codex).is_none());
    }

    #[test]
    fn resolve_finds_idle_box_beside_editor_split() {
        let text = idle_box()
            .lines()
            .enumerate()
            .map(|(i, r)| format!("{:<20}│{r}", format!("let x{i} = 1;")))
            .collect::<Vec<_>>()
            .join("\n");
        assert_eq!(resolve_state(&text, "", "working", None), AgentState::Idle);
    }

    #[test]
    fn resolve_holds_previous_reading_during_transcript_view() {
        let text = "Showing detailed transcript · ctrl+o to toggle";
        assert_eq!(
            resolve_state(text, "", "", Some(AgentState::Working)),
            AgentState::Working
        );
        assert_eq!(resolve_state(text, "", "", None), AgentState::Idle);
    }

    #[test]
    fn resolve_falls_back_to_screen_and_title() {
        assert_eq!(
            resolve_state("│ >    │", "⠧ my-project", "", None),
            AgentState::Working
        );
        assert_eq!(resolve_state("│ >    │", "", "", None), AgentState::Idle);
    }

    #[test]
    fn hook_state_parses_known_values_only() {
        assert_eq!(hook_state("working"), Some(AgentState::Working));
        assert_eq!(hook_state("blocked"), Some(AgentState::Blocked));
        assert_eq!(hook_state("idle"), Some(AgentState::Idle));
        assert_eq!(hook_state("done"), Some(AgentState::Done));
        assert_eq!(hook_state(""), None);
        assert_eq!(hook_state("Working"), None);
    }

    #[test]
    fn transcript_view_detected() {
        assert!(is_transcript_view(
            "line\nShowing detailed transcript · ctrl+o to toggle"
        ));
        assert!(!is_transcript_view("│ >    │\n? for shortcuts"));
        // Quoted mention (source/diff) is not the viewer.
        assert!(!is_transcript_view(
            "contains(\"showing detailed transcript\")"
        ));
    }

    #[test]
    fn blocked_wins_over_working() {
        let text = "✳ Cerebrating… (12s)\nDo you want to proceed?\n❯ 1. Yes";
        assert_eq!(classify(text), AgentState::Blocked);
    }

    #[test]
    fn blocked_choice_survives_quoted_source_beside_it() {
        // An editor showing quoted source shares each line with the agent's
        // dialog. The rule between them lets each region be judged alone.
        let text = "let a = \"x\";      │ Bash command       \n\
                    let b = \"y\";      │ Do you want to?    \n\
                    let c = \"z\";      │ ❯ 1. Yes           \n\
                    let d = \"w\";      │   2. No            ";
        assert_eq!(classify(text), AgentState::Blocked);
    }

    #[test]
    fn idle_on_diff_line_embedding_a_dialog_between_border_chars() {
        // A diff of this file's own tests. The border chars appear once, so
        // they are not a rule and cannot carve the quote out of the region.
        let text = "+        let text = \"  let msg = x;  │ ❯ 1. Yes │\";";
        assert_eq!(classify(text), AgentState::Idle);
    }

    #[test]
    fn working_on_claude_spinner() {
        assert_eq!(
            classify("✳ Cerebrating… (12s · esc to interrupt)"),
            AgentState::Working
        );
        // The cycle passes through a plain middle dot.
        assert_eq!(
            classify("· Bloviating… (5m 36s · ↓ 16.9k tokens)"),
            AgentState::Working
        );
    }

    #[test]
    fn working_on_braille_spinner() {
        assert_eq!(classify("⠹ Running tests"), AgentState::Working);
    }

    #[test]
    fn idle_on_parenthetical_prose_about_the_interrupt_hint() {
        let prose = "While working (press Esc to interrupt), Codex displays a timer.";
        assert_eq!(classify(prose), AgentState::Idle);
        // ...and it must not block correcting a stale working hook.
        let text = format!("{prose}\n{}", idle_box());
        assert_eq!(resolve_state(&text, "", "working", None), AgentState::Idle);
    }

    #[test]
    fn working_on_status_parenthetical_with_elapsed_time() {
        for line in [
            "Working (1m 05s • esc to interrupt) · /ps to view",
            "✳ Cerebrating… (12s · ↓ 2.0k tokens · esc to interrupt)",
        ] {
            assert_eq!(classify(line), AgentState::Working, "{line}");
        }
    }

    #[test]
    fn working_on_codex_status_with_inline_message() {
        // Codex appends background-terminal info after the hint.
        assert_eq!(
            classify(
                "Working (5s • esc to interrupt) · 1 background terminal running · /ps to view · /stop to close"
            ),
            AgentState::Working
        );
    }

    #[test]
    fn working_on_codex_interrupt_hint_without_any_glyph() {
        // codex 0.143 animates its status text, so no spinner char appears.
        assert_eq!(
            classify("Working (12s • Esc to interrupt)"),
            AgentState::Working
        );
        assert_eq!(
            classify("  send message to queue  Esc to interrupt"),
            AgentState::Working
        );
    }

    #[test]
    fn idle_on_quoted_mention_of_the_interrupt_hint() {
        let text = "matching \"esc to interrupt\", \"ctrl+c to interrupt\"";
        assert_eq!(classify(text), AgentState::Idle);
    }

    #[test]
    fn idle_on_prose_mentioning_the_interrupt_hint() {
        // Unquoted mentions in transcript prose; only the footer ends a line
        // with the hint.
        assert_eq!(
            classify("Press `esc to interrupt` the agent at any time."),
            AgentState::Idle
        );
        assert_eq!(
            classify("the footer shows esc to interrupt while a turn runs"),
            AgentState::Idle
        );
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

    #[test]
    fn idle_on_claude_done_summary() {
        // Glyph persists after the task; no ellipsis means no longer working.
        assert_eq!(classify("✻ Churned for 34s"), AgentState::Idle);
        assert_eq!(classify("✻ Worked for 1m 9s"), AgentState::Idle);
    }

    #[test]
    fn idle_on_numbered_transcript_output() {
        // Ordinary numbered output at the bottom of an idle agent: no caret.
        assert_eq!(classify("1. Yes, that works\n2. No"), AgentState::Idle);
        assert_eq!(classify("❯ 1. yesterday's run failed"), AgentState::Idle);
        assert_eq!(classify("1. yesterday I ran the tests"), AgentState::Idle);
    }

    #[test]
    fn idle_when_user_types_yes_at_the_prompt() {
        // Bare "yes" in the input box is not a dialog choice.
        assert_eq!(classify("│ ❯ yes │"), AgentState::Idle);
    }

    #[test]
    fn scrollback_above_tail_ignored() {
        let mut text = String::from("✳ Cerebrating… (12s · esc to interrupt)\n");
        text.push_str(&"\n".repeat(TAIL_LINES));
        text.push_str("│ >    │");
        assert_eq!(classify(&text), AgentState::Idle);
    }

    // Shapes below come from real `capture-pane -J` output of agents running
    // in an nvim terminal split, where one line carries both the editor
    // buffer and the agent's UI, separated by box-drawing rules.

    #[test]
    fn working_when_agent_shares_line_with_editor_buffer() {
        let text = "  30  fn segments()  │                       \n\
                      31  let x = 1;     │ Ran 2 shell commands  \n\
                      32  let y = 2;     │                       \n\
                      33  let z = 3;     │· Bloviating… (5m 36s) ";
        assert_eq!(classify(text), AgentState::Working);
    }

    #[test]
    fn idle_on_done_summary_beside_editor_buffer() {
        // Same layout, but the status is a finished-task summary: no ellipsis.
        let text = "  30  fn segments()  │                       \n\
                      31  let x = 1;     │ Ran 2 shell commands  \n\
                      32  let y = 2;     │                       \n\
                      33  let z = 3;     │✻ Cooked for 2m 0s     ";
        assert_eq!(classify(text), AgentState::Idle);
    }

    #[test]
    fn idle_on_source_code_defining_the_glyph_arrays() {
        // classify.rs open in an editor: its own constants hold every glyph.
        let text = "  │   18   const CLAUDE_GLYPHS: &[char] = &['✳', '✶', '✻'];│";
        assert_eq!(classify(text), AgentState::Idle);
        assert_eq!(
            classify("const BRAILLE: &[char] = &['⠋', '⠙', '⠹'];"),
            AgentState::Idle
        );
    }

    #[test]
    fn idle_on_doc_comment_quoting_glyph_and_ellipsis() {
        let text = "// working when the line is an active status (\"✻ Philosophising…\").";
        assert_eq!(classify(text), AgentState::Idle);
    }

    #[test]
    fn idle_on_prose_listing_the_blocked_patterns() {
        // An agent explaining this very classifier, unquoted. Real capture.
        let text = "  - Blocked: sees prompts like do you want, would you like, \
                    allow command, yes (y), ❯ 1. yes.";
        assert_eq!(classify(text), AgentState::Idle);
    }

    #[test]
    fn idle_on_source_string_literal_holding_a_blocked_phrase() {
        let text = "text.push_str(\"Allow command?\\n> Yes (y)\");";
        assert_eq!(classify(text), AgentState::Idle);
    }

    #[test]
    fn idle_on_wrapped_diff_line_starting_with_a_choice() {
        // A soft-wrapped diff of our own fixture can begin mid-literal.
        assert_eq!(classify("> Yes (y)\");"), AgentState::Idle);
    }
}
