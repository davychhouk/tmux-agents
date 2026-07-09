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
        // Interpreters whose real program is their first argument
        // (`node /path/to/claude`), not the interpreter itself.
        const INTERPRETERS: &[&str] =
            &["node", "bun", "deno", "python", "python3", "ruby", "sh", "bash", "zsh", "fish"];

        fn basename(token: &str) -> Option<&str> {
            token.rsplit('/').next()
        }
        fn kind_of(basename: &str) -> Option<AgentKind> {
            match basename {
                "claude" => Some(AgentKind::Claude),
                "codex" => Some(AgentKind::Codex),
                _ => None,
            }
        }

        let mut tokens = command.split_whitespace();
        let argv0 = basename(tokens.next()?)?;
        if let Some(kind) = kind_of(argv0) {
            return Some(kind);
        }
        if INTERPRETERS.contains(&argv0) {
            let script = tokens.find(|t| !t.starts_with('-'))?;
            return kind_of(basename(script)?);
        }
        None
    }

    pub fn from_pane_text(text: &str) -> Option<Self> {
        text.to_ascii_lowercase()
            .contains("openai codex")
            .then_some(AgentKind::Codex)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Proc {
    pub pid: u32,
    pub ppid: u32,
    pub command: String,
}

pub fn ps_snapshot() -> std::io::Result<Vec<Proc>> {
    let out = Command::new("ps")
        .args(["-eo", "pid=,ppid=,command="])
        .output()?;
    if !out.status.success() {
        return Err(std::io::Error::other(
            String::from_utf8_lossy(&out.stderr).into_owned(),
        ));
    }
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

/// First claude/codex found at `root` or in its descendant tree (DFS).
pub fn find_agent(procs: &[Proc], root: u32) -> Option<AgentKind> {
    let mut queue = vec![root];
    while let Some(pid) = queue.pop() {
        if let Some(kind) = procs
            .iter()
            .find(|p| p.pid == pid)
            .and_then(|p| AgentKind::from_command(&p.command))
        {
            return Some(kind);
        }
        queue.extend(procs.iter().filter(|p| p.ppid == pid).map(|p| p.pid));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(pid: u32, ppid: u32, command: &str) -> Proc {
        Proc {
            pid,
            ppid,
            command: command.to_string(),
        }
    }

    #[test]
    fn finds_agent_as_pane_root() {
        let procs = vec![p(10, 1, "codex")];
        assert_eq!(find_agent(&procs, 10), Some(AgentKind::Codex));
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
    fn finds_codex_from_pane_text() {
        assert_eq!(
            AgentKind::from_pane_text(">_ OpenAI Codex (v0.143.0)"),
            Some(AgentKind::Codex)
        );
    }

    #[test]
    fn no_agent() {
        let procs = vec![p(10, 1, "-fish"), p(20, 10, "vim notes.md")];
        assert_eq!(find_agent(&procs, 10), None);
    }

    #[test]
    fn skips_interpreter_flags_before_script() {
        let procs = vec![
            p(10, 1, "fish"),
            p(20, 10, "node --enable-source-maps /path/to/claude"),
        ];
        assert_eq!(find_agent(&procs, 10), Some(AgentKind::Claude));
    }

    #[test]
    fn matches_inline_command_flag() {
        // `bash -lc claude`: claude is bash's -c command string, not a path.
        let procs = vec![p(10, 1, "fish"), p(20, 10, "bash -lc claude")];
        assert_eq!(find_agent(&procs, 10), Some(AgentKind::Claude));
    }

    #[test]
    fn ignores_stray_arg_matching_agent_name() {
        // "codex"/"claude" as a plain argument (not argv0, not an
        // interpreter's script path) must not match.
        let procs = vec![p(10, 1, "fish"), p(20, 10, "cp draft.md codex")];
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
        assert_eq!(
            procs[1],
            Proc {
                pid: 20,
                ppid: 10,
                command: "nvim .".to_string()
            }
        );
    }
}
