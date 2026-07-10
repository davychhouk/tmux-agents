mod app;
mod classify;
mod detect;
mod tmux;
mod toggle;

fn main() -> std::io::Result<()> {
    match std::env::args().nth(1).as_deref() {
        Some("toggle") => toggle::toggle(),
        Some("follow") => toggle::follow(),
        None => app::run(),
        Some(other) => {
            eprintln!("unknown command: {other}\nusage: tmux-agents [toggle|follow]");
            std::process::exit(2);
        }
    }
}
