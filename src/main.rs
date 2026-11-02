mod agents;
mod config;
mod hook;
mod install;
mod proc;
mod restore;
mod run;
mod state;
mod zellij;

use std::process::ExitCode;

const USAGE: &str = "\
za - resume AI agent sessions across Zellij restarts

Usage:
  za                        Restore the sessions open last time and attach
  za attach <session>       Attach to (or resurrect) one session
  za new [session]          Start a new session
  za run <agent> [args...]  Run an agent in this pane so it can be resumed
  za hook <agent>           Agent hook entry point (set up by `za install`)
  za install [agent|all]    Add za's session hook to agent configs
  za uninstall [agent|all]  Remove za's session hook from agent configs
  za init <zsh|bash|fish>   Print shell functions that route agents through za
  za status                 Show tracked sessions and agent panes
  za gc                     Forget agent panes no saved layout refers to

Agents: claude, copilot, kimi, agy, grok, codex
Files:  ~/.config/za/state.json, ~/.config/za/config.toml
";

fn main() -> ExitCode {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let cmd = if args.is_empty() {
        "restore".to_string()
    } else {
        args.remove(0)
    };
    let arg = |i: usize| args.get(i).map(String::as_str);

    let code = match cmd.as_str() {
        "restore" => restore::launch(restore::Target::Restore),
        "attach" => match arg(0) {
            Some(name) => restore::launch(restore::Target::Attach(name.to_string())),
            None => usage_error("za attach <session>"),
        },
        "new" => restore::launch(restore::Target::New(arg(0).map(String::from))),
        "run" => run::run(args),
        "hook" => hook::hook(arg(0).unwrap_or_default()),
        "install" => install::install(arg(0).unwrap_or("all")),
        "uninstall" => install::uninstall(arg(0).unwrap_or("all")),
        "init" => install::init(arg(0).unwrap_or_default()),
        "status" => status(),
        "gc" => {
            println!("forgot {} agent pane(s)", restore::gc());
            0
        }
        "-h" | "--help" | "help" => {
            print!("{USAGE}");
            0
        }
        "-V" | "--version" => {
            println!("za {}", env!("CARGO_PKG_VERSION"));
            0
        }
        other => {
            eprintln!("za: unknown command '{other}'\n");
            eprint!("{USAGE}");
            2
        }
    };
    ExitCode::from(u8::try_from(code).unwrap_or(1))
}

fn usage_error(usage: &str) -> i32 {
    eprintln!("usage: {usage}");
    2
}

fn status() -> i32 {
    let st = state::load();
    println!("state:  {}", state::path().display());
    match config::try_load() {
        Ok(_) => println!("config: {}", config::path().display()),
        Err(e) => println!("config: {} (invalid, using defaults: {e})", config::path().display()),
    }

    println!("\nsessions open at last check:");
    if st.zellij.open.is_empty() {
        println!("  (none)");
    }
    for name in &st.zellij.open {
        let mark = if st.zellij.focused.as_ref() == Some(name) { "  <- focused" } else { "" };
        println!("  {name}{mark}");
    }

    // Only what the next `za` would bring back. Finished panes linger in the
    // state until saved layouts stop referring to them, but they're not agents.
    let (restoring, quit): (Vec<_>, Vec<_>) = st
        .slots
        .iter()
        .filter(|(_, slot)| slot.is_active())
        .partition(|(_, slot)| {
            slot.zellij_session
                .as_ref()
                .is_some_and(|name| st.zellij.open.contains(name))
        });
    println!("\nagent panes:");
    if restoring.is_empty() {
        println!("  (none)");
    }
    for (id, slot) in &restoring {
        println!(
            "  {id}  {:<8} {:<38} {:<24} {}",
            slot.agent,
            slot.session_id.as_deref().unwrap_or("(no session yet)"),
            slot.zellij_session.as_deref().unwrap_or("-"),
            slot.cwd.as_deref().unwrap_or("-"),
        );
    }
    if !quit.is_empty() {
        println!(
            "\n{} more in sessions you quit (they come back with `za attach <session>`)",
            quit.len()
        );
    }
    0
}
