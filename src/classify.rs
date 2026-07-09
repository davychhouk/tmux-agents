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
    if WORKING.iter().any(|p| text.contains(p)) || pane_text.chars().any(|c| SPINNERS.contains(&c))
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
