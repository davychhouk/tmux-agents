use std::io;
use std::process::Command;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pane {
    pub session: String,
    pub window_index: u32,
    pub window_name: String,
    pub id: String,
    pub pid: u32,
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
        "#{session_name}\t#{window_index}\t#{window_name}\t#{pane_id}\t#{pane_pid}",
    ])?;
    Ok(parse_panes(&out))
}

fn parse_panes(out: &str) -> Vec<Pane> {
    out.lines()
        .filter_map(|line| {
            let f: Vec<&str> = line.split('\t').collect();
            let [session, window_index, window_name, id, pid] = f.as_slice() else {
                return None;
            };
            Some(Pane {
                session: session.to_string(),
                window_index: window_index.parse().ok()?,
                window_name: window_name.to_string(),
                id: id.to_string(),
                pid: pid.parse().ok()?,
            })
        })
        .collect()
}

/// Last 40 lines of a pane's visible content.
pub fn capture(pane_id: &str) -> io::Result<String> {
    tmux(&["capture-pane", "-p", "-t", pane_id, "-S", "-40"])
}

/// Global option with default (`show-options -gqv` prints empty when unset).
pub fn option(name: &str, default: &str) -> String {
    match tmux(&["show-options", "-gqv", name]) {
        Ok(v) if !v.trim().is_empty() => v.trim().to_string(),
        _ => default.to_string(),
    }
}

pub fn jump(pane: &Pane) -> io::Result<()> {
    tmux(&["switch-client", "-t", &pane.session])?;
    tmux(&[
        "select-window",
        "-t",
        &format!("{}:{}", pane.session, pane.window_index),
    ])?;
    tmux(&["select-pane", "-t", &pane.id])?;
    Ok(())
}

pub fn resize_self(width: u16) -> io::Result<()> {
    let pane = std::env::var("TMUX_PANE").map_err(io::Error::other)?;
    tmux(&["resize-pane", "-t", &pane, "-x", &width.to_string()])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_pane_lines() {
        let out = "work\t1\tapi server\t%3\t4242\nwork\t2\tshell\t%7\t4300\n";
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
            }
        );
    }

    #[test]
    fn skips_malformed_lines() {
        assert!(parse_panes("garbage\n").is_empty());
    }
}
