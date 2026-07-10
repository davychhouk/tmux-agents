use std::io;

use crate::tmux::{option, tmux};

/// Move the sidebar into the now-active window, keeping its width (and so
/// its expanded/collapsed state). Run from tmux hooks on window/session
/// switches; a no-op when there's no sidebar or it's already there.
pub fn follow() -> io::Result<()> {
    if option("@agents-follow", "on") == "off" {
        return Ok(());
    }
    let out = tmux(&[
        "list-panes",
        "-a",
        "-F",
        "#{pane_id}\t#{pane_width}\t#{session_name}:#{window_index}\t#{@agents_sidebar}",
    ])?;
    let Some((id, width, at)) = out.lines().find_map(|l| {
        let mut f = l.split('\t');
        let (id, w, at, mark) = (f.next()?, f.next()?, f.next()?, f.next()?);
        (mark == "1").then(|| (id.to_string(), w.to_string(), at.to_string()))
    }) else {
        return Ok(());
    };
    // Hook context resolves to the client/session that triggered it.
    let target = tmux(&["display-message", "-p", "#{session_name}:#{window_index}"])?;
    let target = target.trim();
    if target == at {
        return Ok(());
    }
    let mut args = vec![
        "join-pane", "-d", "-h", "-f", "-l", &width, "-s", &id, "-t", target,
    ];
    if option("@agents-position", "right") == "left" {
        args.push("-b");
    }
    // Best-effort: racing with jump()'s own move is fine.
    let _ = tmux(&args);
    Ok(())
}

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
            // Left needs a spare column: the pane border sits on the right,
            // so a 2-wide strip would butt icons against the content.
            let default = if option("@agents-position", "right") == "left" {
                "3"
            } else {
                "2"
            };
            let collapsed = option("@agents-collapsed-width", default);
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
