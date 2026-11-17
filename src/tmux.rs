use std::io;
use std::process::Command;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pane {
    pub session: String,
    pub window_index: u32,
    pub window_name: String,
    pub id: String,
    pub pid: u32,
    pub title: String,
    /// `@agent_state` pane option, pushed by agent lifecycle hooks.
    /// Empty when no hook has written it.
    pub hook_state: String,
}

/// Run tmux, return stdout. Err if tmux exits nonzero (e.g. server gone).
pub fn tmux(args: &[&str]) -> io::Result<String> {
    let out = Command::new("tmux").args(args).output()?;
    if !out.status.success() {
        return Err(io::Error::other(
            String::from_utf8_lossy(&out.stderr).into_owned(),
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

pub fn list_panes() -> io::Result<Vec<Pane>> {
    let out = tmux(&[
        "list-panes",
        "-a",
        "-F",
        "#{session_name}\t#{window_index}\t#{window_name}\t#{pane_id}\t#{pane_pid}\t#{pane_title}\t#{@agent_state}",
    ])?;
    Ok(parse_panes(&out))
}

fn parse_panes(out: &str) -> Vec<Pane> {
    out.lines()
        .filter_map(|line| {
            let f: Vec<&str> = line.split('\t').collect();
            let [
                session,
                window_index,
                window_name,
                id,
                pid,
                title,
                hook_state,
            ] = f.as_slice()
            else {
                return None;
            };
            Some(Pane {
                session: session.to_string(),
                window_index: window_index.parse().ok()?,
                window_name: window_name.to_string(),
                id: id.to_string(),
                pid: pid.parse().ok()?,
                title: title.to_string(),
                hook_state: hook_state.to_string(),
            })
        })
        .collect()
}

/// Last 40 lines of a pane's visible content. `-J` rejoins soft-wrapped
/// lines so a status line split across screen rows (narrow pane) comes
/// back as one logical line.
pub fn capture(pane_id: &str) -> io::Result<String> {
    tmux(&["capture-pane", "-p", "-J", "-t", pane_id, "-S", "-40"])
}

/// Global option with default (`show-options -gqv` prints empty when unset).
pub fn option(name: &str, default: &str) -> String {
    match tmux(&["show-options", "-gqv", name]) {
        Ok(v) if !v.trim().is_empty() => v.trim().to_string(),
        _ => default.to_string(),
    }
}

/// Whether the sidebar is the active pane of the active window.
pub fn own_pane_active() -> bool {
    let Ok(pane) = std::env::var("TMUX_PANE") else {
        return false;
    };
    tmux(&[
        "display-message",
        "-p",
        "-t",
        &pane,
        "#{?pane_active,#{window_active},0}",
    ])
    .is_ok_and(|s| s.trim() == "1")
}

/// Session and window index the sidebar itself lives in.
pub fn own_window() -> io::Result<(String, u32)> {
    let pane = std::env::var("TMUX_PANE").map_err(io::Error::other)?;
    let out = tmux(&[
        "display-message",
        "-p",
        "-t",
        &pane,
        "#{session_name}\t#{window_index}",
    ])?;
    let out = out.trim();
    let (session, window) = out
        .split_once('\t')
        .ok_or_else(|| io::Error::other("bad display-message output"))?;
    Ok((
        session.to_string(),
        window.parse().map_err(io::Error::other)?,
    ))
}

pub fn jump(pane: &Pane) -> io::Result<()> {
    tmux(&["switch-client", "-t", &pane.session])?;
    let target = format!("{}:{}", pane.session, pane.window_index);
    tmux(&["select-window", "-t", &target])?;
    tmux(&["select-pane", "-t", &pane.id])?;

    // Bring the sidebar along so it stays visible next to the agent.
    let own = std::env::var("TMUX_PANE").map_err(io::Error::other)?;
    if let Ok((session, window)) = own_window()
        && session == pane.session
        && window == pane.window_index
    {
        return Ok(()); // already in the target window
    }
    let width = tmux(&["display-message", "-p", "-t", &own, "#{pane_width}"])?;
    let width = width.trim().to_string();
    // -d keeps focus on the agent pane selected above.
    let mut args = vec![
        "join-pane",
        "-d",
        "-h",
        "-f",
        "-l",
        &width,
        "-s",
        &own,
        "-t",
        &target,
    ];
    if option("@agents-position", "right") == "left" {
        args.push("-b");
    }
    tmux(&args)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_pane_lines() {
        let out =
            "work\t1\tapi server\t%3\t4242\tmy title\tworking\nwork\t2\tshell\t%7\t4300\thost\t\n";
        let panes = parse_panes(out);
        assert_eq!(panes.len(), 2);
        assert_eq!(
            panes[0],
            Pane {
                session: "work".into(),
                window_index: 1,
                window_name: "api server".into(),
                id: "%3".into(),
                pid: 4242,
                title: "my title".into(),
                hook_state: "working".into(),
            }
        );
        assert_eq!(panes[1].hook_state, "");
    }

    #[test]
    fn skips_malformed_lines() {
        assert_eq!(parse_panes("garbage\n"), []);
    }
}
