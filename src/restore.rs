//! The launcher: bring back the Zellij sessions that were open last time,
//! attach to the one you were in, and keep track of which sessions are open
//! while you work so the next launch knows what to restore.
//!
//! Zellij can't tell a reboot from a deliberate quit (both leave an EXITED
//! session), so we track it ourselves: a session that disappears while the
//! launcher is alive was quit on purpose; one that is still listed as open
//! when the launcher dies (reboot, crash, closed terminal) gets restored.

use crate::config::Config;
use crate::state;
use crate::{config, proc, zellij};
use std::collections::HashMap;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

pub enum Target {
    Restore,
    Attach(String),
    New(Option<String>),
}

/// Polls a session must be missing for before it counts as quit.
const MISSING_POLLS: u32 = 2;
/// An agent that no saved layout refers to yet is kept this long: Zellij only
/// saves layouts periodically, so a new pane isn't in one straight away.
const SLOT_TTL_SECS: u64 = 60 * 60;
/// How many tracker polls between garbage collections.
const GC_EVERY_POLLS: u32 = 30;
/// After the client exits, how long a quit session may take to disappear, and
/// how often to look. The first look is delayed so that a shutdown's signal
/// gets here first; everything dies together then and nothing must change.
const SETTLE_POLL: Duration = Duration::from_millis(300);
const SETTLE_POLLS: u32 = 7;

static TERMINATING: AtomicBool = AtomicBool::new(false);

extern "C" fn on_terminate(_: libc::c_int) {
    TERMINATING.store(true, Ordering::SeqCst);
}

pub fn launch(target: Target) -> i32 {
    if std::env::var_os("ZELLIJ").is_some() {
        eprintln!("za: already inside Zellij (use the session manager to switch sessions)");
        return 1;
    }
    let cfg = config::load();
    let Some(sessions) = zellij::sessions() else {
        eprintln!("za: couldn't list Zellij sessions; is zellij on your PATH?");
        return 1;
    };
    let exists = |n: &str| sessions.iter().any(|s| s.name == n);
    let exited = |n: &str| sessions.iter().any(|s| s.name == n && s.exited);
    if cfg.run_commands_on_restore {
        for session in sessions.iter().filter(|s| s.exited) {
            zellij::autostart_agents(&session.name);
        }
    }

    let mut args: Vec<String> = Vec::new();
    let attach = |args: &mut Vec<String>, name: &str| {
        args.push("attach".into());
        if cfg.run_commands_on_restore {
            args.push("--force-run-commands".into());
        }
        args.push(name.to_string());
    };

    match target {
        Target::Restore => {
            let st = state::load();
            let open: Vec<&str> = st
                .zellij
                .open
                .iter()
                .map(String::as_str)
                .filter(|n| exists(n))
                .collect();
            let pick = st
                .zellij
                .focused
                .as_deref()
                .filter(|f| open.contains(f))
                .or_else(|| open.first().copied())
                .or_else(|| sessions.iter().rev().find(|s| !s.exited).map(|s| s.name.as_str()));

            // Everything else that was open comes back in the background, so
            // it's there (agents resumed) when you switch to it.
            for name in open.iter().filter(|n| exited(n) && Some(**n) != pick) {
                let mut bg = Vec::new();
                attach(&mut bg, name);
                bg.insert(1, "--create-background".into());
                let ok = Command::new("zellij")
                    .args(&bg)
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .status()
                    .is_ok_and(|s| s.success());
                if !ok {
                    eprintln!("za: couldn't resurrect session '{name}'");
                }
            }
            if let Some(name) = pick {
                attach(&mut args, name);
            }
        }
        Target::Attach(name) => attach(&mut args, &name),
        Target::New(Some(name)) => args = vec!["--session".into(), name],
        Target::New(None) => {}
    }

    gc();
    run_tracked(&args, &cfg)
}

