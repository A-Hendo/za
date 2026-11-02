//! `za hook <agent>`: called by an agent's session-start hook with the hook
//! payload on stdin. Records the agent's session id against the pane's slot.
//!
//! Must be fast, silent and always succeed: a failing hook can block or
//! clutter the agent it runs in.

use crate::agents::{self, Agent};
use crate::proc;
use crate::state::{self, Slot};
use serde_json::Value;
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::Path;

pub fn hook(name: &str) -> i32 {
    let mut input = String::new();
    let _ = std::io::stdin().take(8 << 20).read_to_string(&mut input);
    if let Some(agent) = agents::find(name) {
        record(agent, &input);
        let mut out = std::io::stdout();
        let _ = out.write_all(agent.hook_stdout.as_bytes());
        let _ = out.flush();
    }
    0
}

fn record(agent: &Agent, input: &str) -> Option<()> {
    let payload: Value = serde_json::from_str(input).unwrap_or(Value::Null);
    if let Some(key) = agent.subagent_key {
        if payload.get(key).and_then(Value::as_str).is_some_and(|s| !s.is_empty()) {
            return None;
        }
    }
    let session_id = agent.session_id(&payload)?;
    let payload_cwd = payload.get("cwd").and_then(Value::as_str).map(String::from);

    if let Some(slot) = own_slot(agent) {
        let cwd = payload_cwd
            .or_else(|| std::env::current_dir().ok().map(|p| p.to_string_lossy().into_owned()));
        return state::update(|st| {
            let s = st.slots.entry(slot).or_insert_with(|| Slot {
                agent: agent.name.to_string(),
                created_at: state::now(),
                ..Slot::default()
            });
            if s.agent == agent.name {
                s.session_id = Some(session_id);
                s.cwd = cwd.or(s.cwd.take());
                s.updated_at = state::now();
            }
        })
        .ok();
    }

    // Fired from the agent's shared server rather than from a pane: nothing
    // says which pane this is, so go by the directory the session runs in.
    let server = agent.server_arg?;
    let cwd = payload_cwd?;
    if !proc::ancestor_has_arg(server) {
        return None;
    }
    state::update(|st| {
        let id = match_by_folder(&st.slots, agent.name, &session_id, &cwd, |s| {
            s.pid.is_some_and(proc::is_live_za)
        })?;
        let s = st.slots.get_mut(&id)?;
        s.session_id = Some(session_id);
        s.updated_at = state::now();
        Some(())
    })
    .ok()
    .flatten()
}

/// The pane slot whose agent fired this hook, from the environment `za run`
/// gave it. Absent outside a za pane, and for hooks run by a shared server
/// that merely inherited some pane's environment.
fn own_slot(agent: &Agent) -> Option<String> {
    // Set by `za run` for the agent it spawned.
    let slot = std::env::var("ZA_SLOT").ok()?;
    if std::env::var("ZA_AGENT").ok()? != agent.name {
        return None;
    }
    let wrapper: i32 = std::env::var("ZA_PID").ok()?.parse().ok()?;
    proc::hook_belongs_to(wrapper).then_some(slot)
}

/// Which live pane a session started in `cwd` belongs to, when the hook
/// can't tell. With one such pane per directory this is exact; panes sharing
/// a directory may end up with each other's sessions.
///
/// A session some slot already has is left alone (a pane resuming). Panes
/// still waiting for their first session are filled oldest first. A new
/// session in a pane that already had one (`/clear`, `/resume`) is only
/// recorded when there's a single candidate, rather than guessing.
fn match_by_folder(
    slots: &BTreeMap<String, Slot>,
    agent: &str,
    session_id: &str,
    cwd: &str,
    alive: impl Fn(&Slot) -> bool,
) -> Option<String> {
    let ours = |s: &&Slot| s.agent == agent;
    if slots.values().filter(ours).any(|s| s.session_id.as_deref() == Some(session_id)) {
        return None;
    }
    let target = canonical(cwd);
    let candidates: Vec<(&String, &Slot)> = slots
        .iter()
        .filter(|(_, s)| {
            s.agent == agent
                && s.is_active()
                && s.cwd.as_deref().is_some_and(|d| canonical(d) == target)
                && alive(s)
        })
        .collect();
    let waiting = candidates
        .iter()
        .filter(|(_, s)| s.session_id.is_none())
        .min_by_key(|(id, s)| (s.created_at, *id));
    match (waiting, candidates.as_slice()) {
        (Some((id, _)), _) => Some((*id).clone()),
        (None, [(id, _)]) => Some((*id).clone()),
        _ => None,
    }
}

fn canonical(dir: &str) -> String {
    std::fs::canonicalize(Path::new(dir))
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|_| dir.trim_end_matches('/').to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slot(cwd: &str, session: Option<&str>, created_at: u64) -> Slot {
        Slot {
            agent: "codex".into(),
            cwd: Some(cwd.into()),
            session_id: session.map(String::from),
            created_at,
            pid: Some(1),
            ..Slot::default()
        }
    }

    fn slots(list: &[(&str, Slot)]) -> BTreeMap<String, Slot> {
        list.iter().map(|(id, s)| (id.to_string(), s.clone())).collect()
    }

    #[test]
    fn folder_match_fills_waiting_panes_oldest_first() {
        let st = slots(&[
            ("b", slot("/no/such/r", None, 20)),
            ("a", slot("/no/such/r", None, 10)),
            ("c", slot("/no/such/other", None, 5)),
        ]);
        let m = |id, cwd| match_by_folder(&st, "codex", id, cwd, |_| true);
        assert_eq!(m("s1", "/no/such/r").as_deref(), Some("a"));
        assert_eq!(m("s1", "/no/such/r/").as_deref(), Some("a"));
        assert_eq!(m("s1", "/no/such/other").as_deref(), Some("c"));
        assert_eq!(m("s1", "/no/such/elsewhere"), None);
    }

    #[test]
    fn folder_match_leaves_known_sessions_and_guesses_never() {
        let st = slots(&[
            ("a", slot("/no/such/r", Some("s1"), 10)),
            ("b", slot("/no/such/r", Some("s2"), 20)),
        ]);
        let m = |id| match_by_folder(&st, "codex", id, "/no/such/r", |_| true);
        assert_eq!(m("s1"), None);
        // A new session with two panes that both already have one: unknown.
        assert_eq!(m("s3"), None);

        let one = slots(&[("a", slot("/no/such/r", Some("s1"), 10))]);
        assert_eq!(
            match_by_folder(&one, "codex", "s3", "/no/such/r", |_| true).as_deref(),
            Some("a")
        );
    }

    #[test]
    fn folder_match_skips_dead_finished_and_other_agents() {
        let mut ended = slot("/no/such/r", None, 1);
        ended.ended = true;
        let mut claude = slot("/no/such/r", None, 2);
        claude.agent = "claude".into();
        let mut dead = slot("/no/such/r", None, 3);
        dead.pid = Some(2);
        let st = slots(&[("e", ended), ("c", claude), ("d", dead)]);
        assert_eq!(
            match_by_folder(&st, "codex", "s1", "/no/such/r", |s| s.pid == Some(1)),
            None
        );
    }
}
