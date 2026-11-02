//! Talking to Zellij: its CLI for session lists, and its cache directory for
//! the layouts and metadata it serializes.

use crate::state;
use serde_json::Value;
use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

/// A session that is shutting down may never answer.
const ACTION_TIMEOUT: Duration = Duration::from_secs(2);

pub struct Session {
    pub name: String,
    pub exited: bool,
}

/// All sessions Zellij knows about, live and resurrectable. `None` when the
/// list couldn't be read, which callers must not mistake for "no sessions".
pub fn sessions() -> Option<Vec<Session>> {
    let out = Command::new("zellij")
        .args(["list-sessions", "--no-formatting"])
        .stdin(Stdio::null())
        .output()
        .ok()?;
    let stdout = String::from_utf8_lossy(&out.stdout);
    if !out.status.success() && !String::from_utf8_lossy(&out.stderr).contains("No active") {
        return None;
    }
    Some(
        stdout
            .lines()
            .filter_map(|line| {
                let (name, rest) = line.split_once(" [Created")?;
                Some(Session {
                    name: name.trim().to_string(),
                    exited: rest.contains("EXITED"),
                })
            })
            .filter(|s| !s.name.is_empty())
            .collect(),
    )
}

fn cache_dir() -> PathBuf {
    if cfg!(target_os = "macos") {
        return state::home().join("Library/Caches/org.Zellij-Contributors.Zellij");
    }
    std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| state::home().join(".cache"))
        .join("zellij")
}

/// `<cache>/contract_version_*/session_info` for every Zellij version present.
fn session_info_dirs() -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(cache_dir()) else {
        return Vec::new();
    };
    entries
        .filter_map(Result::ok)
        .filter(|e| e.file_name().to_string_lossy().starts_with("contract_version_"))
        .map(|e| e.path().join("session_info"))
        .filter(|p| p.is_dir())
        .collect()
}

/// How many clients are attached to a live session, from the metadata file
/// Zellij keeps current while the session runs.
pub fn connected_clients(session: &str) -> Option<u32> {
    session_info_dirs().into_iter().find_map(|dir| {
        let meta = fs::read_to_string(dir.join(session).join("session-metadata.kdl")).ok()?;
        meta.lines()
            .find_map(|l| l.strip_prefix("connected_clients "))
            .and_then(|n| n.trim().parse().ok())
    })
}

/// Run `zellij --session <session> action <args>`, returning stdout on success.
fn action(session: &str, args: &[&str]) -> Option<String> {
    let mut cmd = Command::new("zellij");
    cmd.args(["--session", session, "action"])
        .args(args)
        .stdin(Stdio::null())
        .stderr(Stdio::null());
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || tx.send(cmd.output()));
    let out = rx.recv_timeout(ACTION_TIMEOUT).ok()?.ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Whether a live session still has this terminal pane. `None` means the
/// session couldn't answer (it's gone or going), which says nothing about
/// the pane.
pub fn pane_exists(session: &str, pane_id: &str) -> Option<bool> {
    let id: u64 = pane_id.parse().ok()?;
    terminal_pane_listed(&action(session, &["list-panes", "--json"])?, id)
}

fn terminal_pane_listed(json: &str, id: u64) -> Option<bool> {
    let panes: Value = serde_json::from_str(json).ok()?;
    Some(panes.as_array()?.iter().any(|p| {
        p.get("is_plugin").and_then(Value::as_bool) == Some(false)
            && p.get("id").and_then(Value::as_u64) == Some(id)
    }))
}

pub fn close_pane(session: &str, pane_id: &str) -> bool {
    let target = format!("terminal_{pane_id}");
    action(session, &["close-pane", "--pane-id", &target]).is_some()
}

/// Zellij resurrects a session's commands suspended ("press ENTER to run")
/// unless it was attached with `--force-run-commands`, which za can't pass
/// when the session manager does the resurrecting. Agent panes are za's own,
/// so drop the flag from their saved layout instead.
pub fn autostart_agents(session: &str) {
    for dir in session_info_dirs() {
        let path = dir.join(session).join("session-layout.kdl");
        let Ok(layout) = fs::read_to_string(&path) else {
            continue;
        };
        let stripped = without_agent_suspend(&layout);
        if stripped != layout {
            let tmp = path.with_extension("kdl.za-tmp");
            if fs::write(&tmp, stripped).is_ok() {
                let _ = fs::rename(&tmp, &path);
            }
        }
    }
}

/// `layout` minus `start_suspended true` in panes that run `za run --slot`.
fn without_agent_suspend(layout: &str) -> String {
    let mut out = String::with_capacity(layout.len());
    let mut in_agent_pane = false;
    for line in layout.split_inclusive('\n') {
        let trimmed = line.trim();
        if trimmed.starts_with("args \"run\" \"--slot\" ") {
            in_agent_pane = true;
        } else if trimmed == "}" {
            in_agent_pane = false;
        } else if in_agent_pane && trimmed == "start_suspended true" {
            continue;
        }
        out.push_str(line);
    }
    out
}

/// Slot ids referenced by any saved session layout. `None` if Zellij's cache
/// can't be found, so callers know not to garbage-collect blindly.
pub fn referenced_slots() -> Option<HashSet<String>> {
    let dirs = session_info_dirs();
    if dirs.is_empty() {
        return None;
    }
    let mut ids = HashSet::new();
    for dir in dirs {
        let Ok(sessions) = fs::read_dir(dir) else {
            continue;
        };
        for session in sessions.filter_map(Result::ok) {
            if let Ok(layout) = fs::read_to_string(session.path().join("session-layout.kdl")) {
                ids.extend(slot_ids_in(&layout));
            }
        }
    }
    Some(ids)
}

/// Finds `"--slot" "<id>"` pairs in a serialized layout's args.
fn slot_ids_in(layout: &str) -> Vec<String> {
    const NEEDLE: &str = "\"--slot\" \"";
    layout
        .match_indices(NEEDLE)
        .filter_map(|(at, _)| {
            let rest = &layout[at + NEEDLE.len()..];
            Some(rest[..rest.find('"')?].to_string())
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_slots_in_layout() {
        let layout = r#"
            pane command="/home/u/.cargo/bin/za" cwd="repo" {
                args "run" "--slot" "k3f9ab12" "claude" "--model" "opus"
                start_suspended true
            }
            pane command="nvim" { args "." }
            pane command="za" { args "run" "--slot" "zz00" "kimi" }
        "#;
        assert_eq!(slot_ids_in(layout), vec!["k3f9ab12", "zz00"]);
    }

    #[test]
    fn only_agent_panes_lose_start_suspended() {
        let layout = r#"layout {
    pane command="/home/u/.cargo/bin/za" cwd="repo" focus=true {
        args "run" "--slot" "k3f9ab12" "claude"
        start_suspended true
    }
    pane command="nvim" {
        args "."
        start_suspended true
    }
}
"#;
        let stripped = without_agent_suspend(layout);
        assert_eq!(stripped.matches("start_suspended true").count(), 1);
        assert!(stripped.contains("\"claude\"\n    }\n"));
        assert!(stripped.contains("args \".\"\n        start_suspended true"));
        assert_eq!(without_agent_suspend(&stripped), stripped);
    }

    #[test]
    fn finds_terminal_panes_only() {
        let json = r#"[
            {"id": 3, "is_plugin": false},
            {"id": 7, "is_plugin": true}
        ]"#;
        assert_eq!(terminal_pane_listed(json, 3), Some(true));
        assert_eq!(terminal_pane_listed(json, 7), Some(false));
        assert_eq!(terminal_pane_listed("not json", 3), None);
    }
}
