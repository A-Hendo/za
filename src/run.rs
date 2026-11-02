//! `za run <agent> [args]`: the per-pane wrapper.
//!
//! First launch re-execs as `za run --slot <id> <agent> [args]`. Zellij saves
//! that command line verbatim, so after a resurrect the same slot comes back
//! and za can look up which conversation the pane was in.

use crate::agents::{self, Agent, Launch};
use crate::state::{self, Slot};
use crate::{config, proc, zellij};
use std::collections::hash_map::RandomState;
use std::hash::{BuildHasher, Hasher};
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Child, Command, ExitStatus};
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant, SystemTime};

/// A resume that dies this quickly with an error almost certainly means the
/// saved conversation no longer exists.
const FAILED_RESUME_WINDOW: Duration = Duration::from_secs(4);
/// How often the wrapper checks on its child and for signals.
const WAIT_POLL: Duration = Duration::from_millis(100);
/// Pauses before each check that a hung-up pane was really closed. A session
/// being quit hangs up its panes too, and is gone by the second check.
const CLOSE_CHECKS: [Duration; 2] = [Duration::from_millis(500), Duration::from_millis(700)];
/// How long to wait for Zellij to hang up on us after asking it to close the pane.
const CLOSE_WAIT: Duration = Duration::from_secs(2);

static HANGUP: AtomicBool = AtomicBool::new(false);
static TERMINATED: AtomicBool = AtomicBool::new(false);

extern "C" fn on_hangup(_: libc::c_int) {
    HANGUP.store(true, Ordering::SeqCst);
}

extern "C" fn on_terminate(_: libc::c_int) {
    TERMINATED.store(true, Ordering::SeqCst);
}

/// How an agent stopped.
struct AgentExit {
    code: i32,
    /// It exited by itself, as opposed to being killed or failing to start.
    quit: bool,
}

pub fn run(mut args: Vec<String>) -> i32 {
    let mut slot = None;
    if args.first().is_some_and(|a| a == "--slot") {
        if args.len() < 2 {
            eprintln!("za: --slot needs a value");
            return 2;
        }
        slot = Some(args[1].clone());
        args.drain(..2);
    }
    if args.is_empty() {
        eprintln!("usage: za run <agent> [args...]");
        return 2;
    }
    let name = args.remove(0);

    let agent = agents::find(&name).filter(|a| !a.is_passthrough(&args));
    let Some(agent) = agent else {
        return exec(Command::new(&name).args(&args));
    };
    match slot {
        Some(slot) => supervise(agent, &slot, &args),
        // Only claim a slot for agents started in a Zellij pane, and never for
        // an agent nested inside another wrapped agent.
        None if std::env::var_os("ZELLIJ_PANE_ID").is_none()
            || std::env::var_os("ZA_SLOT").is_some() =>
        {
            exec(Command::new(&name).args(&args))
        }
        None => match std::env::current_exe() {
            Ok(me) => exec(
                Command::new(me)
                    .args(["run", "--slot", &new_slot_id(), &name])
                    .args(&args),
            ),
            Err(_) => exec(Command::new(&name).args(&args)),
        },
    }
}

fn supervise(agent: &Agent, slot: &str, args: &[String]) -> i32 {
    // SAFETY: the handlers only store to atomics, which is async-signal-safe.
    // Handlers (unlike SIG_IGN) reset to the default in spawned children.
    unsafe {
        libc::signal(libc::SIGHUP, on_hangup as extern "C" fn(libc::c_int) as libc::sighandler_t);
        libc::signal(libc::SIGTERM, on_terminate as extern "C" fn(libc::c_int) as libc::sighandler_t);
    }
    let cfg = config::load();
    let here = std::env::current_dir()
        .ok()
        .map(|p| p.to_string_lossy().into_owned());
    let recorded = state::update(|st| {
        let previous = st.slots.get(slot).cloned();
        let rec = st.slots.entry(slot.to_string()).or_insert_with(|| Slot {
            agent: agent.name.to_string(),
            cwd: here.clone(),
            created_at: state::now(),
            ..Slot::default()
        });
        rec.zellij_session = std::env::var("ZELLIJ_SESSION_NAME").ok();
        rec.updated_at = state::now();
        previous
    });
    let previous = match recorded {
        Ok(prev) => prev.filter(|p| p.agent == agent.name),
        Err(e) => {
            eprintln!("za: could not save state ({e}); running {} without resume", agent.name);
            None
        }
    };

    // No previous record: a brand new agent, run exactly as typed.
    let Some(previous) = previous else {
        let exit = spawn_and_wait(agent, slot, args, None);
        return finish(&cfg, slot, exit);
    };
    if previous.closed || (previous.ended && !cfg.shell_after_exit) {
        close_own_pane(slot);
        return 0;
    }
    if previous.ended {
        return finish(&cfg, slot, AgentExit { code: 0, quit: true });
    }

    let cwd = previous.cwd.as_deref().filter(|d| Path::new(d).is_dir());
    let launch = match previous.session_id.as_deref() {
        Some(id) => Launch::Resume(id),
        None if cfg.continue_fallback => Launch::Continue,
        None => Launch::Fresh,
    };
    let resuming = !matches!(launch, Launch::Fresh);
    let started = Instant::now();
    let mut exit = spawn_and_wait(agent, slot, &agent.launch_args(args, launch), cwd);

    if resuming && exit.code != 0 && started.elapsed() < FAILED_RESUME_WINDOW {
        eprintln!(
            "za: couldn't resume the previous {} session; starting a new one",
            agent.name
        );
        let _ = state::update(|st| {
            if let Some(s) = st.slots.get_mut(slot) {
                s.session_id = None;
            }
        });
        exit = spawn_and_wait(agent, slot, &agent.launch_args(args, Launch::Fresh), cwd);
    }
    finish(&cfg, slot, exit)
}

