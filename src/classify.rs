use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentState {
    Working,
    Blocked,
    Idle,
    /// Was working, now idle. Set by the app from state transitions;
    /// `classify()` never returns it (not detectable from pane text alone).
    Done,
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
const CLAUDE_GLYPHS: &[char] = &['·', '✢', '✳', '✶', '✻', '✽'];

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
    segs.retain(|s| !s.contains('"'));
    segs.iter_mut().for_each(|s| *s = s.trim().to_string());
    segs
}

fn starts_with(seg: &str, marks: &[char]) -> bool {
    seg.chars().next().is_some_and(|c| marks.contains(&c))
}

/// A spinner leads its region; a glyph inside running text is prose or code.
/// The interrupt hint needs no anchor — it only ever appears while a turn is
/// live, and quoted mentions of it are already dropped with their region.
fn is_working(seg: &str) -> bool {
    starts_with(seg, BRAILLE)
        || (starts_with(seg, CLAUDE_GLYPHS) && seg.contains('…'))
        || seg.to_lowercase().contains(INTERRUPT)
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
    if low.contains("to navigate") && low.contains("enter to select") && low.contains("esc to cancel")
    {
        return true;
    }
    if !starts_with(seg, CARETS) {
        return false;
    }
    let rest = seg.trim_start_matches(|c| CARETS.contains(&c)).trim_start();
    let rest = rest.to_lowercase();
    let numbered = rest
        .strip_prefix("1. yes")
        .is_some_and(|tail| !tail.starts_with(|c: char| c.is_alphanumeric()));
    numbered || rest.starts_with("yes (y")
}

/// Agents announce state through the terminal title (OSC 0/2), which tmux
/// exposes as `pane_title`: a leading braille spinner frame means an active
/// turn (claude and codex), and codex titles a pending approval "Action
/// Required". The agent writes the title itself — transcript text can't
/// forge it — so it outranks anything scraped off the screen. No braille
/// title is not evidence of idle: a default title is just the hostname, and
/// an agent nested in nvim may not reach the terminal title at all.
pub fn classify_pane(pane_text: &str, title: &str) -> AgentState {
    if title.to_lowercase().contains("action required") {
        return AgentState::Blocked;
    }
    let mut chars = title.chars();
    if matches!(chars.next(), Some('\u{2800}'..='\u{28FF}')) && chars.next() == Some(' ') {
        return AgentState::Working;
    }
    classify(pane_text)
}

/// State pushed by the agent's own lifecycle hooks via the `@agent_state`
/// tmux pane option. Ground truth when present — the agent reports its own
/// transitions — so it outranks scraping; panes without hooks yield `None`
/// (empty or unrecognized value) and fall back to the classifier.
pub fn hook_state(value: &str) -> Option<AgentState> {
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
pub fn is_transcript_view(pane_text: &str) -> bool {
    let lines: Vec<&str> = pane_text.lines().collect();
    let tail = &lines[lines.len().saturating_sub(TAIL_LINES)..];
    tail.iter()
        .any(|l| !l.contains('"') && l.to_lowercase().contains("showing detailed transcript"))
}

pub fn classify(pane_text: &str) -> AgentState {
    let lines: Vec<&str> = pane_text.lines().collect();
    let tail = &lines[lines.len().saturating_sub(TAIL_LINES)..];
    let rules = rule_columns(tail);
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
    fn blocked_on_codex_approval() {
        let text = "codex wants to run: rm -rf build\nAllow command?\n> Yes (y)";
        assert_eq!(classify(text), AgentState::Blocked);
    }

    #[test]
    fn blocked_on_tall_dialog_past_tail_window() {
        // A long command preview pushes the question off the window; only the
        // trailing choice needs to survive.
        let mut text = String::new();
        for i in 0..30 {
            text.push_str(&format!("preview line {i}\n"));
        }
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
        assert!(is_transcript_view("line\nShowing detailed transcript · ctrl+o to toggle"));
        assert!(!is_transcript_view("│ >    │\n? for shortcuts"));
        // Quoted mention (source/diff) is not the viewer.
        assert!(!is_transcript_view("contains(\"showing detailed transcript\")"));
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



