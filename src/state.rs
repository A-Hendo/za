//! Persistent state in `~/.config/za/state.json`.
//!
//! Hooks, wrappers and launchers all write here concurrently, so every
//! mutation goes through [`update`], which holds an exclusive lock and
//! replaces the file atomically.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::os::unix::io::AsRawFd;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct State {
    pub version: u32,
    pub zellij: ZellijState,
    pub slots: BTreeMap<String, Slot>,
}

/// Which Zellij sessions were running the last time a tracker looked.
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ZellijState {
    pub open: Vec<String>,
    pub focused: Option<String>,
    pub updated_at: u64,
}

/// One agent pane. The slot id is baked into the pane's command line, so it
/// survives Zellij resurrection even though pane ids do not.
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Slot {
    pub agent: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub zellij_session: Option<String>,
    /// The agent was quit in a resurrected pane that then became a shell.
    /// Zellij keeps re-running the pane's original command, so later
    /// restores must give it the shell, not the agent.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub ended: bool,
    /// The pane itself was closed. A saved layout can lag behind by up to a
    /// serialization interval, so if one still brings the pane back it must
    /// close itself again.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub closed: bool,
    pub created_at: u64,
    pub updated_at: u64,
}

impl Slot {
    /// Whether a restore would bring this agent back.
    pub fn is_active(&self) -> bool {
        !self.ended && !self.closed
    }
}

pub fn home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/"))
}

/// `$ZA_HOME`, else `$XDG_CONFIG_HOME/za`, else `~/.config/za`.
pub fn dir() -> PathBuf {
    if let Some(d) = std::env::var_os("ZA_HOME") {
        return PathBuf::from(d);
    }
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| home().join(".config"))
        .join("za")
}

pub fn path() -> PathBuf {
    dir().join("state.json")
}

pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

pub fn load() -> State {
    fs::read(path())
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

/// Read-modify-write the state under an exclusive lock. The file is only
/// rewritten when `f` actually changed something.
pub fn update<T>(f: impl FnOnce(&mut State) -> T) -> io::Result<T> {
    let dir = dir();
    fs::create_dir_all(&dir)?;
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(dir.join("state.lock"))?;
    // SAFETY: `lock` is a valid open fd; the lock is released when it is dropped.
    if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX) } != 0 {
        return Err(io::Error::last_os_error());
    }

    let mut st = load();
    let before = st.clone();
    let out = f(&mut st);
    if st != before {
        st.version = 1;
        let tmp = dir.join(format!("state.json.{}.tmp", std::process::id()));
        let mut file = File::create(&tmp)?;
        file.write_all(&serde_json::to_vec_pretty(&st)?)?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        fs::rename(&tmp, path())?;
    }
    Ok(out)
}
