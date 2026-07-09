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

/// First claude/codex found in the descendant tree of `root` (DFS).
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
