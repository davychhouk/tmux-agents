use std::io;

use crate::tmux::{option, tmux};

pub fn toggle() -> io::Result<()> {
    // All panes, any session/window, with sidebar marker.
    let out = tmux(&["list-panes", "-a", "-F", "#{pane_id}\t#{@agents_sidebar}"])?;
    if let Some(id) = out.lines().find_map(|l| {
        let (id, mark) = l.split_once('\t')?;
        (mark == "1").then(|| id.to_string())
    }) {
        tmux(&["kill-pane", "-t", &id])?;
        return Ok(());
    }

    let width = option("@agents-width", "32");
    let exe = std::env::current_exe()?;
    let exe = exe
        .to_str()
        .ok_or_else(|| io::Error::other("non-utf8 exe path"))?;

    let mut args = vec!["split-window", "-h", "-f", "-l", width.as_str()];
    if option("@agents-position", "right") == "left" {
        args.push("-b");
    }
    args.extend(["-P", "-F", "#{pane_id}", exe]);
    let pane_id = tmux(&args)?;
    tmux(&[
        "set-option",
        "-p",
        "-t",
        pane_id.trim(),
        "@agents_sidebar",
        "1",
    ])?;
    Ok(())
}
