use std::io;

use crate::tmux::{option, tmux};

pub fn toggle() -> io::Result<()> {
    // All panes, any session/window, with sidebar marker and current width.
    let out = tmux(&[
        "list-panes",
        "-a",
        "-F",
        "#{pane_id}\t#{pane_width}\t#{@agents_sidebar}",
    ])?;
    let width = option("@agents-width", "32");
    if let Some((id, cur)) = out.lines().find_map(|l| {
        let mut f = l.split('\t');
        let (id, cur, mark) = (f.next()?, f.next()?, f.next()?);
        (mark == "1").then(|| (id.to_string(), cur.to_string()))
    }) {
        if option("@agents-toggle", "collapsed") == "hidden" {
            tmux(&["kill-pane", "-t", &id])?;
        } else {
            let collapsed = option("@agents-collapsed-width", "2");
            let target = if cur == collapsed { &width } else { &collapsed };
            tmux(&["resize-pane", "-t", &id, "-x", target])?;
        }
        return Ok(());
    }

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
