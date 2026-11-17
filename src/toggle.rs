use std::io;

use crate::tmux::{option, tmux};

#[derive(Debug, PartialEq, Eq)]
struct Sidebar {
    id: String,
    width: String,
    /// `session:window` the sidebar currently lives in.
    at: String,
}

const SIDEBAR_FORMAT: &str =
    "#{pane_id}\t#{pane_width}\t#{session_name}:#{window_index}\t#{@agents_sidebar}";

/// The pane marked `@agents_sidebar` in `list-panes -a -F SIDEBAR_FORMAT`.
fn find_sidebar(out: &str) -> Option<Sidebar> {
    out.lines().find_map(|l| {
        let mut f = l.split('\t');
        let (id, width, at, mark) = (f.next()?, f.next()?, f.next()?, f.next()?);
        (mark == "1").then(|| Sidebar {
            id: id.into(),
            width: width.into(),
            at: at.into(),
        })
    })
}

/// Left needs a spare column: the pane border sits on the right, so a
/// 2-wide strip would butt icons against the content.
fn default_collapsed_width(left: bool) -> &'static str {
    if left { "3" } else { "2" }
}

/// Width to resize to: expand when collapsed, collapse otherwise.
fn toggled_width<'a>(cur: &str, collapsed: &'a str, expanded: &'a str) -> &'a str {
    if cur == collapsed {
        expanded
    } else {
        collapsed
    }
}

fn is_left() -> bool {
    option("@agents-position", "right") == "left"
}

/// Move the sidebar into the now-active window, keeping its width (and so
/// its expanded/collapsed state). Run from tmux hooks on window/session
/// switches; a no-op when there's no sidebar or it's already there.
pub fn follow() -> io::Result<()> {
    if option("@agents-follow", "on") == "off" {
        return Ok(());
    }
    let Some(sb) = find_sidebar(&tmux(&["list-panes", "-a", "-F", SIDEBAR_FORMAT])?) else {
        return Ok(());
    };
    // Hook context resolves to the client/session that triggered it.
    let target = tmux(&["display-message", "-p", "#{session_name}:#{window_index}"])?;
    let target = target.trim();
    if target == sb.at {
        return Ok(());
    }
    let mut args = vec![
        "join-pane",
        "-d",
        "-h",
        "-f",
        "-l",
        &sb.width,
        "-s",
        &sb.id,
        "-t",
        target,
    ];
    if is_left() {
        args.push("-b");
    }
    // Best-effort: racing with jump()'s own move is fine.
    let _ = tmux(&args);
    Ok(())
}

fn is_hidden_mode() -> bool {
    option("@agents-toggle", "collapsed") == "hidden"
}

fn collapsed_width() -> String {
    option(
        "@agents-collapsed-width",
        default_collapsed_width(is_left()),
    )
}

/// Collapse the sidebar (or kill it in hidden mode); never expands.
pub fn close(pane_id: &str) -> io::Result<()> {
    if is_hidden_mode() {
        tmux(&["kill-pane", "-t", pane_id])?;
    } else {
        tmux(&["resize-pane", "-t", pane_id, "-x", &collapsed_width()])?;
    }
    Ok(())
}

pub fn toggle() -> io::Result<()> {
    let width = option("@agents-width", "32");
    if let Some(sb) = find_sidebar(&tmux(&["list-panes", "-a", "-F", SIDEBAR_FORMAT])?) {
        if is_hidden_mode() {
            tmux(&["kill-pane", "-t", &sb.id])?;
        } else {
            let target = toggled_width(&sb.width, &collapsed_width(), &width).to_string();
            tmux(&["resize-pane", "-t", &sb.id, "-x", &target])?;
        }
        return Ok(());
    }

    let exe = std::env::current_exe()?;
    let exe = exe
        .to_str()
        .ok_or_else(|| io::Error::other("non-utf8 exe path"))?;

    let mut args = vec!["split-window", "-h", "-f", "-l", width.as_str()];
    if is_left() {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_marked_sidebar_pane() {
        let out = "%1\t120\tmain:0\t\n%7\t32\tmain:2\t1\n%9\t80\twork:1\t\n";
        assert_eq!(
            find_sidebar(out),
            Some(Sidebar {
                id: "%7".into(),
                width: "32".into(),
                at: "main:2".into(),
            })
        );
    }

    #[test]
    fn no_sidebar_when_unmarked_or_malformed() {
        assert_eq!(find_sidebar("%1\t120\tmain:0\t\n"), None);
        assert_eq!(find_sidebar("garbage\n"), None);
        assert_eq!(find_sidebar(""), None);
    }

    #[test]
    fn toggle_flips_between_collapsed_and_expanded() {
        assert_eq!(toggled_width("2", "2", "32"), "32");
        assert_eq!(toggled_width("32", "2", "32"), "2");
        // Manually resized to anything else → collapse.
        assert_eq!(toggled_width("50", "2", "32"), "2");
    }

    #[test]
    fn left_sidebar_gets_spare_collapsed_column() {
        assert_eq!(default_collapsed_width(true), "3");
        assert_eq!(default_collapsed_width(false), "2");
    }
}
