#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentState {
    Working,
    Blocked,
    Idle,
    /// Was working, now idle. Set by the app from state transitions;
    /// `classify()` never returns it (not detectable from pane text alone).
    Done,
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

const BRAILLE: &[char] = &['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏']; // codex

// Claude's glyph persists after a task ("✻ Worked for 1m 9s"), so it only
// means working when the line is an active status ("✻ Philosophising…").
const CLAUDE_GLYPHS: &[char] = &['✳', '✶', '✻', '✽', '✢'];

/// Working-status lines live near the bottom of the pane; lines above are
/// transcript that may *quote* the patterns (e.g. a chat about spinners).
/// Blocked dialogs get the full capture instead: unlike a status glyph, a
/// resolved dialog clears from the screen rather than sitting there stale,
/// so there's no history to guard against — and dialogs can run taller than
/// a fixed tail window.
const TAIL_LINES: usize = 25;

pub fn classify(pane_text: &str) -> AgentState {
    if BLOCKED.iter().any(|p| pane_text.to_lowercase().contains(p)) {
        return AgentState::Blocked;
    }
    let lines: Vec<&str> = pane_text.lines().collect();
    let tail = &lines[lines.len().saturating_sub(TAIL_LINES)..];
    let tail_text = tail.join("\n").to_lowercase();
    if WORKING.iter().any(|p| tail_text.contains(p))
        || tail.iter().any(|l| l.contains(BRAILLE))
        || tail.iter().any(|l| l.contains(CLAUDE_GLYPHS) && l.contains('…'))
    {
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
    fn blocked_on_tall_dialog_past_tail_window() {
        // Diff preview pushes the yes/no prompt beyond TAIL_LINES from the
        // bottom of a 40-line capture.
        let mut text = String::new();
        for i in 0..30 {
            text.push_str(&format!("diff line {i}\n"));
        }
        text.push_str("Allow command?\n> Yes (y)");
        assert_eq!(classify(&text), AgentState::Blocked);
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

    #[test]
    fn scrollback_above_tail_ignored() {
        // Pattern quoted in transcript, pushed above the tail window by
        // newer output; pane bottom is a plain idle prompt.
        let mut text = String::from("✳ Cerebrating… (12s · esc to interrupt)\n");
        text.push_str(&"\n".repeat(TAIL_LINES));
        text.push_str("│ >    │");
        assert_eq!(classify(&text), AgentState::Idle);
    }

    #[test]
    fn idle_on_claude_done_summary() {
        // Static line left behind after a task finishes.
        assert_eq!(classify("✻ Churned for 34s"), AgentState::Idle);
        assert_eq!(classify("✻ Worked for 1m 9s"), AgentState::Idle);
    }

    #[test]
    fn working_on_claude_active_status_without_interrupt_hint() {
        // Narrow pane truncates "esc to interrupt"; glyph mid-line (nvim split).
        let text = "│✻ Philosophising… (21s · ↓ 608 tokens)";
        assert_eq!(classify(text), AgentState::Working);
    }
}
