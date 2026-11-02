//! `za hook <agent>`: called by an agent's session-start hook with the hook
//! payload on stdin. Records the agent's session id against the pane's slot.
//!
//! Must be fast, silent and always succeed: a failing hook can block or
//! clutter the agent it runs in.

use crate::agents::{self, Agent};
use crate::state::{self, Slot};
use crate::proc;
use serde_json::Value;
use std::io::{Read, Write};

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
    // Set by `za run` for the agent it spawned; absent outside a za pane.
    let slot = std::env::var("ZA_SLOT").ok()?;
    if std::env::var("ZA_AGENT").ok()? != agent.name {
        return None;
    }
    let wrapper: i32 = std::env::var("ZA_PID").ok()?.parse().ok()?;

    let payload: Value = serde_json::from_str(input).unwrap_or(Value::Null);
    if let Some(key) = agent.subagent_key {
        if payload.get(key).and_then(Value::as_str).is_some_and(|s| !s.is_empty()) {
            return None;
        }
    }
    let session_id = agent.session_id(&payload)?;
    if !proc::hook_belongs_to(wrapper) {
        return None;
    }
    let cwd = payload
        .get("cwd")
        .and_then(Value::as_str)
        .map(String::from)
        .or_else(|| std::env::current_dir().ok().map(|p| p.to_string_lossy().into_owned()));

    state::update(|st| {
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
    .ok()
}