fn spawn_and_wait(agent: &Agent, slot: &str, args: &[String], cwd: Option<&str>) -> AgentExit {
    let mut cmd = Command::new(agent.name);
    cmd.args(args)
        .env("ZA_SLOT", slot)
        .env("ZA_PID", std::process::id().to_string())
        .env("ZA_AGENT", agent.name);
    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("za: {}: {e}", agent.name);
            return AgentExit { code: 127, quit: false };
        }
    };
    let status = wait_in_pane(&mut child, slot);
    AgentExit {
        quit: status.as_ref().is_ok_and(|s| s.code().is_some()),
        code: proc::exit_code(status),
    }
}

/// Wait for the agent or shell in this pane, or leave if the pane goes away.
fn wait_in_pane(child: &mut Child, slot: &str) -> std::io::Result<ExitStatus> {
    proc::ignore_interrupts(true);
    let status = loop {
        let done = child.try_wait();
        // Checked after try_wait: a child that dies in a shutdown or a closing
        // pane didn't quit, even if it finished before we got to look.
        if HANGUP.load(Ordering::SeqCst) || TERMINATED.load(Ordering::SeqCst) {
            let running = matches!(done, Ok(None));
            leave_pane(child, slot, running);
        }
        match done {
            Ok(Some(status)) => break Ok(status),
            Ok(None) => thread::sleep(WAIT_POLL),
            Err(e) => break Err(e),
        }
    };
    proc::ignore_interrupts(false);
    status
}

/// We were signalled, so the pane or everything around it is going away. Only
/// a closed pane may change what comes back: reboots and quits must not.
fn leave_pane(child: &Child, slot: &str, running: bool) -> ! {
    let signal = if TERMINATED.load(Ordering::SeqCst) {
        libc::SIGTERM
    } else {
        libc::SIGHUP
    };
    if running {
        // SAFETY: `child` hasn't been reaped, so its pid is still ours to signal.
        unsafe { libc::kill(child.id() as i32, signal) };
    }
    if signal == libc::SIGHUP && pane_was_closed() {
        set_closed(slot);
    }
    std::process::exit(128 + signal);
}

fn pane_was_closed() -> bool {
    let (Ok(session), Ok(pane)) = (
        std::env::var("ZELLIJ_SESSION_NAME"),
        std::env::var("ZELLIJ_PANE_ID"),
    ) else {
        return false;
    };
    CLOSE_CHECKS.iter().all(|pause| {
        thread::sleep(*pause);
        !TERMINATED.load(Ordering::SeqCst) && zellij::pane_exists(&session, &pane) == Some(false)
    })
}

fn set_closed(slot: &str) {
    let _ = state::update(|st| {
        if let Some(s) = st.slots.get_mut(slot) {
            s.closed = true;
        }
    });
}

/// The agent is done. Record that so it doesn't come back on the next restore,
/// unless it was killed rather than quit. A resurrected pane has no shell
/// behind it, so give it one, as if it had been started from one.
fn finish(cfg: &config::Config, slot: &str, exit: AgentExit) -> i32 {
    if exit.quit {
        let _ = state::update(|st| {
            if let Some(s) = st.slots.get_mut(slot) {
                s.ended = true;
            }
        });
    }
    if cfg.shell_after_exit && proc::parent_is_zellij() {
        return shell_then_close(slot);
    }
    exit.code
}

/// Run a shell in a resurrected pane. Zellij would hold this pane on an
/// "EXIT CODE" screen whenever its command exits, so a shell that simply
/// exited (ctrl+d) wouldn't close it. Stay alive and close the pane ourselves.
fn shell_then_close(slot: &str) -> i32 {
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into());
    let code = match Command::new(&shell).spawn() {
        Ok(mut child) => proc::exit_code(wait_in_pane(&mut child, slot)),
        Err(e) => {
            eprintln!("za: {shell}: {e}");
            127
        }
    };
    close_own_pane(slot);
    code
}

/// Close this pane and wait to be hung up on, so that Zellij never sees the
/// command exit and flashes the "EXIT CODE" screen.
fn close_own_pane(slot: &str) {
    set_closed(slot);
    let (Ok(session), Ok(pane)) = (
        std::env::var("ZELLIJ_SESSION_NAME"),
        std::env::var("ZELLIJ_PANE_ID"),
    ) else {
        return;
    };
    if zellij::close_pane(&session, &pane) {
        let asked = Instant::now();
        while !HANGUP.load(Ordering::SeqCst) && asked.elapsed() < CLOSE_WAIT {
            thread::sleep(WAIT_POLL);
        }
    }
}

fn exec(cmd: &mut Command) -> i32 {
    let err = cmd.exec();
    eprintln!("za: {}: {err}", cmd.get_program().to_string_lossy());
    127
}

/// 8 base36 chars from a randomly seeded hasher, checked against existing slots.
fn new_slot_id() -> String {
    let existing = state::load().slots;
    loop {
        let mut h = RandomState::new().build_hasher();
        h.write_u128(
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos()),
        );
        h.write_u32(std::process::id());
        let mut n = h.finish();
        let mut id = String::with_capacity(8);
        for _ in 0..8 {
            id.push(char::from_digit((n % 36) as u32, 36).unwrap_or('0'));
            n /= 36;
        }
        if !existing.contains_key(&id) {
            return id;
        }
    }
}