fn run_tracked(args: &[String], cfg: &Config) -> i32 {
    let mut child = match Command::new("zellij").args(args).spawn() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("za: couldn't start zellij: {e}");
            return 127;
        }
    };
    proc::ignore_interrupts(true);
    // SAFETY: the handler only stores to an atomic, which is async-signal-safe.
    unsafe {
        libc::signal(libc::SIGHUP, on_terminate as extern "C" fn(libc::c_int) as libc::sighandler_t);
        libc::signal(libc::SIGTERM, on_terminate as extern "C" fn(libc::c_int) as libc::sighandler_t);
    }

    let done = Arc::new(AtomicBool::new(false));
    let interval = Duration::from_secs(cfg.poll_interval_secs.max(1));
    let autostart = cfg.run_commands_on_restore;
    let tracker = {
        let done = done.clone();
        thread::spawn(move || {
            let mut tracker = Tracker {
                autostart,
                ..Tracker::default()
            };
            while !done.load(Ordering::SeqCst) {
                if !TERMINATING.load(Ordering::SeqCst) {
                    tracker.poll(MISSING_POLLS);
                }
                let until = Instant::now() + interval;
                while Instant::now() < until && !done.load(Ordering::SeqCst) {
                    thread::sleep(Duration::from_millis(100));
                }
            }
            tracker
        })
    };

    let status = child.wait();
    done.store(true, Ordering::SeqCst);
    let mut tracker = tracker.join().unwrap_or_default();

    // A clean exit usually follows a quit; record it so that session isn't
    // brought back next time, as soon as the session is gone: the terminal
    // may well be closed right after. During shutdown or a closed terminal we
    // get SIGTERM/SIGHUP, and must leave the state untouched.
    if status.as_ref().is_ok_and(|s| s.success()) {
        for _ in 0..SETTLE_POLLS {
            thread::sleep(SETTLE_POLL);
            if TERMINATING.load(Ordering::SeqCst) || tracker.poll(1) {
                break;
            }
        }
    }
    proc::exit_code(status)
}

#[derive(Default)]
struct Tracker {
    missing: HashMap<String, u32>,
    autostart: bool,
    polls: u32,
}

impl Tracker {
    /// Update the open sessions, dropping any missing for `missing_polls`
    /// polls in a row. Returns whether a session was dropped.
    fn poll(&mut self, missing_polls: u32) -> bool {
        let Some(sessions) = zellij::sessions() else {
            return false;
        };
        self.polls += 1;
        if self.polls.is_multiple_of(GC_EVERY_POLLS) {
            gc();
        }
        if self.autostart {
            for session in sessions.iter().filter(|s| s.exited) {
                zellij::autostart_agents(&session.name);
            }
        }
        let live: Vec<String> = sessions
            .into_iter()
            .filter(|s| !s.exited)
            .map(|s| s.name)
            .collect();
        let connected: Vec<&String> = live
            .iter()
            .filter(|n| zellij::connected_clients(n).unwrap_or(0) > 0)
            .collect();

        let mut dropped = false;
        let _ = state::update(|st| {
            let z = &mut st.zellij;
            let mut open = Vec::new();
            for name in &z.open {
                if live.contains(name) {
                    self.missing.remove(name);
                    open.push(name.clone());
                } else {
                    let polls = self.missing.entry(name.clone()).or_insert(0);
                    *polls += 1;
                    if *polls < missing_polls {
                        open.push(name.clone());
                    } else {
                        dropped = true;
                    }
                }
            }
            for name in &live {
                if !open.contains(name) {
                    open.push(name.clone());
                }
            }
            let focused = match &z.focused {
                Some(f) if connected.contains(&f) => Some(f.clone()),
                prev => connected
                    .first()
                    .map(|s| s.to_string())
                    .or_else(|| prev.clone().filter(|f| open.contains(f))),
            };
            if open != z.open || focused != z.focused {
                z.open = open;
                z.focused = focused;
                z.updated_at = state::now();
            }
        });
        dropped
    }
}

/// Forget slots that can't come back: nothing refers to them, and they're
/// finished or too old to be a pane Zellij just hasn't saved yet.
pub fn gc() -> usize {
    let Some(referenced) = zellij::referenced_slots() else {
        return 0;
    };
    let now = state::now();
    state::update(|st| {
        let before = st.slots.len();
        st.slots.retain(|id, slot| {
            referenced.contains(id)
                || (slot.is_active() && now.saturating_sub(slot.updated_at) < SLOT_TTL_SECS)
        });
        before - st.slots.len()
    })
    .unwrap_or(0)
}
